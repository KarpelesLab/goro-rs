//! The mb_* functions (PHP manual: Multibyte String Functions).

use goro_charset::ops::{self, CaseMode, TrimMode};
use goro_charset::{Encoding, SubstMode};
use goro_runtime::{Key, KeyRef, ObjRef, PhpArray, PhpRef, PhpStr, Value};

use super::*;

// ---------------------------------------------------------------- settings

pub(crate) fn mb_language(vm: &mut Vm, a: &mut Args) -> R<Value> {
    match a.opt_str(0) {
        None => {
            let l = st(vm).lang;
            Ok(Value::from(LANGS[l].name))
        }
        Some(n) => match find_lang(&n) {
            Some(l) => {
                st(vm).lang = l;
                vm.set_ini(b"mbstring.language", LANGS[l].name.as_bytes());
                Ok(Value::Bool(true))
            }
            None => {
                // the failed change leaves the startup language (test
                // mb_language.phpt)
                let s = st(vm);
                s.lang = s.lang0;
                vm.fthrow(
                    a,
                    "ValueError",
                    &format!(
                        "Argument #1 ($language) must be a valid language, \"{}\" given",
                        n.to_string_lossy()
                    ),
                )
            }
        },
    }
}

pub(crate) fn mb_internal_encoding(vm: &mut Vm, a: &mut Args) -> R<Value> {
    match a.opt_str(0) {
        None => {
            let e = internal(vm);
            Ok(Value::from(e.name()))
        }
        Some(n) => match Encoding::from_name(&n) {
            Some(e) => {
                let s = st(vm);
                s.internal = e;
                s.internal_func = true;
                Ok(Value::Bool(true))
            }
            None => vm.fthrow(
                a,
                "ValueError",
                &format!(
                    "Argument #1 ($encoding) must be a valid encoding, \"{}\" given",
                    n.to_string_lossy()
                ),
            ),
        },
    }
}

fn last_input(vm: &mut Vm) -> Value {
    match st(vm).last_input {
        Some(e) => Value::from(e.name()),
        None => Value::Bool(false),
    }
}

/// No HTTP input in the CLI: "I" and "L" give the configured input list
/// (observed on PHP 8.5).
pub(crate) fn mb_http_input(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(t) = a.opt_str(0) else {
        return Ok(last_input(vm));
    };
    let c = if t.len() == 1 { t.first() } else { None };
    match c.map(|c| c.to_ascii_uppercase()) {
        Some(b'G' | b'P' | b'C' | b'S') => Ok(last_input(vm)),
        Some(b'I') => {
            let l = st(vm).http_input.clone();
            Ok(names_array(&l))
        }
        Some(b'L') => {
            let l = st(vm).http_input.clone();
            if l.is_empty() {
                return Ok(Value::Bool(false));
            }
            let s: Vec<&str> = l.iter().map(|e| e.name()).collect();
            Ok(Value::from(s.join(",").as_str()))
        }
        _ => vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($type) must be one of \"G\", \"P\", \"C\", \"S\", \"I\", or \"L\"",
        ),
    }
}

pub(crate) fn mb_http_output(vm: &mut Vm, a: &mut Args) -> R<Value> {
    match a.opt_str(0) {
        None => Ok(match st(vm).http_output {
            Output::Pass => Value::from("pass"),
            Output::Enc(e) => Value::from(e.name()),
        }),
        Some(n) => {
            let o = if n.eq_ignore_ascii_case(b"pass") {
                Output::Pass
            } else {
                match Encoding::from_name(&n) {
                    Some(e) => Output::Enc(e),
                    None => {
                        return vm.fthrow(
                            a,
                            "ValueError",
                            &format!(
                                "Argument #1 ($encoding) must be a valid encoding, \"{}\" given",
                                n.to_string_lossy()
                            ),
                        );
                    }
                }
            };
            let s = st(vm);
            s.http_output = o;
            s.http_output_func = true;
            Ok(Value::Bool(true))
        }
    }
}

pub(crate) fn mb_detect_order(vm: &mut Vm, a: &mut Args) -> R<Value> {
    match a.get(0).map(|v| v.deref()) {
        None | Some(Value::Null) => {
            let l = st(vm).detect.clone();
            Ok(names_array(&l))
        }
        Some(v) => {
            let l = enc_list(vm, a, &v, 0, "encoding")?;
            st(vm).detect = l;
            Ok(Value::Bool(true))
        }
    }
}

pub(crate) fn mb_substitute_character(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let v = a.val(0);
    let bad = |vm: &mut Vm, a: &Args| -> R<Value> {
        vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($substitute_character) must be \"none\", \"long\", \"entity\" or a valid codepoint",
        )
    };
    let cur = subst(vm);
    let set = |vm: &mut Vm, m: SubstMode, ch: u32| {
        st(vm).sub = goro_charset::Subst { mode: m, ch };
        Ok(Value::Bool(true))
    };
    match v {
        Value::Null => Ok(match cur.mode {
            SubstMode::None => Value::from("none"),
            SubstMode::Long => Value::from("long"),
            SubstMode::Entity => Value::from("entity"),
            SubstMode::Char => Value::Int(cur.ch as i64),
        }),
        Value::Str(s) => match s.to_ascii_lowercase().as_slice() {
            b"none" => set(vm, SubstMode::None, cur.ch),
            b"long" => set(vm, SubstMode::Long, cur.ch),
            b"entity" => set(vm, SubstMode::Entity, cur.ch),
            _ => bad(vm, a),
        },
        Value::Int(i) => {
            if !(0..=0x10FFFF).contains(&i) || (0xD800..0xE000).contains(&i) {
                return vm.fthrow(
                    a,
                    "ValueError",
                    "Argument #1 ($substitute_character) is not a valid codepoint",
                );
            }
            set(vm, SubstMode::Char, i as u32)
        }
        _ => bad(vm, a),
    }
}

pub(crate) fn mb_preferred_mime_name(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = a.str(0);
    let Some(e) = Encoding::from_name(&n) else {
        return vm.fthrow(
            a,
            "ValueError",
            &format!(
                "Argument #1 ($encoding) must be a valid encoding, \"{}\" given",
                n.to_string_lossy()
            ),
        );
    };
    match e.mime_name() {
        Some(m) => Ok(Value::from(m)),
        None => {
            vm.fwarn(
                a,
                &format!(
                    "No MIME preferred name corresponding to \"{}\"",
                    n.to_string_lossy()
                ),
            )?;
            Ok(Value::Bool(false))
        }
    }
}

pub(crate) fn mb_list_encodings(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(names_array(&goro_charset::encoding::ALL))
}

pub(crate) fn mb_encoding_aliases(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = a.str(0);
    let Some(e) = Encoding::from_name(&n) else {
        return vm.fthrow(
            a,
            "ValueError",
            &format!(
                "Argument #1 ($encoding) must be a valid encoding, \"{}\" given",
                n.to_string_lossy()
            ),
        );
    };
    Ok(Value::Array(PhpArray::from_values(
        e.aliases().iter().map(|s| Value::from(*s)),
    )))
}

fn sub_value(s: goro_charset::Subst) -> Value {
    match s.mode {
        SubstMode::None => Value::from("none"),
        SubstMode::Long => Value::from("long"),
        SubstMode::Entity => Value::from("entity"),
        SubstMode::Char => Value::Int(s.ch as i64),
    }
}

fn info_value(vm: &mut Vm, key: &str) -> Option<Value> {
    if key == "http_output_conv_mimetypes" {
        return Some(Value::Str(
            vm.get_ini(b"mbstring.http_output_conv_mimetypes")
                .unwrap_or_else(PhpStr::empty),
        ));
    }
    if key == "encoding_translation" {
        let on = ini_truthy(
            vm.get_ini(b"mbstring.encoding_translation")
                .unwrap_or_else(PhpStr::empty)
                .as_bytes(),
        );
        return Some(Value::from(if on { "On" } else { "Off" }));
    }
    let s = st(vm);
    let l = &LANGS[s.lang];
    Some(match key {
        "internal_encoding" => Value::from(s.internal.name()),
        "http_input" => Value::from(s.last_input?.name()),
        "http_output" => match s.http_output {
            Output::Pass => Value::from("pass"),
            Output::Enc(e) => Value::from(e.name()),
        },
        "mail_charset" => Value::from(l.mail_charset.name()),
        "mail_header_encoding" => Value::from(if l.header_b64 {
            "BASE64"
        } else {
            "Quoted-Printable"
        }),
        "mail_body_encoding" => Value::from(l.body.name()),
        "illegal_chars" => Value::Int(s.illegal as i64),
        "language" => Value::from(l.name),
        "detect_order" => names_array(&s.detect),
        "substitute_character" => sub_value(s.sub),
        "strict_detection" => Value::from(if s.strict { "On" } else { "Off" }),
        _ => return None,
    })
}

const INFO_KEYS: &[&str] = &[
    "internal_encoding",
    "http_input",
    "http_output",
    "http_output_conv_mimetypes",
    "mail_charset",
    "mail_header_encoding",
    "mail_body_encoding",
    "illegal_chars",
    "encoding_translation",
    "language",
    "detect_order",
    "substitute_character",
    "strict_detection",
];

pub(crate) fn mb_get_info(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let t = if a.has(0) {
        String::from_utf8_lossy(&a.str(0).to_ascii_lowercase()).into_owned()
    } else {
        "all".to_string()
    };
    if t == "all" {
        let mut out = PhpArray::new();
        for k in INFO_KEYS {
            if let Some(v) = info_value(vm, k) {
                out.insert(Key::from(*k), v);
            }
        }
        return Ok(Value::Array(out));
    }
    if INFO_KEYS.contains(&t.as_str()) {
        // http_input without input is null (test mb_get_info_http_input.phpt)
        return Ok(info_value(vm, &t).unwrap_or(Value::Null));
    }
    // a warning, in lower case (test mb_get_info_errors.phpt)
    vm.fwarn(a, "argument #1 ($type) must be a valid type")?;
    Ok(Value::Bool(false))
}

// ---------------------------------------------------------------- HTTP

pub(crate) fn mb_parse_str(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let seps = vm
        .get_ini(b"arg_separator.input")
        .unwrap_or_else(|| PhpStr::from("&"));
    let raw = a.bytes(0).to_vec();
    let to = internal(vm);
    // The source encoding: mbstring.http_input's list, detected when it
    // has several (test mb_parse_str_multi.phpt), else the internal one.
    let list = st(vm).http_input.clone();
    let from = match list.as_slice() {
        [] => to,
        [one] => *one,
        many => {
            let strict = st(vm).strict;
            let mut dec = Vec::new();
            for p in raw.split(|c| seps.contains(c)) {
                for q in p.splitn(2, |&c| c == b'=') {
                    dec.extend(goro_ext_standard::url::urldecode(q));
                    dec.push(b' ');
                }
            }
            ops::detect::detect_encoding(&dec, many, strict).unwrap_or(to)
        }
    };
    st(vm).last_input = Some(from);
    let arr = super::super::string::parse_query(&raw, &seps);
    fn conv(vm: &mut Vm, arr: &PhpArray, to: Encoding, from: Encoding) -> PhpArray {
        let mut out = PhpArray::new();
        for (key, v) in arr.iter() {
            let key = match key {
                KeyRef::Str(s) => {
                    Key::from_php_str(&PhpStr::from(convert_counted(vm, s, to, from)))
                }
                KeyRef::Int(i) => Key::Int(i),
            };
            let v = match v.deref() {
                Value::Str(s) => sv(convert_counted(vm, &s, to, from)),
                Value::Array(inner) => Value::Array(conv(vm, &inner, to, from)),
                other => other,
            };
            out.insert(key, v);
        }
        out
    }
    let arr = conv(vm, &arr, to, from);
    a.set_ref(1, Value::Array(arr));
    Ok(Value::Bool(true))
}

/// Converts output from the internal encoding to mbstring.http_output when
/// the content type (default_mimetype in the CLI) matches
/// mbstring.http_output_conv_mimetypes (PHP manual: mb_output_handler).
pub(crate) fn mb_output_handler(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.str(0);
    let Output::Enc(to) = st(vm).http_output else {
        return Ok(Value::Str(s));
    };
    let from = internal(vm);
    if to == from || matches!(to, Encoding::EightBit) {
        return Ok(Value::Str(s));
    }
    let mime = match vm
        .ext::<super::super::core_funcs::ResponseHeaders>()
        .content_type
        .clone()
    {
        Some(ct) => ct,
        None => vm
            .get_ini(b"default_mimetype")
            .unwrap_or_else(|| PhpStr::from("text/html")),
    };
    let pat = vm
        .get_ini(b"mbstring.http_output_conv_mimetypes")
        .unwrap_or_else(PhpStr::empty);
    if !mime_matches(&pat, &mime) {
        return Ok(Value::Str(s));
    }
    Ok(sv(convert_counted(vm, &s, to, from)))
}

/// Whether the content type matches mbstring.http_output_conv_mimetypes,
/// a regular expression (PHP manual: mbstring runtime configuration).
fn mime_matches(pat: &[u8], mime: &[u8]) -> bool {
    goro_regex::Regex::new(pat, goro_regex::Flags::CASELESS)
        .ok()
        .and_then(|r| r.is_match(mime).ok())
        .unwrap_or(false)
}

// ---------------------------------------------------------------- strings

pub(crate) fn mb_strlen(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let e = enc_arg(vm, a, 1)?;
    Ok(Value::Int(ops::strlen(a.bytes(0), e) as i64))
}

pub(crate) fn mb_str_split(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = a.int_or(1, 1);
    if n < 1 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($length) must be greater than 0",
        );
    }
    // observed on PHP 8.5: 2^30 and more is refused
    if n >= 1 << 30 {
        return vm.fthrow(a, "ValueError", "Argument #2 ($length) is too large");
    }
    let e = enc_arg(vm, a, 2)?;
    let sub = subst(vm);
    let parts = ops::str_split(a.bytes(0), n as usize, e, &sub);
    Ok(Value::Array(PhpArray::from_values(
        parts.into_iter().map(sv),
    )))
}

type PosFn = fn(&[u8], &[u8], i64, Encoding) -> Result<Option<usize>, MbError>;

fn pos_common(vm: &mut Vm, a: &mut Args, f: PosFn) -> R<Value> {
    let e = enc_arg(vm, a, 3)?;
    match f(a.bytes(0), a.bytes(1), a.int_or(2, 0), e) {
        Ok(Some(p)) => Ok(Value::Int(p as i64)),
        Ok(None) => Ok(Value::Bool(false)),
        Err(err) => mb_err(vm, a, err),
    }
}

pub(crate) fn mb_strpos(vm: &mut Vm, a: &mut Args) -> R<Value> {
    pos_common(vm, a, ops::strpos)
}
pub(crate) fn mb_strrpos(vm: &mut Vm, a: &mut Args) -> R<Value> {
    pos_common(vm, a, ops::strrpos)
}
pub(crate) fn mb_stripos(vm: &mut Vm, a: &mut Args) -> R<Value> {
    pos_common(vm, a, ops::stripos)
}
pub(crate) fn mb_strripos(vm: &mut Vm, a: &mut Args) -> R<Value> {
    pos_common(vm, a, ops::strripos)
}

type StrFn = fn(&[u8], &[u8], bool, Encoding, &goro_charset::Subst) -> Option<Vec<u8>>;

fn str_common(vm: &mut Vm, a: &mut Args, f: StrFn) -> R<Value> {
    let e = enc_arg(vm, a, 3)?;
    let sub = subst(vm);
    Ok(
        match f(a.bytes(0), a.bytes(1), a.bool_or(2, false), e, &sub) {
            Some(b) => sv(b),
            None => Value::Bool(false),
        },
    )
}

pub(crate) fn mb_strstr(vm: &mut Vm, a: &mut Args) -> R<Value> {
    str_common(vm, a, ops::strstr)
}
pub(crate) fn mb_strrchr(vm: &mut Vm, a: &mut Args) -> R<Value> {
    str_common(vm, a, ops::strrchr)
}
pub(crate) fn mb_stristr(vm: &mut Vm, a: &mut Args) -> R<Value> {
    str_common(vm, a, ops::stristr)
}
pub(crate) fn mb_strrichr(vm: &mut Vm, a: &mut Args) -> R<Value> {
    str_common(vm, a, ops::strrichr)
}

pub(crate) fn mb_substr_count(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let e = enc_arg(vm, a, 2)?;
    match ops::substr_count(a.bytes(0), a.bytes(1), e) {
        Ok(n) => Ok(Value::Int(n as i64)),
        Err(err) => mb_err(vm, a, err),
    }
}

pub(crate) fn mb_substr(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let e = enc_arg(vm, a, 3)?;
    // PHP_INT_MIN cannot be negated (test gh16360.phpt)
    let range = format!("must be between {} and {}", i64::MIN + 1, i64::MAX);
    if a.int(1) == i64::MIN {
        return vm.fthrow(a, "ValueError", &format!("Argument #2 ($start) {range}"));
    }
    if a.opt_int(2) == Some(i64::MIN) {
        return vm.fthrow(a, "ValueError", &format!("Argument #3 ($length) {range}"));
    }
    let sub = subst(vm);
    Ok(sv(ops::substr(a.bytes(0), a.int(1), a.opt_int(2), e, &sub)))
}

pub(crate) fn mb_strcut(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let e = enc_arg(vm, a, 3)?;
    Ok(sv(ops::strcut(a.bytes(0), a.int(1), a.opt_int(2), e)))
}

pub(crate) fn mb_strwidth(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let e = enc_arg(vm, a, 1)?;
    Ok(Value::Int(ops::strwidth(a.bytes(0), e) as i64))
}

pub(crate) fn mb_strimwidth(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let e = enc_arg(vm, a, 4)?;
    let width = a.int(2);
    let sub = subst(vm);
    let r = ops::strimwidth(a.bytes(0), a.int(1), width, a.bytes(3), e, &sub);
    match r {
        Ok(b) => {
            if width < 0 {
                vm.fdeprecated(
                    a,
                    "passing a negative integer to argument #3 ($width) is deprecated",
                )?;
            }
            Ok(sv(b))
        }
        Err(err) => mb_err(vm, a, err),
    }
}

pub(crate) fn mb_str_pad(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let e = enc_arg(vm, a, 4)?;
    let sub = subst(vm);
    let pad = if a.has(2) {
        a.str(2)
    } else {
        PhpStr::from(" ")
    };
    match ops::str_pad(a.bytes(0), a.int(1), &pad, a.int_or(3, 1), e, &sub) {
        Ok(b) => Ok(sv(b)),
        Err(err) => mb_err(vm, a, err),
    }
}

fn trim_common(vm: &mut Vm, a: &mut Args, m: TrimMode) -> R<Value> {
    let e = enc_arg(vm, a, 2)?;
    let sub = subst(vm);
    let chars = a.opt_str(1);
    Ok(sv(ops::trim(
        a.bytes(0),
        chars.as_ref().map(|c| c.as_bytes()),
        m,
        e,
        &sub,
    )))
}

pub(crate) fn mb_trim(vm: &mut Vm, a: &mut Args) -> R<Value> {
    trim_common(vm, a, TrimMode::Both)
}
pub(crate) fn mb_ltrim(vm: &mut Vm, a: &mut Args) -> R<Value> {
    trim_common(vm, a, TrimMode::Left)
}
pub(crate) fn mb_rtrim(vm: &mut Vm, a: &mut Args) -> R<Value> {
    trim_common(vm, a, TrimMode::Right)
}

pub(crate) fn mb_convert_case(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(mode) = CaseMode::from_php(a.int(1)) else {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($mode) must be one of the MB_CASE_* constants",
        );
    };
    let e = enc_arg(vm, a, 2)?;
    let sub = subst(vm);
    Ok(sv(ops::convert_case(a.bytes(0), mode, e, &sub)))
}

pub(crate) fn mb_strtoupper(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let e = enc_arg(vm, a, 1)?;
    let sub = subst(vm);
    Ok(sv(ops::strtoupper(a.bytes(0), e, &sub)))
}

pub(crate) fn mb_strtolower(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let e = enc_arg(vm, a, 1)?;
    let sub = subst(vm);
    Ok(sv(ops::strtolower(a.bytes(0), e, &sub)))
}

pub(crate) fn mb_ucfirst(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let e = enc_arg(vm, a, 1)?;
    let sub = subst(vm);
    Ok(sv(ops::ucfirst(a.bytes(0), e, &sub)))
}

pub(crate) fn mb_lcfirst(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let e = enc_arg(vm, a, 1)?;
    let sub = subst(vm);
    Ok(sv(ops::lcfirst(a.bytes(0), e, &sub)))
}

pub(crate) fn mb_ord(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let e = enc_arg(vm, a, 1)?;
    if a.bytes(0).is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #1 ($string) must not be empty");
    }
    match ops::ord(a.bytes(0), e) {
        Ok(Some(c)) => Ok(Value::Int(c as i64)),
        Ok(None) => Ok(Value::Bool(false)),
        Err(err) => mb_err(vm, a, err),
    }
}

pub(crate) fn mb_chr(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let e = enc_arg(vm, a, 1)?;
    let cp = a.int(0);
    if !(0..=0x10FFFF).contains(&cp) {
        // Unsupported encodings are still reported.
        if let Err(err) = ops::chr(0x41, e) {
            return mb_err(vm, a, err);
        }
        return Ok(Value::Bool(false));
    }
    match ops::chr(cp as u32, e) {
        Ok(Some(b)) => Ok(sv(b)),
        Ok(None) => Ok(Value::Bool(false)),
        Err(err) => mb_err(vm, a, err),
    }
}

pub(crate) fn mb_scrub(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let e = enc_arg(vm, a, 1)?;
    let s = a.bytes(0).to_vec();
    Ok(sv(convert_counted(vm, &s, e, e)))
}

pub(crate) fn mb_convert_kana(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let mode = if a.has(1) {
        a.str(1)
    } else {
        PhpStr::from("KV")
    };
    let m = match ops::kana::KanaMode::parse(&mode) {
        Ok(m) => m,
        Err(err) => return mb_err(vm, a, err),
    };
    let e = enc_arg(vm, a, 2)?;
    let sub = subst(vm);
    Ok(sv(ops::kana::convert_kana(a.bytes(0), m, e, &sub)))
}

fn entity_map(vm: &mut Vm, a: &Args) -> R<Vec<ops::entity::MapEntry>> {
    let arr = a.array(1);
    if !arr.len().is_multiple_of(4) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($map) must have a multiple of 4 elements",
        );
    }
    let mut nums = Vec::with_capacity(arr.len());
    for (_, v) in arr.iter() {
        match v.deref() {
            Value::Int(i) => nums.push(i),
            _ => {
                return vm.fthrow(
                    a,
                    "ValueError",
                    "Argument #2 ($map) must only be composed of values of type int",
                );
            }
        }
    }
    Ok(nums
        .chunks(4)
        .map(|c| ops::entity::MapEntry::from_php(c[0], c[1], c[2], c[3]))
        .collect())
}

pub(crate) fn mb_encode_numericentity(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let map = entity_map(vm, a)?;
    let e = enc_arg(vm, a, 2)?;
    let sub = subst(vm);
    Ok(sv(ops::entity::encode_numericentity(
        a.bytes(0),
        &map,
        e,
        a.bool_or(3, false),
        &sub,
    )))
}

pub(crate) fn mb_decode_numericentity(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let map = entity_map(vm, a)?;
    let e = enc_arg(vm, a, 2)?;
    let sub = subst(vm);
    Ok(sv(ops::entity::decode_numericentity(
        a.bytes(0),
        &map,
        e,
        &sub,
    )))
}

pub(crate) fn mb_encode_mimeheader(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let lang = st(vm).lang;
    let charset = match a.opt_str(1) {
        None => LANGS[lang].mail_charset,
        Some(n) => named_enc(vm, a, 1, "charset", &n)?,
    };
    let transfer = match a.opt_str(2) {
        None => {
            if LANGS[lang].header_b64 {
                ops::mime::Transfer::Base64
            } else {
                ops::mime::Transfer::QuotedPrintable
            }
        }
        Some(t) => ops::mime::Transfer::from_php(&t),
    };
    let nl = if a.has(3) {
        a.str(3)
    } else {
        PhpStr::from("\r\n")
    };
    let indent = a.int_or(4, 0).max(0) as usize;
    let internal = internal(vm);
    match ops::mime::encode_mimeheader(a.bytes(0), charset, transfer, &nl, indent, internal) {
        Ok(b) => Ok(sv(b)),
        Err(err) => mb_err(vm, a, err),
    }
}

pub(crate) fn mb_decode_mimeheader(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let internal = internal(vm);
    let sub = subst(vm);
    Ok(sv(ops::mime::decode_mimeheader(a.bytes(0), internal, &sub)))
}

// ---------------------------------------------------------------- conversion

/// The source among `from`, detected when there are several.
fn pick_source(vm: &mut Vm, s: &[u8], from: &[Encoding]) -> Option<Encoding> {
    match from {
        [one] => Some(*one),
        many => {
            let strict = st(vm).strict;
            ops::detect::detect_encoding(s, many, strict)
        }
    }
}

#[derive(Default)]
struct Visit {
    refs: Vec<PhpRef>,
    objs: Vec<ObjRef>,
    /// A recursion was found: the whole conversion fails.
    failed: bool,
}

fn convert_value(
    vm: &mut Vm,
    a: &Args,
    v: &Value,
    to: Encoding,
    from: &[Encoding],
    seen: &mut Visit,
) -> R<Option<Value>> {
    match v {
        Value::Str(s) => {
            let Some(src) = pick_source(vm, s, from) else {
                vm.fwarn(a, "Unable to detect character encoding")?;
                return Ok(None);
            };
            Ok(Some(sv(convert_counted(vm, s, to, src))))
        }
        Value::Array(arr) => {
            let mut out = PhpArray::new();
            for (k, x) in arr.iter() {
                let key = match k {
                    KeyRef::Str(s) => {
                        let Some(src) = pick_source(vm, s, from) else {
                            // the entry is left out (test gh10627.phpt)
                            vm.fwarn(a, "Unable to detect character encoding")?;
                            continue;
                        };
                        Key::from_php_str(&PhpStr::from(convert_counted(vm, s, to, src)))
                    }
                    KeyRef::Int(i) => Key::Int(i),
                };
                let item = match x {
                    Value::Ref(r) => {
                        if seen.refs.iter().any(|p| PhpRef::ptr_eq(p, r)) {
                            vm.fwarn(a, "Cannot convert recursively referenced values")?;
                            seen.failed = true;
                            return Ok(None);
                        }
                        seen.refs.push(r.clone());
                        let inner = r.get();
                        let res = convert_value(vm, a, &inner, to, from, seen)?;
                        seen.refs.pop();
                        res
                    }
                    Value::Object(_) => {
                        vm.fwarn(a, "Object is not supported")?;
                        continue;
                    }
                    _ => convert_value(vm, a, x, to, from, seen)?,
                };
                let Some(item) = item else {
                    if seen.failed {
                        return Ok(None);
                    }
                    continue;
                };
                out.insert(key, item);
            }
            Ok(Some(Value::Array(out)))
        }
        other => Ok(Some(other.clone())),
    }
}

pub(crate) fn mb_convert_encoding(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let to_name = a.str(1);
    let to = named_enc(vm, a, 1, "to_encoding", &to_name)?;
    let from = match a.get(2).map(|v| v.deref()) {
        None | Some(Value::Null) => vec![internal(vm)],
        Some(v) => enc_list(vm, a, &v, 2, "from_encoding")?,
    };
    let v = a.val(0);
    let mut seen = Visit::default();
    Ok(convert_value(vm, a, &v, to, &from, &mut seen)?.unwrap_or(Value::Bool(false)))
}

pub(crate) fn mb_detect_encoding(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let list = match a.get(1).map(|v| v.deref()) {
        None | Some(Value::Null) => st(vm).detect.clone(),
        Some(v) => enc_list(vm, a, &v, 1, "encodings")?,
    };
    // mbstring.strict_detection makes it strict too (observed on PHP 8.5).
    let strict = a.bool_or(2, false) || st(vm).strict;
    Ok(
        match ops::detect::detect_encoding(a.bytes(0), &list, strict) {
            Some(e) => Value::from(e.name()),
            None => Value::Bool(false),
        },
    )
}

fn check_value(vm: &mut Vm, a: &Args, v: &Value, e: Encoding, seen: &mut Visit) -> R<bool> {
    Ok(match v {
        Value::Str(s) => goro_charset::check_encoding(s, e),
        Value::Array(arr) => {
            for (k, x) in arr.iter() {
                if let KeyRef::Str(s) = k
                    && !goro_charset::check_encoding(s, e)
                {
                    return Ok(false);
                }
                let ok = if let Value::Ref(r) = x {
                    if seen.refs.iter().any(|p| PhpRef::ptr_eq(p, r)) {
                        vm.fwarn(a, "Cannot not handle circular references")?;
                        return Ok(false);
                    }
                    seen.refs.push(r.clone());
                    let inner = r.get();
                    let ok = check_value(vm, a, &inner, e, seen)?;
                    seen.refs.pop();
                    ok
                } else {
                    check_value(vm, a, x, e, seen)?
                };
                if !ok {
                    return Ok(false);
                }
            }
            true
        }
        _ => true,
    })
}

pub(crate) fn mb_check_encoding(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let e = enc_arg(vm, a, 1)?;
    let v = a.val(0);
    if v.is_null() {
        vm.fdeprecated(
            a,
            "Calling mb_check_encoding() without argument is deprecated",
        )?;
        return Ok(Value::Bool(st(vm).illegal == 0));
    }
    let mut seen = Visit::default();
    Ok(Value::Bool(check_value(vm, a, &v, e, &mut seen)?))
}

/// Collects the strings of a variable for detection; false on recursion.
fn collect_strings(v: &Value, out: &mut Vec<u8>, seen: &mut Visit) -> bool {
    match v {
        Value::Str(s) => {
            out.extend_from_slice(s);
            true
        }
        Value::Ref(r) => {
            if seen.refs.iter().any(|p| PhpRef::ptr_eq(p, r)) {
                return false;
            }
            seen.refs.push(r.clone());
            let inner = r.get();
            let ok = collect_strings(&inner, out, seen);
            seen.refs.pop();
            ok
        }
        Value::Array(arr) => arr.iter().all(|(_, x)| collect_strings(x, out, seen)),
        Value::Object(o) => {
            if seen.objs.iter().any(|p| ObjRef::ptr_eq(p, o)) {
                return false;
            }
            seen.objs.push(o.clone());
            let slots = o.obj().slots.borrow().clone();
            let props = o.obj().props.borrow().clone();
            let ok = slots.iter().all(|x| collect_strings(x, out, seen))
                && props.iter().all(|(_, x)| collect_strings(x, out, seen));
            seen.objs.pop();
            ok
        }
        _ => true,
    }
}

fn convert_in_place(vm: &mut Vm, v: &Value, to: Encoding, from: Encoding) -> Value {
    let conv_arr = |vm: &mut Vm, arr: &PhpArray| {
        let mut out = PhpArray::new();
        for (k, x) in arr.iter() {
            let key = match k {
                KeyRef::Str(s) => Key::from_php_str(s),
                KeyRef::Int(i) => Key::Int(i),
            };
            out.insert(key, convert_in_place(vm, x, to, from));
        }
        out
    };
    match v {
        Value::Str(s) => sv(convert_counted(vm, s, to, from)),
        // References inside arrays are separated: the copy gets the
        // converted value, the referenced variable keeps its own (test
        // bug26639.phpt).
        Value::Ref(r) => {
            let inner = r.get();
            convert_in_place(vm, &inner, to, from)
        }
        Value::Array(arr) => Value::Array(conv_arr(vm, arr)),
        // A property holding a reference to an array keeps the reference;
        // other referenced properties are separated (test gh16261.phpt).
        Value::Object(o) => {
            let prop = |vm: &mut Vm, x: &Value| -> Value {
                match x {
                    _ if vm.is_uninit(x) => x.clone(),
                    Value::Ref(r) if matches!(&*r.borrow(), Value::Array(_)) => {
                        let inner = r.get();
                        drop(r.set(convert_in_place(vm, &inner, to, from)));
                        x.clone()
                    }
                    _ => convert_in_place(vm, x, to, from),
                }
            };
            let n = o.obj().slots.borrow().len();
            for i in 0..n {
                let x = o.obj().slots.borrow()[i].clone();
                let nv = prop(vm, &x);
                let old = std::mem::replace(&mut o.obj().slots.borrow_mut()[i], nv);
                drop(old);
            }
            let props = o.obj().props.borrow().clone();
            let mut out = PhpArray::new();
            for (k, x) in props.iter() {
                let key = match k {
                    KeyRef::Str(s) => Key::from_php_str(s),
                    KeyRef::Int(i) => Key::Int(i),
                };
                out.insert(key, prop(vm, x));
            }
            let old = std::mem::replace(&mut *o.obj().props.borrow_mut(), out);
            drop(old);
            v.clone()
        }
        other => other.clone(),
    }
}

pub(crate) fn mb_convert_variables(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let to_name = a.str(0);
    let to = named_enc(vm, a, 0, "to_encoding", &to_name)?;
    let fv = a.val(1);
    let from = enc_list(vm, a, &fv, 1, "from_encoding")?;
    let n = a.len();
    let mut all = Vec::new();
    let mut seen = Visit::default();
    for i in 2..n {
        let v = a.val(i);
        // the variable itself counts ($a[] = &$a; test gh20836.phpt)
        seen.refs = a.ref_at(i).into_iter().collect();
        if !collect_strings(&v, &mut all, &mut seen) {
            vm.fwarn(a, "Cannot handle recursive references")?;
            return Ok(Value::Bool(false));
        }
    }
    let src = if from.len() == 1 {
        from[0]
    } else {
        let strict = st(vm).strict;
        match ops::detect::detect_encoding(&all, &from, strict) {
            Some(e) => e,
            None => {
                vm.fwarn(a, "Unable to detect encoding")?;
                return Ok(Value::Bool(false));
            }
        }
    };
    for i in 2..n {
        let v = a.val(i);
        let nv = convert_in_place(vm, &v, to, src);
        if !matches!(v, Value::Object(_)) {
            a.set_ref(i, nv);
        }
    }
    Ok(Value::from(src.name()))
}
