//! mbstring module globals (C: `MBSTRG(...)`), INI handling and languages.

use std::cell::RefCell;

use goro_core::value::Value;
use goro_core::vm::Vm;

use crate::mbfl::{self, EncNo, Encoding};
use crate::mbfl::{MBFL_OUTPUTFILTER_ILLEGAL_MODE_CHAR, MBFL_OUTPUTFILTER_ILLEGAL_MODE_ENTITY, MBFL_OUTPUTFILTER_ILLEGAL_MODE_LONG, MBFL_OUTPUTFILTER_ILLEGAL_MODE_NONE};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LangNo {
    Uni,
    Japanese,
    Korean,
    SimplifiedChinese,
    TraditionalChinese,
    English,
    German,
    Russian,
    Ukrainian,
    Armenian,
    Turkish,
    Neutral,
}

pub struct Language {
    pub no: LangNo,
    pub name: &'static str,
    pub short_name: &'static str,
    pub aliases: &'static [&'static str],
    pub mail_charset: EncNo,
    pub mail_header_encoding: EncNo,
    pub mail_body_encoding: EncNo,
}

pub static LANGUAGES: &[Language] = &[
    Language { no: LangNo::Uni, name: "uni", short_name: "uni", aliases: &["universal"], mail_charset: EncNo::Utf8, mail_header_encoding: EncNo::Base64, mail_body_encoding: EncNo::Base64 },
    Language { no: LangNo::Japanese, name: "Japanese", short_name: "ja", aliases: &[], mail_charset: EncNo::Iso2022jp, mail_header_encoding: EncNo::Base64, mail_body_encoding: EncNo::SevenBit },
    Language { no: LangNo::Korean, name: "Korean", short_name: "ko", aliases: &[], mail_charset: EncNo::Iso2022kr, mail_header_encoding: EncNo::Base64, mail_body_encoding: EncNo::SevenBit },
    Language { no: LangNo::SimplifiedChinese, name: "Simplified Chinese", short_name: "zh-cn", aliases: &[], mail_charset: EncNo::Hz, mail_header_encoding: EncNo::Base64, mail_body_encoding: EncNo::SevenBit },
    Language { no: LangNo::TraditionalChinese, name: "Traditional Chinese", short_name: "zh-tw", aliases: &[], mail_charset: EncNo::Big5, mail_header_encoding: EncNo::Base64, mail_body_encoding: EncNo::EightBit },
    Language { no: LangNo::English, name: "English", short_name: "en", aliases: &[], mail_charset: EncNo::Iso8859_1, mail_header_encoding: EncNo::Qprint, mail_body_encoding: EncNo::EightBit },
    Language { no: LangNo::German, name: "German", short_name: "de", aliases: &["Deutsch"], mail_charset: EncNo::Iso8859_15, mail_header_encoding: EncNo::Qprint, mail_body_encoding: EncNo::EightBit },
    Language { no: LangNo::Russian, name: "Russian", short_name: "ru", aliases: &[], mail_charset: EncNo::Koi8r, mail_header_encoding: EncNo::Qprint, mail_body_encoding: EncNo::EightBit },
    Language { no: LangNo::Ukrainian, name: "Ukrainian", short_name: "ua", aliases: &[], mail_charset: EncNo::Koi8u, mail_header_encoding: EncNo::Qprint, mail_body_encoding: EncNo::EightBit },
    Language { no: LangNo::Armenian, name: "Armenian", short_name: "hy", aliases: &[], mail_charset: EncNo::Armscii8, mail_header_encoding: EncNo::Qprint, mail_body_encoding: EncNo::EightBit },
    Language { no: LangNo::Turkish, name: "Turkish", short_name: "tr", aliases: &[], mail_charset: EncNo::Iso8859_9, mail_header_encoding: EncNo::Qprint, mail_body_encoding: EncNo::EightBit },
    Language { no: LangNo::Neutral, name: "neutral", short_name: "neutral", aliases: &[], mail_charset: EncNo::Utf8, mail_header_encoding: EncNo::Base64, mail_body_encoding: EncNo::Base64 },
];

pub fn name2language(name: &[u8]) -> Option<&'static Language> {
    LANGUAGES
        .iter()
        .find(|l| l.name.as_bytes().eq_ignore_ascii_case(name))
        .or_else(|| LANGUAGES.iter().find(|l| l.short_name.as_bytes().eq_ignore_ascii_case(name)))
        .or_else(|| LANGUAGES.iter().find(|l| l.aliases.iter().any(|a| a.as_bytes().eq_ignore_ascii_case(name))))
}

pub fn no2language(no: LangNo) -> &'static Language {
    LANGUAGES.iter().find(|l| l.no == no).unwrap()
}

fn default_detect_order(lang: LangNo) -> &'static [EncNo] {
    match lang {
        LangNo::Japanese => &[EncNo::Ascii, EncNo::Jis, EncNo::Utf8, EncNo::EucJp, EncNo::Sjis],
        LangNo::Korean => &[EncNo::Ascii, EncNo::Utf8, EncNo::EucKr, EncNo::Uhc],
        LangNo::TraditionalChinese => &[EncNo::Ascii, EncNo::Utf8, EncNo::EucTw, EncNo::Big5],
        LangNo::SimplifiedChinese => &[EncNo::Ascii, EncNo::Utf8, EncNo::EucCn, EncNo::Cp936],
        LangNo::Russian => &[EncNo::Ascii, EncNo::Utf8, EncNo::Koi8r, EncNo::Cp1251, EncNo::Cp866],
        LangNo::Armenian => &[EncNo::Ascii, EncNo::Utf8, EncNo::Armscii8],
        LangNo::Turkish => &[EncNo::Ascii, EncNo::Utf8, EncNo::Cp1254, EncNo::Iso8859_9],
        LangNo::Ukrainian => &[EncNo::Ascii, EncNo::Utf8, EncNo::Koi8u],
        _ => &[EncNo::Ascii, EncNo::Utf8],
    }
}

pub struct MbState {
    pub initialized: bool,
    pub language: LangNo,
    pub internal_encoding: &'static Encoding,
    pub current_internal_encoding: &'static Encoding,
    pub http_output_encoding: &'static Encoding,
    pub current_http_output_encoding: &'static Encoding,
    pub http_input_identify: Option<&'static Encoding>,
    pub http_input_identify_get: Option<&'static Encoding>,
    pub http_input_identify_post: Option<&'static Encoding>,
    pub http_input_identify_cookie: Option<&'static Encoding>,
    pub http_input_identify_string: Option<&'static Encoding>,
    pub http_input_list: Vec<&'static Encoding>,
    pub detect_order_list: Vec<&'static Encoding>,
    pub current_detect_order_list: Vec<&'static Encoding>,
    pub default_detect_order_list: &'static [EncNo],
    pub filter_illegal_mode: u8,
    pub filter_illegal_substchar: u32,
    pub current_filter_illegal_mode: u8,
    pub current_filter_illegal_substchar: u32,
    pub illegalchars: usize,
    pub encoding_translation: bool,
    pub strict_detection: bool,
    pub outconv_enabled: bool,
    pub outconv_state: u32,
    pub internal_encoding_set: bool,
    pub http_output_set: bool,
    pub http_input_set: bool,
    /// Last seen raw values of the INI settings we track
    pub ini_seen: Vec<(&'static str, Option<Vec<u8>>)>,
}

impl MbState {
    fn new() -> MbState {
        let utf8 = &mbfl::basic::ENC_UTF8;
        MbState {
            initialized: false,
            language: LangNo::Uni,
            internal_encoding: utf8,
            current_internal_encoding: utf8,
            http_output_encoding: &mbfl::basic::ENC_PASS,
            current_http_output_encoding: &mbfl::basic::ENC_PASS,
            http_input_identify: None,
            http_input_identify_get: None,
            http_input_identify_post: None,
            http_input_identify_cookie: None,
            http_input_identify_string: None,
            http_input_list: Vec::new(),
            detect_order_list: Vec::new(),
            current_detect_order_list: Vec::new(),
            default_detect_order_list: default_detect_order(LangNo::Neutral),
            filter_illegal_mode: MBFL_OUTPUTFILTER_ILLEGAL_MODE_CHAR,
            filter_illegal_substchar: '?' as u32,
            current_filter_illegal_mode: MBFL_OUTPUTFILTER_ILLEGAL_MODE_CHAR,
            current_filter_illegal_substchar: '?' as u32,
            illegalchars: 0,
            encoding_translation: false,
            strict_detection: false,
            outconv_enabled: false,
            outconv_state: 0,
            internal_encoding_set: false,
            http_output_set: false,
            http_input_set: false,
            ini_seen: Vec::new(),
        }
    }
}

thread_local! {
    pub static MBSTRG: RefCell<MbState> = RefCell::new(MbState::new());
}

/// Run `f` with mutable access to the globals.
pub fn with<R>(f: impl FnOnce(&mut MbState) -> R) -> R {
    MBSTRG.with(|s| f(&mut s.borrow_mut()))
}

const TRACKED_INI: &[&str] = &[
    "mbstring.language",
    "mbstring.detect_order",
    "mbstring.http_input",
    "mbstring.http_output",
    "mbstring.internal_encoding",
    "mbstring.substitute_character",
    "mbstring.strict_detection",
    "mbstring.encoding_translation",
    "internal_encoding",
    "input_encoding",
    "output_encoding",
    "default_charset",
];

fn ini_raw(vm: &Vm, key: &str) -> Option<Vec<u8>> {
    vm.constants.get(key.as_bytes()).map(|v| match v {
        Value::String(s) => s.as_bytes().to_vec(),
        other => other.to_php_string().as_bytes().to_vec(),
    })
}

fn ini_str(vm: &Vm, key: &str) -> Vec<u8> {
    ini_raw(vm, key).unwrap_or_default()
}

fn get_internal_encoding_name(vm: &Vm) -> Vec<u8> {
    let ie = ini_str(vm, "internal_encoding");
    if !ie.is_empty() {
        return ie;
    }
    let dc = ini_str(vm, "default_charset");
    if !dc.is_empty() {
        return dc;
    }
    b"UTF-8".to_vec()
}

fn get_input_encoding_name(vm: &Vm) -> Vec<u8> {
    let ie = ini_str(vm, "input_encoding");
    if !ie.is_empty() {
        return ie;
    }
    let dc = ini_str(vm, "default_charset");
    if !dc.is_empty() {
        return dc;
    }
    b"UTF-8".to_vec()
}

fn get_output_encoding_name(vm: &Vm) -> Vec<u8> {
    let ie = ini_str(vm, "output_encoding");
    if !ie.is_empty() {
        return ie;
    }
    let dc = ini_str(vm, "default_charset");
    if !dc.is_empty() {
        return dc;
    }
    b"UTF-8".to_vec()
}

fn is_truthy_ini(v: &[u8]) -> bool {
    let s = String::from_utf8_lossy(v).to_ascii_lowercase();
    matches!(s.trim(), "1" | "on" | "yes" | "true") || s.trim().parse::<i64>().map(|n| n != 0).unwrap_or(false)
}

/// C: `php_mb_get_encoding_or_pass`
pub fn get_encoding_or_pass(name: &[u8]) -> Option<&'static Encoding> {
    // strncmp(encoding_name, "pass", encoding_name_len) == 0
    if name.len() <= 4 && b"pass".starts_with(name) {
        return Some(&mbfl::basic::ENC_PASS);
    }
    mbfl::name2encoding(name)
}

/// php_mb_parse_encoding_list in INI context (arg_num == 0): warnings
/// instead of exceptions. Returns None on failure.
pub fn parse_encoding_list_ini(vm: &mut Vm, st: &MbState, value: &[u8]) -> Option<Vec<&'static Encoding>> {
    match parse_encoding_list(st, value) {
        Ok(l) => Some(l),
        Err(bad) => {
            vm.emit_warning(&format!("INI setting contains invalid encoding \"{}\"", String::from_utf8_lossy(&bad)));
            None
        }
    }
}

/// C: `php_mb_parse_encoding_list`; on failure returns the offending name.
pub fn parse_encoding_list(st: &MbState, value: &[u8]) -> Result<Vec<&'static Encoding>, Vec<u8>> {
    let mut list = Vec::new();
    if value.is_empty() {
        return Ok(list);
    }
    let mut v = value;
    if v.len() > 2 && v[0] == b'"' && v[v.len() - 1] == b'"' {
        v = &v[1..v.len() - 1];
    }
    let mut included_auto = false;
    for part in v.split(|&c| c == b',') {
        let mut s = part;
        while let Some((&c, rest)) = s.split_first() {
            if c == b' ' || c == b'\t' {
                s = rest;
            } else {
                break;
            }
        }
        while let Some((&c, rest)) = s.split_last() {
            if (c == b' ' || c == b'\t') && rest.len() > 0 {
                s = rest;
            } else {
                break;
            }
        }
        // strncasecmp(p1, "auto", p1_length) == 0
        if s.len() <= 4 && b"auto"[..s.len()].eq_ignore_ascii_case(s) {
            if !included_auto {
                included_auto = true;
                for no in st.default_detect_order_list {
                    if let Some(e) = mbfl::no2encoding(*no) {
                        list.push(e);
                    }
                }
            }
        } else {
            match mbfl::name2encoding(s) {
                Some(e) => list.push(e),
                None => return Err(s.to_vec()),
            }
        }
    }
    Ok(list)
}

fn set_internal_encoding_ini(vm: &mut Vm, st: &mut MbState, name: &[u8]) {
    let enc = if name.is_empty() { None } else { mbfl::name2encoding(name) };
    let enc = match enc {
        Some(e) => e,
        None => {
            vm.emit_warning(&format!("Unknown encoding \"{}\" in ini setting", String::from_utf8_lossy(name)));
            &mbfl::basic::ENC_UTF8
        }
    };
    st.internal_encoding = enc;
    st.current_internal_encoding = enc;
}

fn set_http_output_ini(st: &mut MbState, name: &[u8]) -> bool {
    match get_encoding_or_pass(name) {
        Some(e) => {
            st.http_output_encoding = e;
            st.current_http_output_encoding = e;
            true
        }
        None => false,
    }
}

fn set_http_input_ini(vm: &mut Vm, st: &mut MbState, name: &[u8]) -> bool {
    if name == b"pass" {
        st.http_input_list = vec![&mbfl::basic::ENC_PASS];
        return true;
    }
    match parse_encoding_list_ini(vm, st, name) {
        Some(l) if !l.is_empty() => {
            st.http_input_list = l;
            true
        }
        _ => false,
    }
}

fn apply_ini(vm: &mut Vm, st: &mut MbState, key: &str, value: Option<&[u8]>, startup: bool) {
    match key {
        "mbstring.language" => {
            let v = value.unwrap_or(b"neutral");
            let v = if v.is_empty() { b"neutral".as_ref() } else { v };
            match name2language(v) {
                Some(l) => {
                    st.language = l.no;
                    st.default_detect_order_list = default_detect_order(l.no);
                }
                None => {
                    st.language = LangNo::Neutral;
                }
            }
        }
        "mbstring.detect_order" => match value {
            None => st.detect_order_list.clear(),
            Some(v) => {
                if let Some(l) = parse_encoding_list_ini(vm, st, v) {
                    if !l.is_empty() {
                        st.detect_order_list = l;
                    }
                }
            }
        },
        "mbstring.http_input" => {
            if value.is_some() {
                vm.emit_deprecated("Use of mbstring.http_input is deprecated");
            }
            match value {
                Some(v) if !v.is_empty() => {
                    st.http_input_set = true;
                    set_http_input_ini(vm, st, v);
                }
                _ => {
                    st.http_input_set = false;
                    let e = get_input_encoding_name(vm);
                    set_http_input_ini(vm, st, &e);
                }
            }
        }
        "mbstring.http_output" => {
            if value.is_some() {
                vm.emit_deprecated("Use of mbstring.http_output is deprecated");
            }
            match value {
                Some(v) if !v.is_empty() => {
                    st.http_output_set = true;
                    set_http_output_ini(st, v);
                }
                _ => {
                    st.http_output_set = false;
                    let e = get_output_encoding_name(vm);
                    set_http_output_ini(st, &e);
                }
            }
        }
        "mbstring.internal_encoding" => {
            if value.is_some() {
                vm.emit_deprecated("Use of mbstring.internal_encoding is deprecated");
            }
            match value {
                Some(v) if !v.is_empty() => {
                    st.internal_encoding_set = true;
                    set_internal_encoding_ini(vm, st, v);
                }
                _ => {
                    st.internal_encoding_set = false;
                    let e = get_internal_encoding_name(vm);
                    set_internal_encoding_ini(vm, st, &e);
                }
            }
        }
        "mbstring.substitute_character" => match value {
            Some(v) => {
                let lower = v.to_ascii_lowercase();
                let mode = match lower.as_slice() {
                    b"none" => MBFL_OUTPUTFILTER_ILLEGAL_MODE_NONE,
                    b"long" => MBFL_OUTPUTFILTER_ILLEGAL_MODE_LONG,
                    b"entity" => MBFL_OUTPUTFILTER_ILLEGAL_MODE_ENTITY,
                    _ => MBFL_OUTPUTFILTER_ILLEGAL_MODE_CHAR,
                };
                st.filter_illegal_mode = mode;
                st.current_filter_illegal_mode = mode;
                if mode == MBFL_OUTPUTFILTER_ILLEGAL_MODE_CHAR && !v.is_empty() {
                    if let Some(c) = strtol_full(v) {
                        st.filter_illegal_substchar = c as u32;
                        st.current_filter_illegal_substchar = c as u32;
                    }
                }
            }
            None => {
                st.filter_illegal_mode = MBFL_OUTPUTFILTER_ILLEGAL_MODE_CHAR;
                st.current_filter_illegal_mode = MBFL_OUTPUTFILTER_ILLEGAL_MODE_CHAR;
                st.filter_illegal_substchar = '?' as u32;
                st.current_filter_illegal_substchar = '?' as u32;
            }
        },
        "mbstring.strict_detection" => {
            st.strict_detection = value.map(is_truthy_ini).unwrap_or(false);
        }
        "mbstring.encoding_translation" => {
            st.encoding_translation = value.map(is_truthy_ini).unwrap_or(false);
        }
        "internal_encoding" | "input_encoding" | "output_encoding" | "default_charset" => {
            if startup {
                return;
            }
            // mbstring_internal_encoding_changed_hook
            if !st.internal_encoding_set {
                let e = get_internal_encoding_name(vm);
                set_internal_encoding_ini(vm, st, &e);
            }
            if !st.http_output_set {
                let e = get_output_encoding_name(vm);
                set_http_output_ini(st, &e);
            }
            if !st.http_input_set {
                let e = get_input_encoding_name(vm);
                set_http_input_ini(vm, st, &e);
            }
        }
        _ => {}
    }
}

/// strtol(s, &end, 0) where the whole string must be consumed
fn strtol_full(v: &[u8]) -> Option<i64> {
    let s = std::str::from_utf8(v).ok()?;
    let t = s.trim_start();
    let (neg, t) = if let Some(r) = t.strip_prefix('-') {
        (true, r)
    } else if let Some(r) = t.strip_prefix('+') {
        (false, r)
    } else {
        (false, t)
    };
    let n = if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        i64::from_str_radix(h, 16).ok()?
    } else if t.len() > 1 && t.starts_with('0') {
        i64::from_str_radix(&t[1..], 8).ok()?
    } else {
        t.parse::<i64>().ok()?
    };
    Some(if neg { -n } else { n })
}

/// Make sure the globals reflect the current INI settings. The first call
/// performs MINIT/RINIT; later calls pick up settings changed by ini_set().
pub fn sync(vm: &mut Vm) {
    let mut st = MBSTRG.with(|s| std::mem::replace(&mut *s.borrow_mut(), MbState::new()));
    if !st.initialized {
        st.initialized = true;
        // MINIT: register INI entries in order (with their default values)
        let lang = ini_raw(vm, "mbstring.language").filter(|v| !v.is_empty());
        apply_ini(vm, &mut st, "mbstring.language", Some(lang.as_deref().unwrap_or(b"neutral")), true);
        for key in [
            "mbstring.detect_order",
            "mbstring.http_input",
            "mbstring.http_output",
            "mbstring.internal_encoding",
            "mbstring.substitute_character",
            "mbstring.encoding_translation",
            "mbstring.strict_detection",
        ] {
            let raw = ini_raw(vm, key).filter(|v| !v.is_empty());
            // Settings with an empty value behave like their NULL default
            match key {
                "mbstring.http_input" | "mbstring.http_output" | "mbstring.internal_encoding" => {
                    if raw.is_some() {
                        apply_ini(vm, &mut st, key, raw.as_deref(), true);
                    } else {
                        // NULL default: no deprecation, fall back to core settings
                        match key {
                            "mbstring.http_input" => {
                                let e = get_input_encoding_name(vm);
                                set_http_input_ini(vm, &mut st, &e);
                            }
                            "mbstring.http_output" => {
                                let e = get_output_encoding_name(vm);
                                set_http_output_ini(&mut st, &e);
                            }
                            _ => {
                                let e = get_internal_encoding_name(vm);
                                set_internal_encoding_ini(vm, &mut st, &e);
                            }
                        }
                    }
                }
                _ => {
                    if raw.is_some() {
                        apply_ini(vm, &mut st, key, raw.as_deref(), true);
                    }
                }
            }
        }
        // RINIT
        st.current_internal_encoding = st.internal_encoding;
        st.current_http_output_encoding = st.http_output_encoding;
        st.current_filter_illegal_mode = st.filter_illegal_mode;
        st.current_filter_illegal_substchar = st.filter_illegal_substchar;
        st.illegalchars = 0;
        populate_current_detect_order_list(&mut st);
        st.ini_seen = TRACKED_INI.iter().map(|k| (*k, ini_raw(vm, k))).collect();
    } else {
        for i in 0..st.ini_seen.len() {
            let key = st.ini_seen[i].0;
            let now = ini_raw(vm, key);
            if now != st.ini_seen[i].1 {
                st.ini_seen[i].1 = now.clone();
                apply_ini(vm, &mut st, key, now.as_deref(), false);
            }
        }
    }
    MBSTRG.with(|s| *s.borrow_mut() = st);
}

pub fn populate_current_detect_order_list(st: &mut MbState) {
    if !st.detect_order_list.is_empty() {
        st.current_detect_order_list = st.detect_order_list.clone();
    } else {
        st.current_detect_order_list = st.default_detect_order_list.iter().filter_map(|no| mbfl::no2encoding(*no)).collect();
    }
}

/// Helper: (replacement char, error mode) currently in effect
pub fn illegal_settings() -> (u32, u8) {
    with(|s| (s.current_filter_illegal_substchar, s.current_filter_illegal_mode))
}

pub fn current_internal_encoding() -> &'static Encoding {
    with(|s| s.current_internal_encoding)
}

pub fn add_illegal_chars(n: usize) {
    with(|s| s.illegalchars += n)
}
