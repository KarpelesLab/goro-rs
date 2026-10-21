pub mod mbfl;
pub mod entities;
pub mod mbstring;
pub mod mime;
pub mod misc;
pub mod state;
pub mod strfuncs;
pub mod unicode;
pub mod zpp;
use std::cell::RefCell;
use std::rc::Rc;
use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

pub fn register(vm: &mut Vm) {
    vm.register_extension(b"mbstring");
    vm.register_function(b"mb_detect_encoding", mb_detect_encoding);
    vm.register_function(b"mb_internal_encoding", mb_internal_encoding);
    vm.register_function(b"mb_strlen", mb_strlen);
    vm.register_function(b"mb_strtolower", mb_strtolower);
    vm.register_function(b"mb_strtoupper", mb_strtoupper);
    vm.register_function(b"mb_substr", mb_substr);
    vm.register_function(b"mb_strpos", mb_strpos);
    vm.register_function(b"mb_strrpos", mb_strrpos);
    vm.register_function(b"mb_stripos", mb_stripos);
    vm.register_function(b"mb_strripos", mb_strripos);
    vm.register_function(b"mb_convert_encoding", mb_convert_encoding);
    vm.register_function(b"mb_substitute_character", mb_substitute_character);
    vm.register_function(b"mb_check_encoding", mb_check_encoding);
    vm.register_function(b"mb_substr_count", mb_substr_count);
    vm.register_function(b"mb_strstr", mb_strstr_fn);
    vm.register_function(b"mb_stristr", mb_stristr_fn);
    vm.register_function(b"mb_strrchr", mb_strrchr_fn);
    vm.register_function(b"mb_strrichr", mb_strrichr_fn);
    vm.register_function(b"mb_str_split", mb_str_split_fn);
    vm.register_function(b"mb_convert_case", mb_convert_case_fn);
    vm.register_function(b"mb_language", mb_language_fn);
    vm.register_function(b"mb_list_encodings", mb_list_encodings_fn);
    vm.register_function(b"mb_encoding_aliases", mb_encoding_aliases_fn);
    vm.register_function(b"mb_ord", mb_ord_fn);
    vm.register_function(b"mb_chr", mb_chr_fn);
    vm.register_function(b"mb_strcut", mb_strcut_fn);
    vm.register_function(b"mb_detect_order", mb_detect_order_fn);
    vm.register_function(b"mb_get_info", mb_get_info_fn);
    vm.register_function(b"mb_regex_encoding", mb_regex_encoding_fn);
    vm.register_function(b"mb_http_output", mb_http_output_fn);
    vm.register_function(b"mb_preferred_mime_name", mb_preferred_mime_name_fn);
    vm.register_function(b"mb_output_handler", mb_output_handler_fn);
    vm.register_function(b"mb_str_pad", mb_str_pad_fn);
    vm.register_function(b"mb_http_input", mb_http_input_fn);
    vm.register_function(b"mb_regex_set_options", mb_regex_set_options_fn);
    vm.register_function(b"mb_strimwidth", mb_strimwidth_fn);
    vm.register_function(b"mb_strwidth", mb_strwidth_fn);
    vm.register_function(b"mb_convert_kana", mb_convert_kana_fn);
    vm.register_function(b"mb_decode_numericentity", mb_decode_numericentity_fn);
    vm.register_function(b"mb_encode_numericentity", mb_encode_numericentity_fn);
    vm.register_function(b"mb_decode_mimeheader", mb_decode_mimeheader_fn);
    vm.register_function(b"mb_encode_mimeheader", mb_encode_mimeheader_fn);
    vm.register_function(b"mb_convert_variables", mb_convert_variables_fn);
    vm.register_function(b"mb_parse_str", mb_parse_str_fn);
    vm.register_function(b"mb_scrub", mb_scrub_fn);
    vm.register_function(b"mb_trim", mb_trim_fn);
    vm.register_function(b"mb_ltrim", mb_ltrim_fn);
    vm.register_function(b"mb_rtrim", mb_rtrim_fn);
    vm.register_function(b"mb_ucfirst", mb_ucfirst_fn);
    vm.register_function(b"mb_lcfirst", mb_lcfirst_fn);
    vm.register_function(b"mb_ereg", mb_ereg_fn);
    vm.register_function(b"mb_eregi", mb_eregi_fn);
    vm.register_function(b"mb_ereg_replace", mb_ereg_replace_fn);
    vm.register_function(b"mb_eregi_replace", mb_eregi_replace_fn);
    vm.register_function(b"mb_ereg_match", mb_ereg_match_fn);
    vm.register_function(b"mb_ereg_search_init", mb_ereg_search_init_fn);
    vm.register_function(b"mb_ereg_search", mb_ereg_search_fn);
    vm.register_function(b"mb_ereg_search_pos", mb_ereg_search_pos_fn);
    vm.register_function(b"mb_ereg_search_regs", mb_ereg_search_regs_fn);
    vm.register_function(b"mb_ereg_search_getregs", mb_ereg_search_getregs_fn);
    vm.register_function(b"mb_ereg_search_getpos", mb_ereg_search_getpos_fn);
    vm.register_function(b"mb_ereg_search_setpos", mb_ereg_search_setpos_fn);
    vm.register_function(b"mb_split", mb_split_fn);
    vm.register_function(b"mb_send_mail", mb_send_mail_fn);
    // New libmbfl-based implementations (override the legacy ones above)
    mbstring::register(vm);

    // Register parameter names for named argument support
    vm.builtin_param_names.insert(b"mb_strcut".to_vec(), vec![b"string".to_vec(), b"start".to_vec(), b"length".to_vec(), b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"mb_detect_order".to_vec(), vec![b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"mb_strlen".to_vec(), vec![b"string".to_vec(), b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"mb_strtolower".to_vec(), vec![b"string".to_vec(), b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"mb_strtoupper".to_vec(), vec![b"string".to_vec(), b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"mb_substr".to_vec(), vec![b"string".to_vec(), b"start".to_vec(), b"length".to_vec(), b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"mb_strpos".to_vec(), vec![b"haystack".to_vec(), b"needle".to_vec(), b"offset".to_vec(), b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"mb_strrpos".to_vec(), vec![b"haystack".to_vec(), b"needle".to_vec(), b"offset".to_vec(), b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"mb_stripos".to_vec(), vec![b"haystack".to_vec(), b"needle".to_vec(), b"offset".to_vec(), b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"mb_strripos".to_vec(), vec![b"haystack".to_vec(), b"needle".to_vec(), b"offset".to_vec(), b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"mb_convert_encoding".to_vec(), vec![b"string".to_vec(), b"to_encoding".to_vec(), b"from_encoding".to_vec()]);
    vm.builtin_param_names.insert(b"mb_detect_encoding".to_vec(), vec![b"string".to_vec(), b"encodings".to_vec(), b"strict".to_vec()]);
    vm.builtin_param_names.insert(b"mb_check_encoding".to_vec(), vec![b"value".to_vec(), b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"mb_substitute_character".to_vec(), vec![b"substitute_character".to_vec()]);
    vm.builtin_param_names.insert(b"mb_substr_count".to_vec(), vec![b"haystack".to_vec(), b"needle".to_vec(), b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"mb_internal_encoding".to_vec(), vec![b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"mb_convert_case".to_vec(), vec![b"string".to_vec(), b"mode".to_vec(), b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"mb_language".to_vec(), vec![b"language".to_vec()]);
    vm.builtin_param_names.insert(b"mb_list_encodings".to_vec(), vec![]);
    vm.builtin_param_names.insert(b"mb_encoding_aliases".to_vec(), vec![b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"mb_str_split".to_vec(), vec![b"string".to_vec(), b"length".to_vec(), b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"mb_strstr".to_vec(), vec![b"haystack".to_vec(), b"needle".to_vec(), b"before_needle".to_vec(), b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"mb_stristr".to_vec(), vec![b"haystack".to_vec(), b"needle".to_vec(), b"before_needle".to_vec(), b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"mb_strrchr".to_vec(), vec![b"haystack".to_vec(), b"needle".to_vec(), b"before_needle".to_vec(), b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"mb_strrichr".to_vec(), vec![b"haystack".to_vec(), b"needle".to_vec(), b"before_needle".to_vec(), b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"mb_ord".to_vec(), vec![b"string".to_vec(), b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"mb_chr".to_vec(), vec![b"codepoint".to_vec(), b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"mb_get_info".to_vec(), vec![b"type".to_vec()]);
    vm.builtin_param_names.insert(b"mb_regex_encoding".to_vec(), vec![b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"mb_http_output".to_vec(), vec![b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"mb_http_input".to_vec(), vec![b"type".to_vec()]);
    vm.builtin_param_names.insert(b"mb_preferred_mime_name".to_vec(), vec![b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"mb_output_handler".to_vec(), vec![b"string".to_vec(), b"status".to_vec()]);
    vm.builtin_param_names.insert(b"mb_str_pad".to_vec(), vec![b"string".to_vec(), b"length".to_vec(), b"pad_string".to_vec(), b"pad_type".to_vec(), b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"mb_regex_set_options".to_vec(), vec![b"options".to_vec()]);
    vm.builtin_param_names.insert(b"mb_strimwidth".to_vec(), vec![b"string".to_vec(), b"start".to_vec(), b"width".to_vec(), b"trim_marker".to_vec(), b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"mb_strwidth".to_vec(), vec![b"string".to_vec(), b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"mb_convert_kana".to_vec(), vec![b"string".to_vec(), b"mode".to_vec(), b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"mb_decode_numericentity".to_vec(), vec![b"string".to_vec(), b"map".to_vec(), b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"mb_encode_numericentity".to_vec(), vec![b"string".to_vec(), b"map".to_vec(), b"encoding".to_vec(), b"hex".to_vec()]);
    vm.builtin_param_names.insert(b"mb_decode_mimeheader".to_vec(), vec![b"string".to_vec()]);
    vm.builtin_param_names.insert(b"mb_encode_mimeheader".to_vec(), vec![b"string".to_vec(), b"charset".to_vec(), b"transfer_encoding".to_vec(), b"newline".to_vec(), b"indent".to_vec()]);
    vm.builtin_param_names.insert(b"mb_convert_variables".to_vec(), vec![b"to_encoding".to_vec(), b"from_encoding".to_vec(), b"var".to_vec()]);
    vm.builtin_param_names.insert(b"mb_parse_str".to_vec(), vec![b"string".to_vec(), b"result".to_vec()]);
    vm.builtin_param_names.insert(b"mb_scrub".to_vec(), vec![b"string".to_vec(), b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"mb_trim".to_vec(), vec![b"string".to_vec(), b"characters".to_vec(), b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"mb_ltrim".to_vec(), vec![b"string".to_vec(), b"characters".to_vec(), b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"mb_rtrim".to_vec(), vec![b"string".to_vec(), b"characters".to_vec(), b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"mb_ucfirst".to_vec(), vec![b"string".to_vec(), b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"mb_lcfirst".to_vec(), vec![b"string".to_vec(), b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"mb_ereg".to_vec(), vec![b"pattern".to_vec(), b"string".to_vec(), b"matches".to_vec()]);
    vm.builtin_param_names.insert(b"mb_eregi".to_vec(), vec![b"pattern".to_vec(), b"string".to_vec(), b"matches".to_vec()]);
    vm.builtin_param_names.insert(b"mb_ereg_replace".to_vec(), vec![b"pattern".to_vec(), b"replacement".to_vec(), b"string".to_vec(), b"options".to_vec()]);
    vm.builtin_param_names.insert(b"mb_eregi_replace".to_vec(), vec![b"pattern".to_vec(), b"replacement".to_vec(), b"string".to_vec(), b"options".to_vec()]);
    vm.builtin_param_names.insert(b"mb_ereg_match".to_vec(), vec![b"pattern".to_vec(), b"string".to_vec(), b"options".to_vec()]);
    vm.builtin_param_names.insert(b"mb_send_mail".to_vec(), vec![b"to".to_vec(), b"subject".to_vec(), b"message".to_vec(), b"additional_headers".to_vec(), b"additional_parameters".to_vec()]);

    // MB_CASE constants
    vm.constants.insert(b"MB_CASE_UPPER".to_vec(), Value::Long(0));
    vm.constants.insert(b"MB_CASE_LOWER".to_vec(), Value::Long(1));
    vm.constants.insert(b"MB_CASE_TITLE".to_vec(), Value::Long(2));
    vm.constants.insert(b"MB_CASE_FOLD".to_vec(), Value::Long(3));
    vm.constants.insert(b"MB_CASE_UPPER_SIMPLE".to_vec(), Value::Long(4));
    vm.constants.insert(b"MB_CASE_LOWER_SIMPLE".to_vec(), Value::Long(5));
    vm.constants.insert(b"MB_CASE_FOLD_SIMPLE".to_vec(), Value::Long(7));
    vm.constants.insert(b"MB_CASE_TITLE_SIMPLE".to_vec(), Value::Long(6));

    // Default mbstring.* INI keys to empty strings so ini_get() returns "".
    // PHP's mbstring registers these at startup with empty default values.
    // Only set if not already configured.
    for key in [
        "mbstring.http_input",
        "mbstring.http_output",
        "mbstring.internal_encoding",
        "mbstring.encoding_translation",
        "mbstring.detect_order",
        "mbstring.substitute_character",
        "mbstring.func_overload",
        "mbstring.regex_retry_limit",
        "mbstring.regex_stack_limit",
        "mbstring.strict_detection",
        "mbstring.language",
        "mbstring.http_output_conv_mimetypes",
    ] {
        if !vm.constants.contains_key(key.as_bytes()) {
            vm.constants.insert(key.as_bytes().to_vec(), Value::String(PhpString::empty()));
        }
    }
}

// ========== Encoding conversion helpers ==========

/// Resolve encoding name to encoding_rs Encoding
fn resolve_encoding(name: &str) -> Option<&'static encoding_rs::Encoding> {
    let lower = name.to_ascii_lowercase();
    let lower = lower.trim();
    match lower.as_ref() {
        "utf-8" | "utf8" => Some(encoding_rs::UTF_8),
        "ascii" | "us-ascii" | "iso646-us" => Some(encoding_rs::UTF_8), // ASCII is a subset of UTF-8
        "iso-8859-1" | "iso8859-1" | "latin1" | "latin-1" => Some(encoding_rs::WINDOWS_1252), // PHP treats ISO-8859-1 like Windows-1252
        "iso-8859-2" | "iso8859-2" | "latin2" | "latin-2" => Some(encoding_rs::ISO_8859_2),
        "iso-8859-3" | "iso8859-3" | "latin3" | "latin-3" => Some(encoding_rs::ISO_8859_3),
        "iso-8859-4" | "iso8859-4" | "latin4" | "latin-4" => Some(encoding_rs::ISO_8859_4),
        "iso-8859-5" | "iso8859-5" => Some(encoding_rs::ISO_8859_5),
        "iso-8859-6" | "iso8859-6" => Some(encoding_rs::ISO_8859_6),
        "iso-8859-7" | "iso8859-7" => Some(encoding_rs::ISO_8859_7),
        "iso-8859-8" | "iso8859-8" => Some(encoding_rs::ISO_8859_8),
        "iso-8859-9" | "iso8859-9" | "latin5" | "latin-5" => Some(encoding_rs::WINDOWS_1254),
        "iso-8859-10" | "iso8859-10" | "latin6" | "latin-6" => Some(encoding_rs::ISO_8859_10),
        "iso-8859-13" | "iso8859-13" => Some(encoding_rs::ISO_8859_13),
        "iso-8859-14" | "iso8859-14" => Some(encoding_rs::ISO_8859_14),
        "iso-8859-15" | "iso8859-15" | "latin9" | "latin-9" => Some(encoding_rs::ISO_8859_15),
        "iso-8859-16" | "iso8859-16" => Some(encoding_rs::ISO_8859_16),
        "windows-1250" | "cp1250" | "win-1250" => Some(encoding_rs::WINDOWS_1250),
        "windows-1251" | "cp1251" | "win-1251" => Some(encoding_rs::WINDOWS_1251),
        "windows-1252" | "cp1252" | "win-1252" => Some(encoding_rs::WINDOWS_1252),
        "windows-1253" | "cp1253" | "win-1253" => Some(encoding_rs::WINDOWS_1253),
        "windows-1254" | "cp1254" | "win-1254" => Some(encoding_rs::WINDOWS_1254),
        "windows-1255" | "cp1255" | "win-1255" => Some(encoding_rs::WINDOWS_1255),
        "windows-1256" | "cp1256" | "win-1256" => Some(encoding_rs::WINDOWS_1256),
        "windows-1257" | "cp1257" | "win-1257" => Some(encoding_rs::WINDOWS_1257),
        "windows-1258" | "cp1258" | "win-1258" => Some(encoding_rs::WINDOWS_1258),
        "euc-jp" | "eucjp" | "euc_jp" => Some(encoding_rs::EUC_JP),
        "shift_jis" | "sjis" | "sjis-win" | "cp932" | "windows-31j" | "ms_kanji" => Some(encoding_rs::SHIFT_JIS),
        "iso-2022-jp" | "jis" | "jis-ms" => Some(encoding_rs::ISO_2022_JP),
        "euc-kr" | "euckr" | "euc_kr" | "uhc" | "cp949" => Some(encoding_rs::EUC_KR),
        "gb2312" | "gb18030" | "gbk" | "cp936" | "euc-cn" | "hz-gb-2312" | "hz" => Some(encoding_rs::GBK),
        "big5" | "big-5" | "cn-big5" | "cp950" | "big-five" => Some(encoding_rs::BIG5),
        "utf-16" | "utf16" => Some(encoding_rs::UTF_16LE),
        "utf-16be" | "utf16be" => Some(encoding_rs::UTF_16BE),
        "utf-16le" | "utf16le" => Some(encoding_rs::UTF_16LE),
        "koi8-r" | "koi8r" => Some(encoding_rs::KOI8_R),
        "koi8-u" | "koi8u" => Some(encoding_rs::KOI8_U),
        "macintosh" | "mac-roman" | "x-mac-roman" => Some(encoding_rs::MACINTOSH),
        _ => encoding_rs::Encoding::for_label(lower.as_bytes()),
    }
}

/// Decode bytes in a UCS-4/UTF-32 encoding to a UTF-8 string.
fn decode_ucs4(bytes: &[u8], be: bool) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i + 3 < bytes.len() {
        let cp = if be {
            ((bytes[i] as u32) << 24)
                | ((bytes[i+1] as u32) << 16)
                | ((bytes[i+2] as u32) << 8)
                | (bytes[i+3] as u32)
        } else {
            (bytes[i] as u32)
                | ((bytes[i+1] as u32) << 8)
                | ((bytes[i+2] as u32) << 16)
                | ((bytes[i+3] as u32) << 24)
        };
        if let Some(c) = char::from_u32(cp) {
            out.push(c);
        } else {
            out.push('\u{FFFD}');
        }
        i += 4;
    }
    out
}

/// Encode a UTF-8 string to UCS-4/UTF-32 bytes.
fn encode_ucs4(s: &str, be: bool) -> Vec<u8> {
    let mut out = Vec::new();
    for c in s.chars() {
        let cp = c as u32;
        if be {
            out.push((cp >> 24) as u8);
            out.push((cp >> 16) as u8);
            out.push((cp >> 8) as u8);
            out.push(cp as u8);
        } else {
            out.push(cp as u8);
            out.push((cp >> 8) as u8);
            out.push((cp >> 16) as u8);
            out.push((cp >> 24) as u8);
        }
    }
    out
}

/// Decode UCS-2 bytes (2 bytes per codepoint, no surrogate handling) to UTF-8.
/// UCS-2 is restricted to the BMP (U+0000..U+FFFF).
fn decode_ucs2(bytes: &[u8], be: bool) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i + 1 < bytes.len() {
        let cp = if be {
            ((bytes[i] as u32) << 8) | (bytes[i + 1] as u32)
        } else {
            (bytes[i] as u32) | ((bytes[i + 1] as u32) << 8)
        };
        if let Some(c) = char::from_u32(cp) {
            out.push(c);
        } else {
            out.push('\u{FFFD}');
        }
        i += 2;
    }
    out
}

/// Encode a UTF-8 string to UCS-2 bytes (BMP only; higher codepoints replaced
/// with U+FFFD).
fn encode_ucs2(s: &str, be: bool) -> Vec<u8> {
    let mut out = Vec::new();
    for c in s.chars() {
        let cp = c as u32;
        let val: u16 = if cp <= 0xFFFF { cp as u16 } else { 0xFFFD };
        if be {
            out.push((val >> 8) as u8);
            out.push(val as u8);
        } else {
            out.push(val as u8);
            out.push((val >> 8) as u8);
        }
    }
    out
}

/// Decode UTF-16 bytes (with surrogate pair handling) to UTF-8.
fn decode_utf16(bytes: &[u8], be: bool) -> String {
    let mut units: Vec<u16> = Vec::with_capacity(bytes.len() / 2);
    let mut i = 0;
    while i + 1 < bytes.len() {
        let u = if be {
            ((bytes[i] as u16) << 8) | (bytes[i + 1] as u16)
        } else {
            (bytes[i] as u16) | ((bytes[i + 1] as u16) << 8)
        };
        units.push(u);
        i += 2;
    }
    String::from_utf16_lossy(&units)
}

/// Encode a UTF-8 string to UTF-16 bytes (with surrogate pairs).
fn encode_utf16(s: &str, be: bool) -> Vec<u8> {
    let mut out = Vec::new();
    for u in s.encode_utf16() {
        if be {
            out.push((u >> 8) as u8);
            out.push(u as u8);
        } else {
            out.push(u as u8);
            out.push((u >> 8) as u8);
        }
    }
    out
}

/// Decode HTML numeric entities (&#N; and &#xH;) to UTF-8.
fn decode_html_entities(data: &[u8]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < data.len() {
        if data[i] == b'&' && i + 1 < data.len() && data[i+1] == b'#' {
            let start = i;
            let mut j = i + 2;
            let is_hex = j < data.len() && (data[j] == b'x' || data[j] == b'X');
            if is_hex { j += 1; }
            let num_start = j;
            while j < data.len() {
                let c = data[j];
                let ok = if is_hex {
                    c.is_ascii_hexdigit()
                } else {
                    c.is_ascii_digit()
                };
                if !ok { break; }
                j += 1;
            }
            if j > num_start && j < data.len() && data[j] == b';' {
                let num_str = std::str::from_utf8(&data[num_start..j]).unwrap_or("0");
                let parsed = if is_hex {
                    u32::from_str_radix(num_str, 16).ok()
                } else {
                    num_str.parse::<u32>().ok()
                };
                if let Some(cp) = parsed {
                    // Codepoints > 0x10FFFF are out of Unicode range: leave
                    // the entity unchanged. Codepoints in surrogate range
                    // (D800..DFFF) are also invalid; treat them the same.
                    if cp <= 0x10FFFF {
                        if let Some(c) = char::from_u32(cp) {
                            out.push(c);
                            i = j + 1;
                            continue;
                        }
                    }
                    // Fall through: pass-through unchanged.
                }
            }
            out.push(data[start] as char);
            i = start + 1;
        } else {
            if data[i] < 0x80 {
                out.push(data[i] as char);
            } else {
                out.push(data[i] as char);
            }
            i += 1;
        }
    }
    out
}

/// Convert bytes from one encoding to another, returning the result as bytes
fn convert_encoding(data: &[u8], to_enc: &str, from_enc: &str) -> Option<Vec<u8>> {
    if data.is_empty() {
        return Some(Vec::new());
    }

    let to_lower = to_enc.to_ascii_lowercase();
    let from_lower = from_enc.to_ascii_lowercase();

    // Same encoding - pass through
    if to_lower == from_lower {
        return Some(data.to_vec());
    }

    // UCS-4 / UTF-32 family
    let is_ucs4_be = |s: &str| matches!(s, "ucs-4" | "ucs4" | "ucs-4be" | "ucs4be" | "utf-32be" | "utf32be");
    let is_ucs4_le = |s: &str| matches!(s, "ucs-4le" | "ucs4le" | "utf-32" | "utf32" | "utf-32le" | "utf32le");
    if is_ucs4_be(&from_lower) || is_ucs4_le(&from_lower) {
        let s = decode_ucs4(data, is_ucs4_be(&from_lower));
        if to_lower == "utf-8" || to_lower == "utf8" {
            return Some(s.into_bytes());
        }
        if is_ucs4_be(&to_lower) || is_ucs4_le(&to_lower) {
            return Some(encode_ucs4(&s, is_ucs4_be(&to_lower)));
        }
        if let Some(enc) = resolve_encoding(&to_lower) {
            let (cow, _, _) = enc.encode(&s);
            return Some(cow.to_vec());
        }
        return Some(s.into_bytes());
    }
    if is_ucs4_be(&to_lower) || is_ucs4_le(&to_lower) {
        let utf8_str = if from_lower == "utf-8" || from_lower == "utf8" || from_lower == "ascii" {
            String::from_utf8_lossy(data).to_string()
        } else if let Some(enc) = resolve_encoding(&from_lower) {
            let (cow, _, _) = enc.decode(data);
            cow.to_string()
        } else {
            String::from_utf8_lossy(data).to_string()
        };
        return Some(encode_ucs4(&utf8_str, is_ucs4_be(&to_lower)));
    }

    // UCS-2 family (no surrogate handling, BMP only)
    let is_ucs2_be = |s: &str| matches!(s, "ucs-2" | "ucs2" | "ucs-2be" | "ucs2be");
    let is_ucs2_le = |s: &str| matches!(s, "ucs-2le" | "ucs2le");
    if is_ucs2_be(&from_lower) || is_ucs2_le(&from_lower) {
        let s = decode_ucs2(data, is_ucs2_be(&from_lower));
        if to_lower == "utf-8" || to_lower == "utf8" {
            return Some(s.into_bytes());
        }
        if is_ucs2_be(&to_lower) || is_ucs2_le(&to_lower) {
            return Some(encode_ucs2(&s, is_ucs2_be(&to_lower)));
        }
        if let Some(enc) = resolve_encoding(&to_lower) {
            let (cow, _, _) = enc.encode(&s);
            return Some(cow.to_vec());
        }
        return Some(s.into_bytes());
    }
    if is_ucs2_be(&to_lower) || is_ucs2_le(&to_lower) {
        let utf8_str = if from_lower == "utf-8" || from_lower == "utf8" || from_lower == "ascii" {
            String::from_utf8_lossy(data).to_string()
        } else if let Some(enc) = resolve_encoding(&from_lower) {
            let (cow, _, _) = enc.decode(data);
            cow.to_string()
        } else {
            String::from_utf8_lossy(data).to_string()
        };
        return Some(encode_ucs2(&utf8_str, is_ucs2_be(&to_lower)));
    }

    // UTF-16 family (with surrogate pair handling)
    let is_utf16_be = |s: &str| matches!(s, "utf-16be" | "utf16be");
    let is_utf16_le = |s: &str| matches!(s, "utf-16" | "utf16" | "utf-16le" | "utf16le");
    if is_utf16_be(&from_lower) || is_utf16_le(&from_lower) {
        // Strip BOM if present at start
        let (bom_data, _bom_be) = if data.len() >= 2 {
            if data[0] == 0xFE && data[1] == 0xFF {
                (&data[2..], Some(true))
            } else if data[0] == 0xFF && data[1] == 0xFE {
                (&data[2..], Some(false))
            } else {
                (data, None)
            }
        } else {
            (data, None)
        };
        let s = decode_utf16(bom_data, is_utf16_be(&from_lower));
        if to_lower == "utf-8" || to_lower == "utf8" {
            return Some(s.into_bytes());
        }
        if is_utf16_be(&to_lower) || is_utf16_le(&to_lower) {
            return Some(encode_utf16(&s, is_utf16_be(&to_lower)));
        }
        if let Some(enc) = resolve_encoding(&to_lower) {
            let (cow, _, _) = enc.encode(&s);
            return Some(cow.to_vec());
        }
        return Some(s.into_bytes());
    }
    if is_utf16_be(&to_lower) || is_utf16_le(&to_lower) {
        let utf8_str = if from_lower == "utf-8" || from_lower == "utf8" || from_lower == "ascii" {
            String::from_utf8_lossy(data).to_string()
        } else if let Some(enc) = resolve_encoding(&from_lower) {
            let (cow, _, _) = enc.decode(data);
            cow.to_string()
        } else {
            String::from_utf8_lossy(data).to_string()
        };
        return Some(encode_utf16(&utf8_str, is_utf16_be(&to_lower)));
    }

    // HTML-ENTITIES source: decode numeric entities
    if from_lower == "html-entities" || from_lower == "html" {
        let decoded = decode_html_entities(data);
        if to_lower == "utf-8" || to_lower == "utf8" {
            return Some(decoded.into_bytes());
        }
        if let Some(enc) = resolve_encoding(&to_lower) {
            let (cow, _, _) = enc.encode(&decoded);
            return Some(cow.to_vec());
        }
        return Some(decoded.into_bytes());
    }

    // Handle "Base64" encoding
    if to_lower == "base64" {
        // Decode from source encoding to UTF-8, then base64 encode
        let utf8_data = if from_lower == "utf-8" || from_lower == "utf8" || from_lower == "ascii" {
            data.to_vec()
        } else if let Some(enc) = resolve_encoding(&from_lower) {
            let (cow, _, _) = enc.decode(data);
            cow.as_bytes().to_vec()
        } else {
            data.to_vec()
        };
        use std::io::Write;
        let mut buf = Vec::new();
        let _ = write!(&mut buf, "{}", base64_encode(&utf8_data));
        return Some(buf);
    }

    if from_lower == "base64" {
        // Base64 decode, then convert to target encoding
        let decoded = base64_decode(data);
        if to_lower == "utf-8" || to_lower == "utf8" {
            return Some(decoded);
        }
        if let Some(enc) = resolve_encoding(&to_lower) {
            let s = String::from_utf8_lossy(&decoded);
            let (cow, _, _) = enc.encode(&s);
            return Some(cow.to_vec());
        }
        return Some(decoded);
    }

    // Step 1: Decode from source encoding to UTF-8 string
    let utf8_string = if from_lower == "utf-8" || from_lower == "utf8" || from_lower == "ascii" {
        String::from_utf8_lossy(data).to_string()
    } else if let Some(from_encoding) = resolve_encoding(&from_lower) {
        let (cow, _, _) = from_encoding.decode(data);
        cow.to_string()
    } else {
        // Unknown encoding - pass through
        return Some(data.to_vec());
    };

    // Step 2: Encode to target encoding
    if to_lower == "utf-8" || to_lower == "utf8" || to_lower == "ascii" {
        Some(utf8_string.into_bytes())
    } else if let Some(to_encoding) = resolve_encoding(&to_lower) {
        let (cow, _, _) = to_encoding.encode(&utf8_string);
        Some(cow.to_vec())
    } else {
        Some(utf8_string.into_bytes())
    }
}

fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    let chunks = data.chunks(3);
    let total_chunks = (data.len() + 2) / 3;
    for (i, chunk) in chunks.enumerate() {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        // Add line break every 76 chars (MIME style) for multi-line
        if (i + 1) % 19 == 0 && i + 1 < total_chunks {
            result.push('\r');
            result.push('\n');
        }
    }
    result
}

fn base64_decode(data: &[u8]) -> Vec<u8> {
    let mut result = Vec::new();
    let mut buf = 0u32;
    let mut bits = 0;
    for &b in data {
        let val = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            _ => continue,
        };
        buf = (buf << 6) | val as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            result.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    result
}

// ========== Function implementations ==========

fn mb_detect_encoding(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = s.as_bytes();

    // If strict mode and encoding list provided
    let strict = args.get(2).map(|v| v.is_truthy()).unwrap_or(false);

    // If encoding list provided
    if let Some(enc_arg) = args.get(1) {
        match enc_arg {
            Value::Array(arr) => {
                let arr = arr.borrow();
                for (_, v) in arr.iter() {
                    let enc_name = v.to_php_string().to_string_lossy();
                    let enc_lower = enc_name.to_ascii_lowercase();
                    if enc_lower == "ascii" || enc_lower == "us-ascii" {
                        if bytes.iter().all(|&b| b < 128) {
                            return Ok(Value::String(PhpString::from_bytes(b"ASCII")));
                        }
                    } else if enc_lower == "utf-8" || enc_lower == "utf8" {
                        if std::str::from_utf8(bytes).is_ok() {
                            return Ok(Value::String(PhpString::from_bytes(b"UTF-8")));
                        }
                    }
                }
                if strict {
                    return Ok(Value::False);
                }
            }
            Value::String(_) => {
                let enc_name = enc_arg.to_php_string().to_string_lossy();
                // Could be comma-separated list
                for enc in enc_name.split(',') {
                    let enc = enc.trim().to_ascii_lowercase();
                    if enc == "ascii" || enc == "us-ascii" {
                        if bytes.iter().all(|&b| b < 128) {
                            return Ok(Value::String(PhpString::from_bytes(b"ASCII")));
                        }
                    } else if enc == "utf-8" || enc == "utf8" {
                        if std::str::from_utf8(bytes).is_ok() {
                            return Ok(Value::String(PhpString::from_bytes(b"UTF-8")));
                        }
                    }
                }
                if strict {
                    return Ok(Value::False);
                }
            }
            _ => {}
        }
    }

    // Default detection
    if bytes.iter().all(|&b| b < 128) {
        return Ok(Value::String(PhpString::from_bytes(b"ASCII")));
    }
    if std::str::from_utf8(bytes).is_ok() {
        return Ok(Value::String(PhpString::from_bytes(b"UTF-8")));
    }
    Ok(Value::String(PhpString::from_bytes(b"UTF-8")))
}

/// Get the currently effective internal encoding from the VM, checking
/// the runtime override (set by mb_internal_encoding()), then
/// `mbstring.internal_encoding` (mbstring-specific), and finally the core
/// `internal_encoding` directive. Defaults to UTF-8 if unset.
fn current_internal_encoding(vm: &Vm) -> String {
    // Runtime override has highest priority
    if let Some(val) = vm.constants.get(b"__mbstring.runtime_internal_encoding".as_ref()) {
        let s = val.to_php_string().to_string_lossy();
        if !s.is_empty() {
            return canonical_encoding_name(&s);
        }
    }
    if let Some(val) = vm.constants.get(b"mbstring.internal_encoding".as_ref()) {
        let s = val.to_php_string().to_string_lossy();
        if !s.is_empty() {
            return canonical_encoding_name(&s);
        }
    }
    if let Some(val) = vm.constants.get(b"internal_encoding".as_ref()) {
        let s = val.to_php_string().to_string_lossy();
        if !s.is_empty() {
            return canonical_encoding_name(&s);
        }
    }
    "UTF-8".to_string()
}

/// Convert encoding aliases to their canonical PHP names (e.g. "utf-8" -> "UTF-8").
fn canonical_encoding_name(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        "utf-8" | "utf8" => "UTF-8".to_string(),
        "ucs-2" | "ucs2" => "UCS-2".to_string(),
        "ucs-2be" | "ucs2be" => "UCS-2BE".to_string(),
        "ucs-2le" | "ucs2le" => "UCS-2LE".to_string(),
        "ucs-4" | "ucs4" => "UCS-4".to_string(),
        "ucs-4be" | "ucs4be" => "UCS-4BE".to_string(),
        "ucs-4le" | "ucs4le" => "UCS-4LE".to_string(),
        "utf-16" | "utf16" => "UTF-16".to_string(),
        "utf-16be" | "utf16be" => "UTF-16BE".to_string(),
        "utf-16le" | "utf16le" => "UTF-16LE".to_string(),
        "utf-32" | "utf32" => "UTF-32".to_string(),
        "utf-32be" | "utf32be" => "UTF-32BE".to_string(),
        "utf-32le" | "utf32le" => "UTF-32LE".to_string(),
        "utf-7" | "utf7" => "UTF-7".to_string(),
        "utf7-imap" | "utf-7-imap" => "UTF7-IMAP".to_string(),
        "iso-8859-1" | "iso8859-1" | "latin1" => "ISO-8859-1".to_string(),
        "iso-8859-2" | "iso8859-2" | "latin2" => "ISO-8859-2".to_string(),
        "iso-8859-3" | "iso8859-3" | "latin3" => "ISO-8859-3".to_string(),
        "iso-8859-4" | "iso8859-4" | "latin4" => "ISO-8859-4".to_string(),
        "iso-8859-5" | "iso8859-5" => "ISO-8859-5".to_string(),
        "iso-8859-6" | "iso8859-6" => "ISO-8859-6".to_string(),
        "iso-8859-7" | "iso8859-7" => "ISO-8859-7".to_string(),
        "iso-8859-8" | "iso8859-8" => "ISO-8859-8".to_string(),
        "iso-8859-9" | "iso8859-9" => "ISO-8859-9".to_string(),
        "iso-8859-10" | "iso8859-10" => "ISO-8859-10".to_string(),
        "iso-8859-13" | "iso8859-13" => "ISO-8859-13".to_string(),
        "iso-8859-14" | "iso8859-14" => "ISO-8859-14".to_string(),
        "iso-8859-15" | "iso8859-15" => "ISO-8859-15".to_string(),
        "iso-8859-16" | "iso8859-16" => "ISO-8859-16".to_string(),
        "ascii" | "us-ascii" => "ASCII".to_string(),
        "euc-jp" | "eucjp" => "EUC-JP".to_string(),
        "sjis" | "shift_jis" | "shift-jis" => "SJIS".to_string(),
        "eucjp-win" => "eucJP-win".to_string(),
        "sjis-win" => "SJIS-win".to_string(),
        "uhc" => "UHC".to_string(),
        "hz" => "HZ".to_string(),
        "base64" => "BASE64".to_string(),
        "7bit" => "7bit".to_string(),
        "8bit" => "8bit".to_string(),
        "binary" => "BINARY".to_string(),
        "html-entities" | "html" => "HTML-ENTITIES".to_string(),
        "iso-2022-jp" => "ISO-2022-JP".to_string(),
        "jis" => "JIS".to_string(),
        "iso-2022-kr" => "ISO-2022-KR".to_string(),
        "euc-cn" | "euccn" => "EUC-CN".to_string(),
        "euc-kr" | "euckr" => "EUC-KR".to_string(),
        "euc-tw" | "euctw" => "EUC-TW".to_string(),
        "big-5" | "big5" => "BIG-5".to_string(),
        "gb18030" => "GB18030".to_string(),
        "cp866" => "CP866".to_string(),
        "cp932" => "CP932".to_string(),
        "cp936" => "CP936".to_string(),
        "cp950" => "CP950".to_string(),
        "cp1251" => "Windows-1251".to_string(),
        "cp1252" => "Windows-1252".to_string(),
        "cp1254" => "Windows-1254".to_string(),
        "windows-1251" => "Windows-1251".to_string(),
        "windows-1252" => "Windows-1252".to_string(),
        "windows-1254" => "Windows-1254".to_string(),
        "koi8-r" | "koi8r" => "KOI8-R".to_string(),
        "koi8-u" | "koi8u" => "KOI8-U".to_string(),
        "armscii-8" | "armscii8" => "ArmSCII-8".to_string(),
        _ => name.to_string(),
    }
}

fn mb_internal_encoding(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() || matches!(args.first(), Some(Value::Null)) {
        // Return the currently configured internal encoding
        return Ok(Value::String(PhpString::from_string(current_internal_encoding(vm))));
    } else {
        // Set the internal encoding
        let enc = args[0].to_php_string().to_string_lossy();
        let enc_lower = enc.to_ascii_lowercase();
        // Accept encoding_rs encodings + known PHP mbstring encodings
        let is_valid = resolve_encoding(&enc).is_some()
            || matches!(enc_lower.as_str(),
                "utf-8" | "ascii" | "jis" | "utf-7" | "utf7"
                | "utf7-imap" | "utf-7-imap"
                | "ucs-4" | "ucs4" | "ucs-4be" | "ucs4be" | "ucs-4le" | "ucs4le"
                | "ucs-2" | "ucs2" | "ucs-2be" | "ucs2be" | "ucs-2le" | "ucs2le"
                | "utf-16" | "utf16" | "utf-16be" | "utf16be" | "utf-16le" | "utf16le"
                | "utf-32" | "utf32" | "utf-32be" | "utf32be" | "utf-32le" | "utf32le"
                | "cp50220" | "cp50221" | "cp50222" | "iso-2022-jp-ms"
                | "uuencode" | "qprint" | "quoted-printable" | "html" | "html-entities"
                | "7bit" | "8bit" | "base64" | "auto"
                | "euc-jp-2004" | "macjapanese" | "cp932" | "cp51932"
                | "iso-8859-1" | "iso-8859-2" | "iso-8859-3" | "iso-8859-4"
                | "iso-8859-5" | "iso-8859-6" | "iso-8859-7" | "iso-8859-8"
                | "iso-8859-9" | "iso-8859-10" | "iso-8859-13" | "iso-8859-14"
                | "iso-8859-15" | "iso-8859-16"
                | "eucjp-win" | "sjis-win"
                | "sjis-mac" | "sjis-mobile#docomo" | "sjis-mobile#kddi" | "sjis-mobile#softbank"
                | "utf8-mobile#docomo" | "utf8-mobile#kddi-a" | "utf8-mobile#kddi-b" | "utf8-mobile#softbank"
                | "iso-2022-jp-mobile#kddi" | "iso-2022-jp-2004" | "iso-2022-jp-kddi"
                | "sjis-2004"
                | "hz" | "uhc" | "euc-cn" | "euc-tw" | "euc-kr"
                | "cp936" | "cp950" | "big-5" | "big5"
                | "iso-2022-kr" | "armscii-8" | "armscii8"
                | "windows-1251" | "windows-1252" | "windows-1254" | "windows-1255" | "windows-1256" | "windows-1257" | "windows-1258"
                | "cp866" | "cp1251" | "cp1252" | "cp1254"
                | "koi8-r" | "koi8r" | "koi8-u" | "koi8u"
                | "gb18030" | "gb18030-2022"
                | "binary"
            );
        if is_valid {
            let canonical = canonical_encoding_name(&enc);
            // mb_internal_encoding() sets a runtime override that does NOT
            // modify the INI value (this matches PHP behaviour - ini_get()
            // returns the configured INI value, not the runtime one).
            vm.constants.insert(b"__mbstring.runtime_internal_encoding".to_vec(),
                Value::String(PhpString::from_string(canonical)));
            Ok(Value::True)
        } else {
            let msg = format!("mb_internal_encoding(): Argument #1 ($encoding) must be a valid encoding, \"{}\" given", enc);
            let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
        }
    }
}

fn mb_strlen(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let encoding = validate_encoding(_vm, "mb_strlen", 2, "$encoding", args, 1)?;
    let bytes = s.as_bytes();

    let default_enc = current_internal_encoding(_vm);
    let enc_str: String = encoding.clone().unwrap_or(default_enc);
    let enc_lower = enc_str.to_ascii_lowercase();
    if enc_lower == "utf-8" || enc_lower == "utf8" || enc_lower == "ascii" {
        let count = String::from_utf8_lossy(bytes).chars().count();
        return Ok(Value::Long(count as i64));
    }
    // For other encodings, convert to UTF-8 then count
    if let Some(converted) = convert_encoding(bytes, "UTF-8", &enc_str) {
        let count = String::from_utf8_lossy(&converted).chars().count();
        return Ok(Value::Long(count as i64));
    }
    // Fallback: byte count if conversion fails
    let count = String::from_utf8_lossy(bytes).chars().count();
    Ok(Value::Long(count as i64))
}

/// Lowercase a string with PHP/Unicode semantics:
/// - Titlecase characters (Lt) lowercase to the simple lowercase mapping.
/// - Capital sigma Σ at end-of-word becomes final sigma ς; otherwise σ.
///   End-of-word scan-back is limited to 64 characters to match PHP behavior.
fn php_lowercase(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    for i in 0..chars.len() {
        let c = chars[i];
        if c == '\u{03A3}' {
            let cased_before = {
                let mut found = false;
                let mut scanned = 0usize;
                for j in (0..i).rev() {
                    if scanned >= 64 { break; }
                    scanned += 1;
                    let cj = chars[j];
                    if is_case_ignorable(cj) { continue; }
                    found = cj.is_alphabetic() && (cj.is_lowercase() || cj.is_uppercase());
                    break;
                }
                found
            };
            let cased_after = if cased_before {
                let mut found = false;
                for &cj in chars.iter().skip(i+1) {
                    if is_case_ignorable(cj) { continue; }
                    found = cj.is_alphabetic() && (cj.is_lowercase() || cj.is_uppercase());
                    break;
                }
                found
            } else {
                false
            };
            if cased_before && !cased_after {
                out.push('\u{03C2}');
            } else {
                out.push('\u{03C3}');
            }
            continue;
        }
        if let Some(lc) = titlecase_to_lowercase(c) {
            out.push(lc);
            continue;
        }
        for lc in c.to_lowercase() {
            out.push(lc);
        }
    }
    out
}

fn is_case_ignorable(c: char) -> bool {
    matches!(c,
        '\'' | '\u{2019}' | '.' | ':' | '\u{00B7}' | '\u{2027}' | '\u{05F4}' | '\u{02BC}'
    )
    || ('\u{0300}'..='\u{036F}').contains(&c)
    || ('\u{1AB0}'..='\u{1AFF}').contains(&c)
    || ('\u{1DC0}'..='\u{1DFF}').contains(&c)
    || ('\u{20D0}'..='\u{20FF}').contains(&c)
    || ('\u{FE20}'..='\u{FE2F}').contains(&c)
}

fn titlecase_to_lowercase(c: char) -> Option<char> {
    match c {
        '\u{01C5}' => Some('\u{01C6}'),
        '\u{01C8}' => Some('\u{01C9}'),
        '\u{01CB}' => Some('\u{01CC}'),
        '\u{01F2}' => Some('\u{01F3}'),
        '\u{1F88}'..='\u{1F8F}' => Some(char::from_u32(c as u32 - 0x08).unwrap()),
        '\u{1F98}'..='\u{1F9F}' => Some(char::from_u32(c as u32 - 0x08).unwrap()),
        '\u{1FA8}'..='\u{1FAF}' => Some(char::from_u32(c as u32 - 0x08).unwrap()),
        '\u{1FBC}' => Some('\u{1FB3}'),
        '\u{1FCC}' => Some('\u{1FC3}'),
        '\u{1FFC}' => Some('\u{1FF3}'),
        _ => None,
    }
}

/// Map a character to its titlecase form, for MB_CASE_TITLE. Returns None
/// to fall back to uppercase semantics.
fn to_titlecase_char(c: char) -> Option<char> {
    match c {
        '\u{01C4}' | '\u{01C5}' | '\u{01C6}' => Some('\u{01C5}'),
        '\u{01C7}' | '\u{01C8}' | '\u{01C9}' => Some('\u{01C8}'),
        '\u{01CA}' | '\u{01CB}' | '\u{01CC}' => Some('\u{01CB}'),
        '\u{01F1}' | '\u{01F2}' | '\u{01F3}' => Some('\u{01F2}'),
        '\u{1FB3}' | '\u{1FBC}' => Some('\u{1FBC}'),
        '\u{1FC3}' | '\u{1FCC}' => Some('\u{1FCC}'),
        '\u{1FF3}' | '\u{1FFC}' => Some('\u{1FFC}'),
        _ => None,
    }
}

fn php_uppercase(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        // Lt digraphs -> uppercase digraph (Ǆ).
        if let Some(uc) = match c {
            '\u{01C5}' => Some('\u{01C4}'),
            '\u{01C8}' => Some('\u{01C7}'),
            '\u{01CB}' => Some('\u{01CA}'),
            '\u{01F2}' => Some('\u{01F1}'),
            _ => None,
        } {
            out.push(uc);
            continue;
        }
        for uc in c.to_uppercase() {
            out.push(uc);
        }
    }
    out
}

fn mb_strtolower(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let _ = validate_encoding(_vm, "mb_strtolower", 2, "$encoding", args, 1)?;
    let bytes = s.as_bytes();
    if let Ok(s) = std::str::from_utf8(bytes) {
        let lower = php_lowercase(s);
        Ok(Value::String(PhpString::from_vec(lower.into_bytes())))
    } else {
        let lower: Vec<u8> = bytes.iter().map(|b| b.to_ascii_lowercase()).collect();
        Ok(Value::String(PhpString::from_vec(lower)))
    }
}

fn mb_strtoupper(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let _ = validate_encoding(_vm, "mb_strtoupper", 2, "$encoding", args, 1)?;
    let bytes = s.as_bytes();
    if let Ok(s) = std::str::from_utf8(bytes) {
        let upper = php_uppercase(s);
        Ok(Value::String(PhpString::from_vec(upper.into_bytes())))
    } else {
        let upper: Vec<u8> = bytes.iter().map(|b| b.to_ascii_uppercase()).collect();
        Ok(Value::String(PhpString::from_vec(upper)))
    }
}

fn mb_substr(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let enc_opt = validate_encoding(_vm, "mb_substr", 4, "$encoding", args, 3)?;
    let default_enc = current_internal_encoding(_vm);
    let enc_str: String = enc_opt.unwrap_or(default_enc);
    let enc_lower = enc_str.to_ascii_lowercase();
    let bytes = s.as_bytes();

    // PHP only rejects exactly i64::MIN for $start and $length (because
    // negating that value overflows). Any other i64 is accepted.
    if let Some(start_v) = args.get(1) {
        if start_v.to_long() == i64::MIN {
            let msg = format!(
                "mb_substr(): Argument #2 ($start) must be between {} and {}",
                i64::MIN + 1, i64::MAX
            );
            let exc = _vm.create_exception(b"ValueError", &msg, 0);
            _vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: _vm.current_line });
        }
    }
    if let Some(len_v) = args.get(2) {
        if !matches!(len_v, Value::Null) && len_v.to_long() == i64::MIN {
            let msg = format!(
                "mb_substr(): Argument #3 ($length) must be between {} and {}",
                i64::MIN + 1, i64::MAX
            );
            let exc = _vm.create_exception(b"ValueError", &msg, 0);
            _vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: _vm.current_line });
        }
    }

    // Fast path: UTF-8 / ASCII — work with chars directly.
    let utf8_path = enc_lower == "utf-8" || enc_lower == "utf8" || enc_lower == "ascii";

    // Collect characters: each char is a chunk of bytes (e.g. 4 bytes for UTF-32LE).
    let chars: Vec<Vec<u8>> = if utf8_path {
        utf8_chars(bytes).into_iter().map(|c| c.to_vec()).collect()
    } else {
        // Convert to UTF-8 to count chars, then split back into per-codepoint
        // chunks in the original encoding by encoding each codepoint individually.
        if let Some(utf8) = convert_encoding(bytes, "UTF-8", &enc_str) {
            if let Ok(s_utf8) = std::str::from_utf8(&utf8) {
                s_utf8
                    .chars()
                    .map(|c| {
                        let mut buf = [0u8; 4];
                        let utf8_seq = c.encode_utf8(&mut buf);
                        match convert_encoding(utf8_seq.as_bytes(), &enc_str, "UTF-8") {
                            Some(b) => b,
                            None => utf8_seq.as_bytes().to_vec(),
                        }
                    })
                    .collect()
            } else {
                utf8_chars(bytes).into_iter().map(|c| c.to_vec()).collect()
            }
        } else {
            utf8_chars(bytes).into_iter().map(|c| c.to_vec()).collect()
        }
    };
    let char_count = chars.len() as i64;

    let mut start = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    if start < 0 {
        start = (char_count + start).max(0);
    }
    let start = start.min(char_count) as usize;

    let length = args.get(2).and_then(|v| if matches!(v, Value::Null) { None } else { Some(v.to_long()) });
    let end = match length {
        Some(l) if l < 0 => {
            let end_pos = char_count + l;
            if end_pos <= start as i64 { start } else { end_pos as usize }
        }
        Some(l) if l >= 0 => {
            let end_pos = start as i64 + l;
            if end_pos > char_count { char_count as usize } else { end_pos as usize }
        }
        Some(_) => start,
        None => char_count as usize,
    };

    if start >= chars.len() || start >= end {
        return Ok(Value::String(PhpString::empty()));
    }

    let mut result = Vec::new();
    for c in &chars[start..end] {
        result.extend_from_slice(c);
    }
    Ok(Value::String(PhpString::from_vec(result)))
}

fn mb_strpos(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let haystack = args.first().unwrap_or(&Value::Null).to_php_string();
    let needle = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let offset = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    let _ = validate_encoding(vm, "mb_strpos", 4, "$encoding", args, 3)?;
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    // PHP 8.0+: empty needle returns offset position
    if n.is_empty() {
        let chars: Vec<usize> = utf8_char_positions(h);
        let char_count = chars.len() as i64;
        let pos = if offset < 0 { char_count + offset } else { offset };
        if pos < 0 || pos > char_count {
            let msg = "mb_strpos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)";
            let exc = vm.create_exception(b"ValueError", msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg.to_string(), line: vm.current_line });
        }
        return Ok(Value::Long(pos));
    }
    let chars: Vec<usize> = utf8_char_positions(h);
    let char_count = chars.len() as i64;

    // Validate offset
    let start_char = if offset < 0 {
        let from = char_count + offset;
        if from < 0 {
            let msg = "mb_strpos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)";
            let exc = vm.create_exception(b"ValueError", msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg.to_string(), line: vm.current_line });
        }
        from as usize
    } else {
        if offset > char_count {
            let msg = "mb_strpos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)";
            let exc = vm.create_exception(b"ValueError", msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg.to_string(), line: vm.current_line });
        }
        offset as usize
    };

    let start_byte = if start_char < chars.len() { chars[start_char] } else { h.len() };

    if start_byte >= h.len() {
        return Ok(Value::False);
    }
    match h[start_byte..].windows(n.len()).position(|w| w == n) {
        Some(byte_pos) => {
            let abs_byte_pos = start_byte + byte_pos;
            let char_pos = chars.iter().position(|&p| p == abs_byte_pos).unwrap_or(abs_byte_pos);
            Ok(Value::Long(char_pos as i64))
        }
        None => Ok(Value::False),
    }
}

fn mb_strrpos(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let haystack = args.first().unwrap_or(&Value::Null).to_php_string();
    let needle = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let offset = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    let _ = validate_encoding(vm, "mb_strrpos", 4, "$encoding", args, 3)?;
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() {
        let chars: Vec<usize> = utf8_char_positions(h);
        let char_count = chars.len() as i64;
        let pos = if offset < 0 { char_count + offset } else { offset };
        if pos < 0 || pos > char_count {
            let msg = "mb_strrpos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)";
            let exc = vm.create_exception(b"ValueError", msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg.to_string(), line: vm.current_line });
        }
        // mb_strrpos with empty needle returns position right after end of haystack (char_count)
        return Ok(Value::Long(char_count));
    }

    let chars: Vec<usize> = utf8_char_positions(h);
    let char_count = chars.len() as i64;

    if offset < 0 {
        // For mb_strrpos, valid negative offset must satisfy
        // -char_count <= offset (i.e. abs(offset) <= char_count). The match's
        // start position is constrained to be <= char_count + offset.
        if offset.unsigned_abs() as i64 > char_count {
            let msg = "mb_strrpos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)";
            let exc = vm.create_exception(b"ValueError", msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg.to_string(), line: vm.current_line });
        }
        let max_start_char = (char_count + offset).max(0) as usize;
        let limit_byte = if max_start_char < chars.len() { chars[max_start_char] } else { h.len() };
        if h.len() < n.len() {
            return Ok(Value::False);
        }
        let mut found: Option<usize> = None;
        for (i, w) in h.windows(n.len()).enumerate().rev() {
            if i > limit_byte { continue; }
            if w == n {
                found = Some(i);
                break;
            }
        }
        match found {
            Some(byte_pos) => {
                let char_pos = chars.iter().position(|&p| p == byte_pos).unwrap_or(byte_pos);
                Ok(Value::Long(char_pos as i64))
            }
            None => Ok(Value::False),
        }
    } else {
        if offset > char_count {
            let msg = "mb_strrpos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)";
            let exc = vm.create_exception(b"ValueError", msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg.to_string(), line: vm.current_line });
        }
        let start_byte = if (offset as usize) < chars.len() { chars[offset as usize] } else { h.len() };
        if start_byte >= h.len() {
            return Ok(Value::False);
        }
        match h[start_byte..].windows(n.len()).rposition(|w| w == n) {
            Some(byte_pos) => {
                let abs_byte_pos = start_byte + byte_pos;
                let char_pos = chars.iter().position(|&p| p == abs_byte_pos).unwrap_or(abs_byte_pos);
                Ok(Value::Long(char_pos as i64))
            }
            None => Ok(Value::False),
        }
    }
}

fn mb_stripos(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let haystack = args.first().unwrap_or(&Value::Null).to_php_string();
    let needle = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let offset = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    let _ = validate_encoding(vm, "mb_stripos", 4, "$encoding", args, 3)?;

    // Case-insensitive: convert both to lowercase via Unicode
    let h_str = String::from_utf8_lossy(haystack.as_bytes()).to_lowercase();
    let n_str = String::from_utf8_lossy(needle.as_bytes()).to_lowercase();

    if n_str.is_empty() {
        let chars: Vec<usize> = utf8_char_positions(h_str.as_bytes());
        let char_count = chars.len() as i64;
        let pos = if offset < 0 { char_count + offset } else { offset };
        if pos < 0 || pos > char_count {
            let msg = "mb_stripos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)";
            let exc = vm.create_exception(b"ValueError", msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg.to_string(), line: vm.current_line });
        }
        return Ok(Value::Long(pos));
    }

    let h = h_str.as_bytes();
    let n = n_str.as_bytes();
    let chars = utf8_char_positions(h);
    let char_count = chars.len() as i64;

    let start_char = if offset < 0 {
        let from = char_count + offset;
        if from < 0 {
            let msg = "mb_stripos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)";
            let exc = vm.create_exception(b"ValueError", msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg.to_string(), line: vm.current_line });
        }
        from as usize
    } else {
        if offset > char_count {
            let msg = "mb_stripos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)";
            let exc = vm.create_exception(b"ValueError", msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg.to_string(), line: vm.current_line });
        }
        offset as usize
    };
    let _ = vm;

    let start_byte = if start_char < chars.len() { chars[start_char] } else { h.len() };
    if start_byte >= h.len() {
        return Ok(Value::False);
    }

    match h[start_byte..].windows(n.len()).position(|w| w == n) {
        Some(byte_pos) => {
            let abs = start_byte + byte_pos;
            let char_pos = chars.iter().position(|&p| p == abs).unwrap_or(abs);
            Ok(Value::Long(char_pos as i64))
        }
        None => Ok(Value::False),
    }
}

fn mb_strripos(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let haystack = args.first().unwrap_or(&Value::Null).to_php_string();
    let needle = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let offset = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    let _ = validate_encoding(vm, "mb_strripos", 4, "$encoding", args, 3)?;

    let h_str = String::from_utf8_lossy(haystack.as_bytes()).to_lowercase();
    let n_str = String::from_utf8_lossy(needle.as_bytes()).to_lowercase();

    if n_str.is_empty() {
        let chars: Vec<usize> = utf8_char_positions(h_str.as_bytes());
        let char_count = chars.len() as i64;
        let pos = if offset < 0 { char_count + offset } else { offset };
        if pos < 0 || pos > char_count {
            let msg = "mb_strripos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)";
            let exc = vm.create_exception(b"ValueError", msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg.to_string(), line: vm.current_line });
        }
        // Empty needle returns end-of-string position (char_count)
        return Ok(Value::Long(char_count));
    }

    let h = h_str.as_bytes();
    let n = n_str.as_bytes();
    let chars = utf8_char_positions(h);
    let char_count = chars.len() as i64;

    if offset < 0 {
        if offset.unsigned_abs() as i64 > char_count {
            let msg = "mb_strripos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)";
            let exc = vm.create_exception(b"ValueError", msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg.to_string(), line: vm.current_line });
        }
        let max_start_char = (char_count + offset).max(0) as usize;
        let limit_byte = if max_start_char < chars.len() { chars[max_start_char] } else { h.len() };
        if h.len() < n.len() {
            return Ok(Value::False);
        }
        let mut found: Option<usize> = None;
        for (i, w) in h.windows(n.len()).enumerate().rev() {
            if i > limit_byte { continue; }
            if w == n {
                found = Some(i);
                break;
            }
        }
        match found {
            Some(byte_pos) => {
                let char_pos = chars.iter().position(|&p| p == byte_pos).unwrap_or(byte_pos);
                Ok(Value::Long(char_pos as i64))
            }
            None => Ok(Value::False),
        }
    } else {
        if offset > char_count {
            let msg = "mb_strripos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)";
            let exc = vm.create_exception(b"ValueError", msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg.to_string(), line: vm.current_line });
        }
        let start_byte = if (offset as usize) < chars.len() { chars[offset as usize] } else { h.len() };
        if start_byte >= h.len() {
            return Ok(Value::False);
        }
        match h[start_byte..].windows(n.len()).rposition(|w| w == n) {
            Some(byte_pos) => {
                let abs = start_byte + byte_pos;
                let char_pos = chars.iter().position(|&p| p == abs).unwrap_or(abs);
                Ok(Value::Long(char_pos as i64))
            }
            None => Ok(Value::False),
        }
    }
}

fn mb_convert_encoding(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let input = args.first().unwrap_or(&Value::Null);
    let to_enc = args.get(1).map(|v| v.to_php_string().to_string_lossy()).unwrap_or_else(|| "UTF-8".to_string());

    // Validate target encoding ($to_encoding) — PHP throws a ValueError when
    // the destination encoding is unknown. "auto" and pseudo-encodings that
    // make sense as targets are allowed (e.g. Base64).
    if !is_known_encoding(&to_enc) {
        let msg = format!(
            "mb_convert_encoding(): Argument #2 ($to_encoding) must be a valid encoding, \"{}\" given",
            to_enc
        );
        let exc = _vm.create_exception(b"ValueError", &msg, 0);
        _vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: _vm.current_line });
    }

    // Validate from_encoding ($from_encoding) — PHP throws ValueError when any
    // listed encoding is unknown, with a "contains invalid encoding" message.
    if let Some(from_arg) = args.get(2) {
        let check_one = |enc: &str| -> bool { enc.eq_ignore_ascii_case("auto") || is_known_encoding(enc) };
        let empty_msg = "mb_convert_encoding(): Argument #3 ($from_encoding) must specify at least one encoding".to_string();
        match from_arg {
            Value::Null => {}
            Value::Array(arr) => {
                let arr = arr.borrow();
                if arr.len() == 0 {
                    let exc = _vm.create_exception(b"ValueError", &empty_msg, 0);
                    _vm.current_exception = Some(exc);
                    return Err(VmError { message: empty_msg, line: _vm.current_line });
                }
                for (_, v) in arr.iter() {
                    let s = v.to_php_string().to_string_lossy();
                    if !check_one(&s) {
                        let msg = format!(
                            "mb_convert_encoding(): Argument #3 ($from_encoding) contains invalid encoding \"{}\"",
                            s
                        );
                        let exc = _vm.create_exception(b"ValueError", &msg, 0);
                        _vm.current_exception = Some(exc);
                        return Err(VmError { message: msg, line: _vm.current_line });
                    }
                }
            }
            _ => {
                let s = from_arg.to_php_string().to_string_lossy();
                if s.is_empty() {
                    let exc = _vm.create_exception(b"ValueError", &empty_msg, 0);
                    _vm.current_exception = Some(exc);
                    return Err(VmError { message: empty_msg, line: _vm.current_line });
                }
                // Comma-separated list also accepted by PHP
                for part in s.split(',') {
                    let p = part.trim();
                    if !check_one(p) {
                        let msg = format!(
                            "mb_convert_encoding(): Argument #3 ($from_encoding) contains invalid encoding \"{}\"",
                            p
                        );
                        let exc = _vm.create_exception(b"ValueError", &msg, 0);
                        _vm.current_exception = Some(exc);
                        return Err(VmError { message: msg, line: _vm.current_line });
                    }
                }
            }
        }
    }

    let from = get_from_encoding(args);

    // Emit deprecation for the target encoding (PHP only emits for to_encoding,
    // because from_encoding is parsed via a different code path that does not
    // use the cache-aware php_mb_get_encoding helper). The cache key tracks
    // the last target encoding so successive calls with the same target don't
    // re-emit (matching PHP's last_used_encoding behaviour).
    {
        let to_lower = to_enc.to_ascii_lowercase();
        let prev = _vm.constants.get(b"__mbstring_last_target_encoding".as_ref())
            .map(|v| v.to_php_string().to_string_lossy())
            .unwrap_or_default();
        if to_lower != prev {
            if to_lower == "base64" {
                _vm.emit_deprecated("mb_convert_encoding(): Handling Base64 via mbstring is deprecated; use base64_encode/base64_decode instead");
            } else if to_lower == "html-entities" || to_lower == "html" {
                _vm.emit_deprecated("mb_convert_encoding(): Handling HTML entities via mbstring is deprecated; use htmlspecialchars, htmlentities, or mb_encode_numericentity/mb_decode_numericentity instead");
            } else if to_lower == "qprint" || to_lower == "quoted-printable" {
                _vm.emit_deprecated("mb_convert_encoding(): Handling QPrint via mbstring is deprecated; use quoted_printable_encode/quoted_printable_decode instead");
            } else if to_lower == "uuencode" {
                _vm.emit_deprecated("mb_convert_encoding(): Handling Uuencode via mbstring is deprecated; use convert_uuencode/convert_uudecode instead");
            }
            _vm.constants.insert(b"__mbstring_last_target_encoding".to_vec(),
                Value::String(PhpString::from_string(to_lower)));
        }
    }

    // Handle array input
    if let Value::Array(arr) = input {
        let arr = arr.borrow();
        let mut result = PhpArray::new();
        for (key, val) in arr.iter() {
            let s = val.to_php_string();
            if let Some(converted) = convert_encoding(s.as_bytes(), &to_enc, &from) {
                result.set(key.clone(), Value::String(PhpString::from_vec(converted)));
            } else {
                result.set(key.clone(), val.clone());
            }
        }
        return Ok(Value::Array(Rc::new(RefCell::new(result))));
    }

    let s = input.to_php_string();

    if let Some(converted) = convert_encoding(s.as_bytes(), &to_enc, &from) {
        Ok(Value::String(PhpString::from_vec(converted)))
    } else {
        Ok(Value::String(s))
    }
}

/// Extract from_encoding from mb_convert_encoding args (can be string or array)
fn get_from_encoding(args: &[Value]) -> String {
    if let Some(from_arg) = args.get(2) {
        match from_arg {
            Value::Array(arr) => {
                let arr = arr.borrow();
                // Use the first encoding from the array
                if let Some((_, v)) = arr.iter().next() {
                    return v.to_php_string().to_string_lossy();
                }
                "UTF-8".to_string()
            }
            Value::Null => "UTF-8".to_string(),
            _ => from_arg.to_php_string().to_string_lossy(),
        }
    } else {
        "UTF-8".to_string()
    }
}

fn mb_substitute_character(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() || matches!(args.first(), Some(Value::Null)) {
        // Return current substitute character (default: int 63 = '?')
        let prev = _vm.constants.get(b"__mb_substitute_character".as_ref())
            .cloned()
            .unwrap_or(Value::Long(63));
        return Ok(prev);
    }

    let arg = &args[0];
    let new_val = match arg {
        Value::String(s) => {
            let lower = s.to_string_lossy().to_ascii_lowercase();
            match lower.as_str() {
                "none" | "long" | "entity" => Value::String(PhpString::from_string(lower)),
                _ => {
                    let msg = "mb_substitute_character(): Argument #1 ($substitute_character) must be \"none\", \"long\", \"entity\" or a valid codepoint";
                    let exc = _vm.create_exception(b"ValueError", msg, 0);
                    _vm.current_exception = Some(exc);
                    return Err(VmError { message: msg.to_string(), line: _vm.current_line });
                }
            }
        }
        Value::Long(n) => {
            // Validate that the int is a valid codepoint
            let cp = *n;
            if cp < 0 || cp > 0x10FFFF || (0xD800..=0xDFFF).contains(&cp) {
                let msg = "mb_substitute_character(): Argument #1 ($substitute_character) must be \"none\", \"long\", \"entity\" or a valid codepoint";
                let exc = _vm.create_exception(b"ValueError", msg, 0);
                _vm.current_exception = Some(exc);
                return Err(VmError { message: msg.to_string(), line: _vm.current_line });
            }
            Value::Long(cp)
        }
        Value::False => Value::String(PhpString::from_bytes(b"none")),
        _ => Value::String(PhpString::from_bytes(b"none")),
    };
    _vm.constants.insert(b"__mb_substitute_character".to_vec(), new_val);
    Ok(Value::True)
}

fn check_encoding_bytes(bytes: &[u8], enc_lower: &str) -> bool {
    if enc_lower == "utf-8" || enc_lower == "utf8" {
        std::str::from_utf8(bytes).is_ok()
    } else if enc_lower == "ascii" || enc_lower == "us-ascii" {
        bytes.iter().all(|&b| b < 128)
    } else {
        // For other encodings, currently assume valid
        true
    }
}

fn check_encoding_value(v: &Value, enc_lower: &str, depth: usize) -> bool {
    if depth > 256 { return false; }
    match v {
        Value::Null | Value::True | Value::False | Value::Undef => true,
        Value::Long(_) | Value::Double(_) => true,
        Value::String(s) => check_encoding_bytes(s.as_bytes(), enc_lower),
        Value::Array(arr) => {
            let arr_b = arr.borrow();
            for (key, val) in arr_b.iter() {
                if let ArrayKey::String(s) = key {
                    if !check_encoding_bytes(s.as_bytes(), enc_lower) { return false; }
                }
                if !check_encoding_value(val, enc_lower, depth + 1) { return false; }
            }
            true
        }
        Value::Object(_) => true,
        _ => true,
    }
}

fn mb_check_encoding(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Validate encoding argument early (when present and not null/empty)
    if let Some(enc_arg) = args.get(1) {
        if !matches!(enc_arg, Value::Null) {
            let enc = enc_arg.to_php_string().to_string_lossy();
            let enc_lower = enc.to_ascii_lowercase();
            // Reject "pass" and similar non-real pseudo-encodings.
            let is_non_real = matches!(enc_lower.as_str(),
                "pass" | "auto" | "wchar" | "byte2be" | "byte2le" | "byte4be" | "byte4le");
            if !enc.is_empty() && (is_non_real || !is_known_encoding(&enc)) {
                let msg = format!(
                    "mb_check_encoding(): Argument #2 ($encoding) must be a valid encoding, \"{}\" given",
                    enc
                );
                let exc = _vm.create_exception(b"ValueError", &msg, 0);
                _vm.current_exception = Some(exc);
                return Err(VmError { message: msg, line: _vm.current_line });
            }
        }
    }
    if args.is_empty() {
        _vm.emit_deprecated("mb_check_encoding(): Calling mb_check_encoding() without argument is deprecated");
        return Ok(Value::True);
    }
    if matches!(args.first(), Some(Value::Null)) {
        return Ok(Value::True);
    }
    let encoding = args.get(1).and_then(|v| if matches!(v, Value::Null) { None } else { Some(v.to_php_string().to_string_lossy()) }).unwrap_or_else(|| "UTF-8".to_string());
    let enc_lower = encoding.to_ascii_lowercase();

    if enc_lower == "html-entities" || enc_lower == "html" {
        _vm.emit_deprecated("mb_check_encoding(): Handling HTML entities via mbstring is deprecated; use htmlspecialchars, htmlentities, or mb_encode_numericentity/mb_decode_numericentity instead");
    }

    let value = &args[0];
    let ok = check_encoding_value(value, &enc_lower, 0);
    Ok(if ok { Value::True } else { Value::False })
}

fn mb_substr_count(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let haystack = args.first().unwrap_or(&Value::Null).to_php_string();
    let needle = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let _ = validate_encoding(_vm, "mb_substr_count", 3, "$encoding", args, 2)?;
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() {
        // PHP raises ValueError on empty needle.
        let msg = "mb_substr_count(): Argument #2 ($needle) must not be empty";
        let exc = _vm.create_exception(b"ValueError", msg, 0);
        _vm.current_exception = Some(exc);
        return Err(VmError { message: msg.to_string(), line: _vm.current_line });
    }
    let mut count = 0;
    let mut pos = 0;
    while pos + n.len() <= h.len() {
        if &h[pos..pos + n.len()] == n {
            count += 1;
            pos += n.len();
        } else {
            pos += 1;
        }
    }
    Ok(Value::Long(count as i64))
}

fn mb_strstr_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let haystack = args.first().unwrap_or(&Value::Null).to_php_string();
    let needle = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let before_needle = args.get(2).map(|v| v.is_truthy()).unwrap_or(false);
    let _ = validate_encoding(_vm, "mb_strstr", 4, "$encoding", args, 3)?;
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() {
        // PHP 8.0+: empty needle returns haystack for before_needle=false, "" for true
        return Ok(if before_needle {
            Value::String(PhpString::empty())
        } else {
            Value::String(PhpString::from_vec(h.to_vec()))
        });
    }
    match h.windows(n.len()).position(|w| w == n) {
        Some(pos) => {
            if before_needle {
                Ok(Value::String(PhpString::from_vec(h[..pos].to_vec())))
            } else {
                Ok(Value::String(PhpString::from_vec(h[pos..].to_vec())))
            }
        }
        None => Ok(Value::False),
    }
}

fn mb_stristr_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let haystack = args.first().unwrap_or(&Value::Null).to_php_string();
    let needle = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let before_needle = args.get(2).map(|v| v.is_truthy()).unwrap_or(false);
    let _ = validate_encoding(_vm, "mb_stristr", 4, "$encoding", args, 3)?;
    let h = haystack.as_bytes();
    let h_lower: Vec<u8> = String::from_utf8_lossy(h).to_lowercase().into_bytes();
    let n_lower: Vec<u8> = String::from_utf8_lossy(needle.as_bytes()).to_lowercase().into_bytes();
    if n_lower.is_empty() {
        return Ok(if before_needle {
            Value::String(PhpString::empty())
        } else {
            Value::String(PhpString::from_vec(h.to_vec()))
        });
    }
    match h_lower.windows(n_lower.len()).position(|w| w == n_lower.as_slice()) {
        Some(pos) => {
            if before_needle {
                Ok(Value::String(PhpString::from_vec(h[..pos].to_vec())))
            } else {
                Ok(Value::String(PhpString::from_vec(h[pos..].to_vec())))
            }
        }
        None => Ok(Value::False),
    }
}

fn mb_strrchr_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let haystack = args.first().unwrap_or(&Value::Null).to_php_string();
    let needle = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let before_needle = args.get(2).map(|v| v.is_truthy()).unwrap_or(false);
    let _ = validate_encoding(_vm, "mb_strrchr", 4, "$encoding", args, 3)?;
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() {
        // mb_strrchr with empty needle: returns "" for normal, haystack for before_needle=true
        return Ok(if before_needle {
            Value::String(PhpString::from_vec(h.to_vec()))
        } else {
            Value::String(PhpString::empty())
        });
    }
    // mb_strrchr searches for the full needle string (rposition)
    let mut last_pos: Option<usize> = None;
    if h.len() >= n.len() {
        for i in (0..=h.len() - n.len()).rev() {
            if &h[i..i + n.len()] == n {
                last_pos = Some(i);
                break;
            }
        }
    }
    match last_pos {
        Some(pos) => {
            if before_needle {
                Ok(Value::String(PhpString::from_vec(h[..pos].to_vec())))
            } else {
                Ok(Value::String(PhpString::from_vec(h[pos..].to_vec())))
            }
        }
        None => Ok(Value::False),
    }
}

fn mb_strrichr_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let haystack = args.first().unwrap_or(&Value::Null).to_php_string();
    let needle = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let before_needle = args.get(2).map(|v| v.is_truthy()).unwrap_or(false);
    let _ = validate_encoding(_vm, "mb_strrichr", 4, "$encoding", args, 3)?;
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() {
        return Ok(if before_needle {
            Value::String(PhpString::from_vec(h.to_vec()))
        } else {
            Value::String(PhpString::empty())
        });
    }

    // Case-insensitive search for full needle - lowercase both, search in lowered haystack.
    // We need to map back the byte offset to the original haystack though. For
    // UTF-8 where the lowered version preserves byte length most of the time
    // (Greek/Cyrillic case mappings preserve byte length), we use the lowered
    // bytes directly as offsets.
    let h_str = String::from_utf8_lossy(h);
    let n_str = String::from_utf8_lossy(n);
    let h_lower = h_str.to_lowercase();
    let n_lower = n_str.to_lowercase();

    // Special-case: if lower-cased haystack has the same byte length as original,
    // we can use those byte offsets directly.
    if h_lower.len() == h.len() {
        let h_lb = h_lower.as_bytes();
        let nl = n_lower.as_bytes();
        if h_lb.len() < nl.len() { return Ok(Value::False); }
        for i in (0..=h_lb.len() - nl.len()).rev() {
            if &h_lb[i..i + nl.len()] == nl {
                if before_needle {
                    return Ok(Value::String(PhpString::from_vec(h[..i].to_vec())));
                } else {
                    return Ok(Value::String(PhpString::from_vec(h[i..].to_vec())));
                }
            }
        }
        return Ok(Value::False);
    }

    // Fallback: walk character by character and lowercase compare.
    let h_chars: Vec<(usize, char)> = h_str.char_indices().collect();
    let n_chars: Vec<char> = n_lower.chars().collect();
    if n_chars.is_empty() { return Ok(Value::False); }
    let mut found: Option<usize> = None;
    for start in (0..h_chars.len()).rev() {
        let mut ok = true;
        for (k, &nc) in n_chars.iter().enumerate() {
            let idx = start + k;
            if idx >= h_chars.len() { ok = false; break; }
            let hc_lower: String = h_chars[idx].1.to_lowercase().collect();
            let nc_lower: String = nc.to_lowercase().collect();
            if hc_lower != nc_lower { ok = false; break; }
        }
        if ok {
            found = Some(h_chars[start].0);
            break;
        }
    }
    match found {
        Some(pos) => {
            if before_needle {
                Ok(Value::String(PhpString::from_vec(h[..pos].to_vec())))
            } else {
                Ok(Value::String(PhpString::from_vec(h[pos..].to_vec())))
            }
        }
        None => Ok(Value::False),
    }
}

fn mb_str_split_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let split_length = args.get(1).map(|v| v.to_long()).unwrap_or(1);
    if split_length < 1 {
        let msg = "mb_str_split(): Argument #2 ($length) must be greater than 0";
        let exc = vm.create_exception(b"ValueError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg.to_string(), line: vm.current_line });
    }
    // Encoding validation
    let mut encoding_name = String::from("UTF-8");
    if let Some(enc_arg) = args.get(2) {
        if !matches!(enc_arg, Value::Null) {
            let enc = enc_arg.to_php_string().to_string_lossy();
            if !enc.is_empty() && resolve_encoding(&enc).is_none() && !is_pseudo_encoding(&enc) {
                let msg = format!("mb_str_split(): Argument #3 ($encoding) must be a valid encoding, \"{}\" given", enc);
                let exc = vm.create_exception(b"ValueError", &msg, 0);
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg, line: vm.current_line });
            }
            if !enc.is_empty() {
                encoding_name = enc;
            }
        }
    }
    let split_length = split_length as usize;
    let bytes = s.as_bytes();
    let enc_lower = encoding_name.to_ascii_lowercase();
    let mut result = PhpArray::new();

    if enc_lower == "utf-8" || enc_lower == "utf8" || enc_lower == "ascii" {
        let chars: Vec<&[u8]> = utf8_chars(bytes);
        for chunk in chars.chunks(split_length) {
            let mut s = Vec::new();
            for c in chunk { s.extend_from_slice(c); }
            result.push(Value::String(PhpString::from_vec(s)));
        }
    } else {
        let utf8_str: String = if let Some(enc) = resolve_encoding(&enc_lower) {
            let (cow, _, _) = enc.decode(bytes);
            cow.to_string()
        } else {
            String::from_utf8_lossy(bytes).to_string()
        };
        let chars: Vec<char> = utf8_str.chars().collect();
        for chunk in chars.chunks(split_length) {
            let s: String = chunk.iter().collect();
            let encoded: Vec<u8> = if let Some(enc) = resolve_encoding(&enc_lower) {
                let (cow, _, _) = enc.encode(&s);
                cow.to_vec()
            } else {
                s.into_bytes()
            };
            result.push(Value::String(PhpString::from_vec(encoded)));
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn is_pseudo_encoding(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    matches!(lower.as_str(),
        "auto" | "pass" | "wchar" | "byte2be" | "byte2le" | "byte4be" | "byte4le"
        | "ucs-4" | "ucs4" | "ucs-4be" | "ucs4be" | "ucs-4le" | "ucs4le"
        | "ucs-2" | "ucs2" | "ucs-2be" | "ucs2be" | "ucs-2le" | "ucs2le"
        | "utf-32" | "utf32" | "utf-32be" | "utf32be" | "utf-32le" | "utf32le"
        | "utf-7" | "utf7" | "utf7-imap" | "utf-7-imap"
        | "7bit" | "8bit" | "binary"
        | "base64" | "uuencode" | "html-entities" | "html" | "quoted-printable" | "qprint"
        | "armscii-8" | "armscii8" | "cp866"
        | "hz" | "jis" | "iso-2022-jp" | "iso-2022-jp-ms" | "iso-2022-jp-2004" | "iso-2022-kr"
        | "iso-2022-jp-kddi" | "iso-2022-jp-mobile#kddi"
        | "eucjp-win" | "sjis-mac" | "sjis-mobile#docomo" | "sjis-mobile#kddi" | "sjis-mobile#softbank"
        | "utf8-mobile#docomo" | "utf8-mobile#kddi-a" | "utf8-mobile#kddi-b" | "utf8-mobile#softbank"
        | "cp50220" | "cp50221" | "cp50222" | "cp51932"
        | "cp936" | "macjapanese" | "x-mac-japanese"
        | "euc-jp-2004" | "sjis-2004" | "euc-jp-ms"
        | "gb18030" | "gb18030-2022" | "uhc"
    )
}

/// Returns true when the encoding name is recognized either by encoding_rs
/// or as a pseudo-encoding supported by PHP's mbstring.
fn is_known_encoding(name: &str) -> bool {
    if name.is_empty() { return false; }
    if is_pseudo_encoding(name) { return true; }
    resolve_encoding(name).is_some()
}

/// Validate an encoding argument and throw a ValueError when it's not
/// recognized. Returns Ok(Some(encoding_string)) when the argument was
/// present and non-null, Ok(None) when the arg was null or absent,
/// Err on validation failure.
fn validate_encoding(
    vm: &mut Vm,
    fn_name: &str,
    arg_num: usize,
    arg_name: &str,
    args: &[Value],
    idx: usize,
) -> Result<Option<String>, VmError> {
    let Some(arg) = args.get(idx) else { return Ok(None); };
    if matches!(arg, Value::Null) { return Ok(None); }
    let enc = arg.to_php_string().to_string_lossy();
    if enc.is_empty() {
        // Empty string is treated as "use default" by some functions but
        // most throw a ValueError. PHP 8.4+ throws for an empty string too.
        let msg = format!(
            "{}(): Argument #{} ({}) must be a valid encoding, \"\" given",
            fn_name, arg_num, arg_name
        );
        let exc = vm.create_exception(b"ValueError", &msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    if !is_known_encoding(&enc) {
        let msg = format!(
            "{}(): Argument #{} ({}) must be a valid encoding, \"{}\" given",
            fn_name, arg_num, arg_name, enc
        );
        let exc = vm.create_exception(b"ValueError", &msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    Ok(Some(enc))
}

fn mb_convert_case_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let mode = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let _ = validate_encoding(_vm, "mb_convert_case", 3, "$encoding", args, 2)?;
    // Validate mode argument
    if !(0..=6).contains(&mode) {
        let msg = format!("mb_convert_case(): Argument #2 ($mode) must be one of the MB_CASE_* constants");
        let exc = _vm.create_exception(b"ValueError", &msg, 0);
        _vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: _vm.current_line });
    }
    let bytes = s.as_bytes();

    if let Ok(s) = std::str::from_utf8(bytes) {
        let result = match mode {
            0 | 3 => php_uppercase(s), // MB_CASE_UPPER / MB_CASE_UPPER_SIMPLE
            1 | 4 | 5 => php_lowercase(s), // MB_CASE_LOWER / MB_CASE_LOWER_SIMPLE / MB_CASE_FOLD_SIMPLE
            2 => {
                // MB_CASE_TITLE - capitalize first letter of each word.
                let mut result = String::new();
                let mut cap_next = true;
                for c in s.chars() {
                    if cap_next && c.is_alphabetic() {
                        if let Some(tc) = to_titlecase_char(c) {
                            result.push(tc);
                        } else {
                            for uc in c.to_uppercase() {
                                result.push(uc);
                            }
                        }
                        cap_next = false;
                    } else {
                        for lc in c.to_lowercase() {
                            result.push(lc);
                        }
                        // Case-ignorable characters (apostrophes, modifier
                        // letters, combining marks) don't break the word.
                        if !c.is_alphabetic() && !is_case_ignorable(c) {
                            cap_next = true;
                        }
                    }
                }
                result
            }
            _ => s.to_string(),
        };
        Ok(Value::String(PhpString::from_vec(result.into_bytes())))
    } else {
        // Fallback for non-UTF-8
        let result: Vec<u8> = match mode {
            0 => bytes.iter().map(|b| b.to_ascii_uppercase()).collect(),
            1 => bytes.iter().map(|b| b.to_ascii_lowercase()).collect(),
            _ => bytes.to_vec(),
        };
        Ok(Value::String(PhpString::from_vec(result)))
    }
}

/// Canonical mb_language name from any accepted alias.
/// Returns None for unknown languages.
fn canonical_mb_language(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        "japanese" | "ja" => Some("Japanese"),
        "english" | "en" => Some("English"),
        "uni" => Some("uni"),
        "neutral" => Some("neutral"),
        "russian" | "ru" => Some("Russian"),
        "german" | "de" => Some("German"),
        "armenian" | "hy" => Some("Armenian"),
        "turkish" | "tr" => Some("Turkish"),
        "simplified chinese" | "zh-cn" | "chinese-simplified" => Some("Simplified Chinese"),
        "traditional chinese" | "zh-tw" | "chinese-traditional" => Some("Traditional Chinese"),
        "korean" | "ko" => Some("Korean"),
        _ => None,
    }
}

fn mb_language_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Read current language from constants; fall back to "neutral"
    let current_lang = _vm.constants.get(b"mbstring.language".as_ref())
        .map(|v| v.to_php_string().to_string_lossy())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "neutral".to_string());

    if args.is_empty() || matches!(args.first(), Some(Value::Null)) {
        // Always return canonical form
        let canon = canonical_mb_language(&current_lang).unwrap_or("neutral");
        return Ok(Value::String(PhpString::from_string(canon.to_string())));
    }

    let new_lang = args[0].to_php_string().to_string_lossy();
    match canonical_mb_language(&new_lang) {
        Some(canon) => {
            _vm.constants.insert(b"mbstring.language".to_vec(),
                Value::String(PhpString::from_string(canon.to_string())));
            Ok(Value::True)
        }
        None => {
            let msg = format!("mb_language(): Argument #1 ($language) must be a valid language, \"{}\" given", new_lang);
            let exc = _vm.create_exception(b"ValueError", &msg, 0);
            _vm.current_exception = Some(exc);
            Err(VmError { message: msg, line: _vm.current_line })
        }
    }
}

fn mb_list_encodings_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut result = PhpArray::new();
    let encodings = [
        "BASE64", "UUENCODE", "HTML-ENTITIES", "Quoted-Printable",
        "7bit", "8bit", "pass",
        "UTF-8", "UTF-7", "UTF-16", "UTF-16BE", "UTF-16LE",
        "UTF-32", "UTF-32BE", "UTF-32LE",
        "ASCII",
        "EUC-JP", "SJIS", "eucJP-win", "SJIS-win", "JIS", "ISO-2022-JP", "ISO-2022-JP-MS",
        "CP932",
        "EUC-CN", "HZ", "EUC-TW", "CP950",
        "BIG-5",
        "EUC-KR", "UHC", "ISO-2022-KR",
        "Windows-1251", "Windows-1252", "CP866",
        "KOI8-R", "KOI8-U",
        "ArmSCII-8",
        "ISO-8859-1", "ISO-8859-2", "ISO-8859-3", "ISO-8859-4", "ISO-8859-5",
        "ISO-8859-6", "ISO-8859-7", "ISO-8859-8", "ISO-8859-9", "ISO-8859-10",
        "ISO-8859-13", "ISO-8859-14", "ISO-8859-15", "ISO-8859-16",
    ];
    for enc in &encodings {
        result.push(Value::String(PhpString::from_bytes(enc.as_bytes())));
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn mb_encoding_aliases_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let enc = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy();
    let enc_lower = enc.to_ascii_lowercase();
    let mut result = PhpArray::new();
    match enc_lower.as_str() {
        "utf-8" | "utf8" => {
            result.push(Value::String(PhpString::from_bytes(b"utf8")));
        }
        "ascii" | "us-ascii" => {
            result.push(Value::String(PhpString::from_bytes(b"ANSI_X3.4-1968")));
            result.push(Value::String(PhpString::from_bytes(b"ANSI_X3.4-1986")));
            result.push(Value::String(PhpString::from_bytes(b"IBM-367")));
            result.push(Value::String(PhpString::from_bytes(b"IBM367")));
            result.push(Value::String(PhpString::from_bytes(b"ISO646-US")));
            result.push(Value::String(PhpString::from_bytes(b"ISO_646.irv:1991")));
            result.push(Value::String(PhpString::from_bytes(b"US-ASCII")));
            result.push(Value::String(PhpString::from_bytes(b"cp367")));
            result.push(Value::String(PhpString::from_bytes(b"csASCII")));
            result.push(Value::String(PhpString::from_bytes(b"iso-ir-6")));
            result.push(Value::String(PhpString::from_bytes(b"us")));
        }
        "iso-8859-1" | "iso8859-1" | "latin1" => {
            result.push(Value::String(PhpString::from_bytes(b"ISO_8859-1")));
            result.push(Value::String(PhpString::from_bytes(b"latin1")));
        }
        "7bit" => {}
        "8bit" => {
            result.push(Value::String(PhpString::from_bytes(b"binary")));
        }
        "binary" => {
            result.push(Value::String(PhpString::from_bytes(b"8bit")));
        }
        _ => {
            if resolve_encoding(&enc_lower).is_none() && !is_pseudo_encoding(&enc_lower) {
                let msg = format!("mb_encoding_aliases(): Argument #1 ($encoding) must be a valid encoding, \"{}\" given", enc);
                let exc = vm.create_exception(b"ValueError", &msg, 0);
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg, line: vm.current_line });
            }
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn mb_ord_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    // Reject "pass"/pseudo encodings explicitly (mirrors mb_chr behavior).
    if let Some(arg) = args.get(1) {
        if !matches!(arg, Value::Null) {
            let raw = arg.to_php_string().to_string_lossy();
            let lower = raw.to_ascii_lowercase();
            if matches!(lower.as_str(), "pass" | "auto" | "wchar" | "byte2be" | "byte2le" | "byte4be" | "byte4le") {
                let msg = format!(
                    "mb_ord(): Argument #2 ($encoding) must be a valid encoding, \"{}\" given",
                    raw
                );
                let exc = _vm.create_exception(b"ValueError", &msg, 0);
                _vm.current_exception = Some(exc);
                return Err(VmError { message: msg, line: _vm.current_line });
            }
        }
    }
    let enc_opt = validate_encoding(_vm, "mb_ord", 2, "$encoding", args, 1)?;
    let default_enc = current_internal_encoding(_vm);
    let enc_str: String = enc_opt.unwrap_or(default_enc);
    if let Some(unsupported) = mb_chr_unsupported(&enc_str) {
        let msg = format!("mb_ord() does not support the \"{}\" encoding", unsupported);
        let exc = _vm.create_exception(b"ValueError", &msg, 0);
        _vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: _vm.current_line });
    }
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        let msg = "mb_ord(): Argument #1 ($string) must not be empty";
        let exc = _vm.create_exception(b"ValueError", msg, 0);
        _vm.current_exception = Some(exc);
        return Err(VmError { message: msg.to_string(), line: _vm.current_line });
    }
    let enc_lower = enc_str.to_ascii_lowercase();
    if enc_lower == "utf-8" || enc_lower == "utf8" || enc_lower == "ascii" {
        if let Ok(sv) = std::str::from_utf8(bytes) {
            if let Some(c) = sv.chars().next() {
                return Ok(Value::Long(c as i64));
            }
        }
        return Ok(Value::False);
    }
    // For other encodings, convert to UTF-8 then take the first codepoint
    if let Some(converted) = convert_encoding(bytes, "UTF-8", &enc_str) {
        if let Ok(sv) = std::str::from_utf8(&converted) {
            if let Some(c) = sv.chars().next() {
                return Ok(Value::Long(c as i64));
            }
        }
    }
    Ok(Value::False)
}

/// Encodings that cannot losslessly encode arbitrary Unicode codepoints to
/// be returned from mb_chr (matches PHP behavior).
fn mb_chr_unsupported(enc: &str) -> Option<&'static str> {
    let lower = enc.to_ascii_lowercase();
    match lower.as_str() {
        "jis" | "iso-2022-jp" => Some("JIS"),
        "cp50220" => Some("CP50220"),
        "cp50221" => Some("CP50221"),
        "cp50222" => Some("CP50222"),
        "utf-7" | "utf7" => Some("UTF-7"),
        "utf7-imap" | "utf-7-imap" => Some("UTF7-IMAP"),
        _ => None,
    }
}

fn mb_chr_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let cp = args.first().map(|v| v.to_long()).unwrap_or(0);
    // For mb_chr, the "pass" pseudo-encoding and similar non-real encodings
    // should be rejected as invalid (PHP semantics).
    if let Some(arg) = args.get(1) {
        if !matches!(arg, Value::Null) {
            let raw = arg.to_php_string().to_string_lossy();
            let lower = raw.to_ascii_lowercase();
            if matches!(lower.as_str(), "pass" | "auto" | "wchar" | "byte2be" | "byte2le" | "byte4be" | "byte4le") {
                let msg = format!(
                    "mb_chr(): Argument #2 ($encoding) must be a valid encoding, \"{}\" given",
                    raw
                );
                let exc = _vm.create_exception(b"ValueError", &msg, 0);
                _vm.current_exception = Some(exc);
                return Err(VmError { message: msg, line: _vm.current_line });
            }
        }
    }
    let enc = validate_encoding(_vm, "mb_chr", 2, "$encoding", args, 1)?;
    let default_enc = current_internal_encoding(_vm);
    let enc_str = enc.as_deref().unwrap_or(&default_enc);
    if let Some(unsupported) = mb_chr_unsupported(enc_str) {
        let msg = format!("mb_chr() does not support the \"{}\" encoding", unsupported);
        let exc = _vm.create_exception(b"ValueError", &msg, 0);
        _vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: _vm.current_line });
    }
    // Reject surrogate codepoints
    if (0xD800..=0xDFFF).contains(&cp) {
        return Ok(Value::False);
    }
    if cp < 0 || cp > 0x10FFFF {
        return Ok(Value::False);
    }
    if let Some(c) = char::from_u32(cp as u32) {
        // For UTF-8 (default), straightforward
        let lower = enc_str.to_ascii_lowercase();
        if lower == "utf-8" || lower == "utf8" || lower == "ascii" {
            let mut buf = [0u8; 4];
            let s = c.encode_utf8(&mut buf);
            return Ok(Value::String(PhpString::from_bytes(s.as_bytes())));
        }
        // For other encodings, attempt conversion. The result must round-trip;
        // we return false if it can't be represented (mb_chr semantics).
        let utf8 = c.to_string();
        if let Some(converted) = convert_encoding(utf8.as_bytes(), enc_str, "UTF-8") {
            // Validate by round-tripping
            if let Some(roundtrip) = convert_encoding(&converted, "UTF-8", enc_str) {
                if roundtrip == utf8.as_bytes() {
                    return Ok(Value::String(PhpString::from_vec(converted)));
                }
            }
        }
        return Ok(Value::False);
    }
    Ok(Value::False)
}

fn mb_strcut_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let start = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let length = args.get(2).and_then(|v| if matches!(v, Value::Null) { None } else { Some(v.to_long()) });
    let _ = validate_encoding(_vm, "mb_strcut", 4, "$encoding", args, 3)?;
    let bytes = s.as_bytes();
    let len = bytes.len() as i64;

    let start_byte = if start < 0 {
        (len + start).max(0) as usize
    } else {
        start.min(len) as usize
    };

    // Adjust start to UTF-8 character boundary
    let start_byte = {
        let mut sb = start_byte;
        while sb > 0 && sb < bytes.len() && (bytes[sb] & 0xC0) == 0x80 {
            sb -= 1;
        }
        sb
    };

    let end_byte = match length {
        Some(l) if l < 0 => {
            let e = (len + l).max(start_byte as i64) as usize;
            let mut eb = e;
            while eb > start_byte && eb < bytes.len() && (bytes[eb] & 0xC0) == 0x80 {
                eb -= 1;
            }
            eb
        }
        Some(l) => {
            let e = (start_byte as i64 + l).min(len) as usize;
            let mut eb = e;
            while eb > start_byte && eb < bytes.len() && (bytes[eb] & 0xC0) == 0x80 {
                eb -= 1;
            }
            eb
        }
        None => bytes.len(),
    };

    if start_byte >= end_byte || start_byte >= bytes.len() {
        return Ok(Value::String(PhpString::empty()));
    }

    Ok(Value::String(PhpString::from_vec(bytes[start_byte..end_byte].to_vec())))
}

fn mb_detect_order_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() || matches!(args.first(), Some(Value::Null)) {
        let mut result = PhpArray::new();
        result.push(Value::String(PhpString::from_bytes(b"ASCII")));
        result.push(Value::String(PhpString::from_bytes(b"UTF-8")));
        Ok(Value::Array(Rc::new(RefCell::new(result))))
    } else {
        Ok(Value::True)
    }
}

fn mb_get_info_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let info_type = args.first().map(|v| v.to_php_string()).unwrap_or_else(|| PhpString::from_bytes(b"all"));
    let info_lower: Vec<u8> = info_type.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();

    match info_lower.as_slice() {
        b"internal_encoding" => Ok(Value::String(PhpString::from_bytes(b"UTF-8"))),
        b"http_input" => Ok(Value::Null),
        b"http_output" => Ok(Value::String(PhpString::from_bytes(b"UTF-8"))),
        b"http_output_conv_mimetypes" => Ok(Value::String(PhpString::from_bytes(b"^(text/|application/xhtml\\+xml)"))),
        b"mail_charset" => Ok(Value::String(PhpString::from_bytes(b"UTF-8"))),
        b"mail_header_encoding" => Ok(Value::String(PhpString::from_bytes(b"BASE64"))),
        b"mail_body_encoding" => Ok(Value::String(PhpString::from_bytes(b"BASE64"))),
        b"illegal_chars" => Ok(Value::Long(0)),
        b"encoding_translation" => Ok(Value::String(PhpString::from_bytes(b"Off"))),
        b"language" => Ok(Value::String(PhpString::from_bytes(b"neutral"))),
        b"substitute_character" => Ok(Value::Long(63)),
        b"strict_detection" => Ok(Value::String(PhpString::from_bytes(b"Off"))),
        b"detect_order" | b"all" => {
            let mut arr = PhpArray::new();
            if info_lower.as_slice() == b"detect_order" {
                arr.set(ArrayKey::Int(0), Value::String(PhpString::from_bytes(b"ASCII")));
                arr.set(ArrayKey::Int(1), Value::String(PhpString::from_bytes(b"UTF-8")));
                return Ok(Value::Array(Rc::new(RefCell::new(arr))));
            }
            arr.set(ArrayKey::String(PhpString::from_bytes(b"internal_encoding")), Value::String(PhpString::from_bytes(b"UTF-8")));
            arr.set(ArrayKey::String(PhpString::from_bytes(b"http_output")), Value::String(PhpString::from_bytes(b"UTF-8")));
            arr.set(ArrayKey::String(PhpString::from_bytes(b"http_output_conv_mimetypes")), Value::String(PhpString::from_bytes(b"^(text/|application/xhtml\\+xml)")));
            arr.set(ArrayKey::String(PhpString::from_bytes(b"mail_charset")), Value::String(PhpString::from_bytes(b"UTF-8")));
            arr.set(ArrayKey::String(PhpString::from_bytes(b"mail_header_encoding")), Value::String(PhpString::from_bytes(b"BASE64")));
            arr.set(ArrayKey::String(PhpString::from_bytes(b"mail_body_encoding")), Value::String(PhpString::from_bytes(b"BASE64")));
            arr.set(ArrayKey::String(PhpString::from_bytes(b"illegal_chars")), Value::Long(0));
            arr.set(ArrayKey::String(PhpString::from_bytes(b"encoding_translation")), Value::String(PhpString::from_bytes(b"Off")));
            arr.set(ArrayKey::String(PhpString::from_bytes(b"language")), Value::String(PhpString::from_bytes(b"neutral")));
            let mut detect_order = PhpArray::new();
            detect_order.set(ArrayKey::Int(0), Value::String(PhpString::from_bytes(b"ASCII")));
            detect_order.set(ArrayKey::Int(1), Value::String(PhpString::from_bytes(b"UTF-8")));
            arr.set(ArrayKey::String(PhpString::from_bytes(b"detect_order")), Value::Array(Rc::new(RefCell::new(detect_order))));
            arr.set(ArrayKey::String(PhpString::from_bytes(b"substitute_character")), Value::Long(63));
            arr.set(ArrayKey::String(PhpString::from_bytes(b"strict_detection")), Value::String(PhpString::from_bytes(b"Off")));
            Ok(Value::Array(Rc::new(RefCell::new(arr))))
        }
        _ => {
            _vm.emit_warning("mb_get_info(): argument #1 ($type) must be a valid type");
            Ok(Value::False)
        },
    }
}

fn mb_regex_encoding_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() || matches!(args.first(), Some(Value::Null)) {
        return Ok(Value::String(PhpString::from_bytes(b"UTF-8")));
    }
    let enc = args[0].to_php_string().to_string_lossy();
    if !is_known_encoding(&enc) {
        let msg = format!(
            "mb_regex_encoding(): Argument #1 ($encoding) must be a valid encoding, \"{}\" given",
            enc
        );
        let exc = _vm.create_exception(b"ValueError", &msg, 0);
        _vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: _vm.current_line });
    }
    Ok(Value::True)
}

fn mb_http_output_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() || matches!(args.first(), Some(Value::Null)) {
        return Ok(Value::String(PhpString::from_bytes(b"UTF-8")));
    }
    let enc = args[0].to_php_string().to_string_lossy();
    if !is_known_encoding(&enc) {
        let msg = format!(
            "mb_http_output(): Argument #1 ($encoding) must be a valid encoding, \"{}\" given",
            enc
        );
        let exc = _vm.create_exception(b"ValueError", &msg, 0);
        _vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: _vm.current_line });
    }
    Ok(Value::True)
}

fn mb_preferred_mime_name_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let encoding = args.first().unwrap_or(&Value::Null).to_php_string();
    let enc_lower = encoding.to_string_lossy().to_ascii_lowercase();
    match enc_lower.as_str() {
        "utf-8" | "utf8" => Ok(Value::String(PhpString::from_bytes(b"UTF-8"))),
        "iso-8859-1" | "latin1" => Ok(Value::String(PhpString::from_bytes(b"ISO-8859-1"))),
        "ascii" | "us-ascii" => Ok(Value::String(PhpString::from_bytes(b"US-ASCII"))),
        "shift_jis" | "sjis" => Ok(Value::String(PhpString::from_bytes(b"Shift_JIS"))),
        "euc-jp" | "eucjp" => Ok(Value::String(PhpString::from_bytes(b"EUC-JP"))),
        "iso-2022-jp" => Ok(Value::String(PhpString::from_bytes(b"ISO-2022-JP"))),
        "utf-16" | "utf-16be" | "utf-16le" => Ok(Value::String(PhpString::from_bytes(b"UTF-16"))),
        "windows-1252" | "cp1252" => Ok(Value::String(PhpString::from_bytes(b"Windows-1252"))),
        _ => {
            if resolve_encoding(&enc_lower).is_some() {
                Ok(Value::String(encoding))
            } else {
                vm.emit_warning(&format!("mb_preferred_mime_name(): Unknown encoding \"{}\"", encoding.to_string_lossy()));
                Ok(Value::False)
            }
        }
    }
}

fn mb_output_handler_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let content = args.first().unwrap_or(&Value::Null).to_php_string();
    Ok(Value::String(content))
}

fn mb_str_pad_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let length = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    // Default pad_string is " " when arg is absent OR explicitly null
    // (Null can arise from named arguments skipping this position.)
    let pad_string = match args.get(2) {
        None | Some(Value::Null) => PhpString::from_bytes(b" "),
        Some(v) => v.to_php_string(),
    };
    let pad_type = match args.get(3) {
        None | Some(Value::Null) => 1, // STR_PAD_RIGHT
        Some(v) => v.to_long(),
    };
    let enc_opt = validate_encoding(vm, "mb_str_pad", 5, "$encoding", args, 4)?;
    let default_enc = current_internal_encoding(vm);
    let enc_str: String = enc_opt.unwrap_or(default_enc);
    let enc_lower = enc_str.to_ascii_lowercase();

    // Validate pad_type
    if !(0..=2).contains(&pad_type) {
        let msg = "mb_str_pad(): Argument #4 ($pad_type) must be STR_PAD_LEFT, STR_PAD_RIGHT, or STR_PAD_BOTH";
        let exc = vm.create_exception(b"ValueError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg.to_string(), line: vm.current_line });
    }

    let s_bytes = s.as_bytes();

    // Compute char count in the target encoding by converting input to UTF-8 first.
    let s_utf8: String = if enc_lower == "utf-8" || enc_lower == "utf8" || enc_lower == "ascii" {
        String::from_utf8_lossy(s_bytes).into_owned()
    } else if let Some(converted) = convert_encoding(s_bytes, "UTF-8", &enc_str) {
        String::from_utf8_lossy(&converted).into_owned()
    } else {
        String::from_utf8_lossy(s_bytes).into_owned()
    };
    let char_count = s_utf8.chars().count();

    // If length is non-positive or string is already long enough, return original
    if length <= 0 || (length as usize) <= char_count {
        return Ok(Value::String(s));
    }

    // Pad string interpretation: convert from the target encoding to UTF-8
    // and count characters. If the pad bytes are not a valid sequence in
    // the encoding, it's effectively empty.
    let pad_utf8: String = if enc_lower == "utf-8" || enc_lower == "utf8" || enc_lower == "ascii" {
        // Treat input as already UTF-8 / ASCII.
        if std::str::from_utf8(pad_string.as_bytes()).is_ok() {
            String::from_utf8_lossy(pad_string.as_bytes()).into_owned()
        } else {
            String::new()
        }
    } else if let Some(converted) = convert_encoding(pad_string.as_bytes(), "UTF-8", &enc_str) {
        // Round-trip to verify the pad bytes are actually valid in this encoding.
        if let Some(roundtrip) = convert_encoding(&converted, &enc_str, "UTF-8") {
            if roundtrip == pad_string.as_bytes() {
                String::from_utf8_lossy(&converted).into_owned()
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    } else {
        String::new()
    };
    let pad_chars: Vec<char> = pad_utf8.chars().collect();

    // Pad string must not be empty (after encoding-aware interpretation)
    if pad_chars.is_empty() {
        let msg = "mb_str_pad(): Argument #3 ($pad_string) must not be empty";
        let exc = vm.create_exception(b"ValueError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg.to_string(), line: vm.current_line });
    }

    let pad_len = length as usize - char_count;
    let mut pad_utf8_out = String::new();
    for i in 0..pad_len {
        pad_utf8_out.push(pad_chars[i % pad_chars.len()]);
    }

    // For UTF-8 / ASCII / unknown, work directly with bytes.
    if enc_lower == "utf-8" || enc_lower == "utf8" || enc_lower == "ascii" {
        let result = match pad_type {
            0 => format!("{}{}", pad_utf8_out, String::from_utf8_lossy(s_bytes)),
            2 => {
                let left = pad_len / 2;
                let right = pad_len - left;
                let mut lp = String::new();
                for i in 0..left { lp.push(pad_chars[i % pad_chars.len()]); }
                let mut rp = String::new();
                for i in 0..right { rp.push(pad_chars[i % pad_chars.len()]); }
                format!("{}{}{}", lp, String::from_utf8_lossy(s_bytes), rp)
            }
            _ => format!("{}{}", String::from_utf8_lossy(s_bytes), pad_utf8_out),
        };
        return Ok(Value::String(PhpString::from_vec(result.into_bytes())));
    }

    // For other encodings, build the output by converting the pad text from
    // UTF-8 back to the target encoding and concatenating with the original
    // (encoded) input bytes.
    let make_pad_bytes = |count: usize| -> Vec<u8> {
        let mut s = String::new();
        for i in 0..count { s.push(pad_chars[i % pad_chars.len()]); }
        match convert_encoding(s.as_bytes(), &enc_str, "UTF-8") {
            Some(b) => b,
            None => s.into_bytes(),
        }
    };

    let result_bytes: Vec<u8> = match pad_type {
        0 => {
            let mut out = make_pad_bytes(pad_len);
            out.extend_from_slice(s_bytes);
            out
        }
        2 => {
            let left = pad_len / 2;
            let right = pad_len - left;
            let mut out = make_pad_bytes(left);
            out.extend_from_slice(s_bytes);
            out.extend_from_slice(&make_pad_bytes(right));
            out
        }
        _ => {
            let mut out = s_bytes.to_vec();
            out.extend_from_slice(&make_pad_bytes(pad_len));
            out
        }
    };
    Ok(Value::String(PhpString::from_vec(result_bytes)))
}

fn mb_http_input_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}

fn mb_regex_set_options_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let prev = _vm.constants.get(b"__mb_regex_options".as_ref())
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_else(|| "msr".to_string());
    if let Some(arg) = args.first() {
        if !matches!(arg, Value::Null) {
            let opts = arg.to_php_string().to_string_lossy();
            _vm.constants.insert(b"__mb_regex_options".to_vec(),
                Value::String(PhpString::from_string(opts)));
        }
    }
    Ok(Value::String(PhpString::from_string(prev)))
}

// ========== Newly added functions ==========

fn mb_strimwidth_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let start = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let width = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    let trim_marker = args.get(3).map(|v| v.to_php_string()).unwrap_or_else(|| PhpString::from_bytes(b""));
    let _ = validate_encoding(_vm, "mb_strimwidth", 5, "$encoding", args, 4)?;

    let chars: Vec<char> = String::from_utf8_lossy(s.as_bytes()).chars().collect();
    let char_count = chars.len() as i64;

    let start_pos = if start < 0 {
        (char_count + start).max(0) as usize
    } else {
        start.min(char_count) as usize
    };

    let marker_chars: Vec<char> = String::from_utf8_lossy(trim_marker.as_bytes()).chars().collect();
    let marker_width = marker_chars.iter().map(|c| char_display_width(*c)).sum::<usize>();

    let width = if width < 0 {
        let total_width: usize = chars[start_pos..].iter().map(|c| char_display_width(*c)).sum();
        let target = total_width as i64 + width;
        if target <= 0 { 0usize } else { target as usize }
    } else {
        width as usize
    };

    // Collect characters from start, counting width
    let mut result = String::new();
    let mut current_width = 0;
    let remaining = &chars[start_pos..];

    // Check if the full string fits within width
    let total_width: usize = remaining.iter().map(|c| char_display_width(*c)).sum();
    if total_width <= width {
        for c in remaining {
            result.push(*c);
        }
        return Ok(Value::String(PhpString::from_vec(result.into_bytes())));
    }

    // Need to trim - leave room for marker
    let usable_width = if width >= marker_width { width - marker_width } else { width };
    for c in remaining {
        let cw = char_display_width(*c);
        if current_width + cw > usable_width {
            break;
        }
        result.push(*c);
        current_width += cw;
    }
    for c in &marker_chars {
        result.push(*c);
    }

    Ok(Value::String(PhpString::from_vec(result.into_bytes())))
}

fn char_display_width(c: char) -> usize {
    let cp = c as u32;
    // CJK Unified Ideographs and other fullwidth characters
    if (0x1100..=0x115F).contains(&cp) || // Hangul Jamo
       (0x2E80..=0x303E).contains(&cp) || // CJK Radicals
       (0x3041..=0x33BF).contains(&cp) || // Japanese
       (0x3400..=0x4DBF).contains(&cp) || // CJK Unified Ideographs Extension A
       (0x4E00..=0x9FFF).contains(&cp) || // CJK Unified Ideographs
       (0xA000..=0xA4CF).contains(&cp) || // Yi Syllables
       (0xAC00..=0xD7AF).contains(&cp) || // Hangul Syllables
       (0xF900..=0xFAFF).contains(&cp) || // CJK Compatibility Ideographs
       (0xFE30..=0xFE6F).contains(&cp) || // CJK Compatibility Forms
       (0xFF01..=0xFF60).contains(&cp) || // Fullwidth Forms
       (0xFFE0..=0xFFE6).contains(&cp) || // Fullwidth Signs
       (0x20000..=0x2FFFF).contains(&cp)  // CJK Unified Ideographs Extension B-F
    {
        2
    } else {
        1
    }
}

fn mb_strwidth_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let enc = validate_encoding(_vm, "mb_strwidth", 2, "$encoding", args, 1)?;
    let bytes = s.as_bytes();
    // Convert to UTF-8 if needed, then count widths
    let utf8_bytes: Vec<u8> = match enc.as_deref() {
        None => bytes.to_vec(),
        Some(e) => {
            let lower = e.to_ascii_lowercase();
            if lower == "utf-8" || lower == "utf8" || lower == "ascii" {
                bytes.to_vec()
            } else if let Some(converted) = convert_encoding(bytes, "UTF-8", e) {
                converted
            } else {
                bytes.to_vec()
            }
        }
    };
    let chars: Vec<char> = String::from_utf8_lossy(&utf8_bytes).chars().collect();
    let width: usize = chars.iter().map(|c| char_display_width(*c)).sum();
    Ok(Value::Long(width as i64))
}

fn mb_convert_kana_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let mode = args.get(1).map(|v| v.to_php_string().to_string_lossy()).unwrap_or_else(|| "KV".to_string());
    let _ = validate_encoding(_vm, "mb_convert_kana", 3, "$encoding", args, 2)?;

    let text = String::from_utf8_lossy(s.as_bytes()).to_string();
    let mut result = String::new();

    for c in text.chars() {
        let mut converted = c;
        let cp = c as u32;

        for m in mode.chars() {
            match m {
                'r' => {
                    // Fullwidth alphanumeric to halfwidth
                    if (0xFF21..=0xFF3A).contains(&cp) {
                        converted = char::from_u32(cp - 0xFF21 + 0x41).unwrap_or(c);
                    } else if (0xFF41..=0xFF5A).contains(&cp) {
                        converted = char::from_u32(cp - 0xFF41 + 0x61).unwrap_or(c);
                    }
                }
                'R' => {
                    // Halfwidth alphanumeric to fullwidth
                    if (0x41..=0x5A).contains(&cp) {
                        converted = char::from_u32(cp - 0x41 + 0xFF21).unwrap_or(c);
                    } else if (0x61..=0x7A).contains(&cp) {
                        converted = char::from_u32(cp - 0x61 + 0xFF41).unwrap_or(c);
                    }
                }
                'n' => {
                    // Fullwidth digits to halfwidth
                    if (0xFF10..=0xFF19).contains(&cp) {
                        converted = char::from_u32(cp - 0xFF10 + 0x30).unwrap_or(c);
                    }
                }
                'N' => {
                    // Halfwidth digits to fullwidth
                    if (0x30..=0x39).contains(&cp) {
                        converted = char::from_u32(cp - 0x30 + 0xFF10).unwrap_or(c);
                    }
                }
                'a' => {
                    // Fullwidth ASCII to halfwidth
                    if (0xFF01..=0xFF5E).contains(&cp) {
                        converted = char::from_u32(cp - 0xFF01 + 0x21).unwrap_or(c);
                    }
                }
                'A' => {
                    // Halfwidth ASCII to fullwidth
                    if (0x21..=0x7E).contains(&cp) {
                        converted = char::from_u32(cp - 0x21 + 0xFF01).unwrap_or(c);
                    }
                }
                's' => {
                    // Fullwidth space to halfwidth
                    if cp == 0x3000 {
                        converted = ' ';
                    }
                }
                'S' => {
                    // Halfwidth space to fullwidth
                    if cp == 0x20 {
                        converted = '\u{3000}';
                    }
                }
                'K' => {
                    // Halfwidth katakana to fullwidth
                    if (0xFF66..=0xFF9D).contains(&cp) {
                        // Map halfwidth katakana to fullwidth
                        let idx = cp - 0xFF66;
                        let fullwidth_map: &[u32] = &[
                            0x30F2, 0x30A1, 0x30A3, 0x30A5, 0x30A7, 0x30A9, 0x30E3, 0x30E5, 0x30E7, 0x30C3,
                            0x30FC, 0x30A2, 0x30A4, 0x30A6, 0x30A8, 0x30AA, 0x30AB, 0x30AD, 0x30AF, 0x30B1,
                            0x30B3, 0x30B5, 0x30B7, 0x30B9, 0x30BB, 0x30BD, 0x30BF, 0x30C1, 0x30C4, 0x30C6,
                            0x30C8, 0x30CA, 0x30CB, 0x30CC, 0x30CD, 0x30CE, 0x30CF, 0x30D2, 0x30D5, 0x30D8,
                            0x30DB, 0x30DE, 0x30DF, 0x30E0, 0x30E1, 0x30E2, 0x30E4, 0x30E6, 0x30E8, 0x30E9,
                            0x30EA, 0x30EB, 0x30EC, 0x30ED, 0x30EF, 0x30F3,
                        ];
                        if (idx as usize) < fullwidth_map.len() {
                            converted = char::from_u32(fullwidth_map[idx as usize]).unwrap_or(c);
                        }
                    }
                }
                'k' => {
                    // Fullwidth katakana to halfwidth
                    // Simplified - skip for now
                }
                'H' => {
                    // Halfwidth katakana to fullwidth hiragana
                    if (0xFF66..=0xFF9D).contains(&cp) {
                        let idx = cp - 0xFF66;
                        let hiragana_map: &[u32] = &[
                            0x3092, 0x3041, 0x3043, 0x3045, 0x3047, 0x3049, 0x3083, 0x3085, 0x3087, 0x3063,
                            0x30FC, 0x3042, 0x3044, 0x3046, 0x3048, 0x304A, 0x304B, 0x304D, 0x304F, 0x3051,
                            0x3053, 0x3055, 0x3057, 0x3059, 0x305B, 0x305D, 0x305F, 0x3061, 0x3064, 0x3066,
                            0x3068, 0x306A, 0x306B, 0x306C, 0x306D, 0x306E, 0x306F, 0x3072, 0x3075, 0x3078,
                            0x307B, 0x307E, 0x307F, 0x3080, 0x3081, 0x3082, 0x3084, 0x3086, 0x3088, 0x3089,
                            0x308A, 0x308B, 0x308C, 0x308D, 0x308F, 0x3093,
                        ];
                        if (idx as usize) < hiragana_map.len() {
                            converted = char::from_u32(hiragana_map[idx as usize]).unwrap_or(c);
                        }
                    }
                }
                'h' => {
                    // Fullwidth hiragana to halfwidth katakana
                    // Simplified - skip for now
                }
                'C' | 'c' => {
                    // Fullwidth katakana to hiragana or vice versa
                    if m == 'C' && (0x30A1..=0x30F6).contains(&cp) {
                        converted = char::from_u32(cp - 0x60).unwrap_or(c); // katakana to hiragana
                    } else if m == 'c' && (0x3041..=0x3096).contains(&cp) {
                        converted = char::from_u32(cp + 0x60).unwrap_or(c); // hiragana to katakana
                    }
                }
                'V' => {
                    // Combine voiced sound marks
                    // Simplified - skip complex handling
                }
                _ => {}
            }
        }
        result.push(converted);
    }

    Ok(Value::String(PhpString::from_vec(result.into_bytes())))
}

fn mb_decode_numericentity_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let _ = validate_encoding(_vm, "mb_decode_numericentity", 3, "$encoding", args, 2)?;
    let input_bytes = s.as_bytes().to_vec();
    let encoding = args.get(2).map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_else(|| "UTF-8".to_string());
    let enc_lower = encoding.to_ascii_lowercase();

    // Parse convmap as unsigned 32-bit quadruples (start, end, offset, mask).
    let convmap: Vec<(u32, u32, u32, u32)> = if let Some(Value::Array(arr)) = args.get(1) {
        let arr_borrow = arr.borrow();
        // Validate: every element must be int
        for (_, v) in arr_borrow.iter() {
            if !matches!(v, Value::Long(_)) {
                let msg = "mb_decode_numericentity(): Argument #2 ($map) must only be composed of values of type int".to_string();
                let exc = _vm.create_exception(b"ValueError", &msg, 0);
                _vm.current_exception = Some(exc);
                return Err(VmError { message: msg, line: _vm.current_line });
            }
        }
        // Validate: length must be multiple of 4
        if arr_borrow.len() == 0 || arr_borrow.len() % 4 != 0 {
            let msg = "mb_decode_numericentity(): Argument #2 ($map) must have a multiple of 4 elements".to_string();
            let exc = _vm.create_exception(b"ValueError", &msg, 0);
            _vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: _vm.current_line });
        }
        let values: Vec<u32> = arr_borrow.iter().map(|(_, v)| v.to_long() as u32).collect();
        let mut quads: Vec<(u32, u32, u32, u32)> = Vec::new();
        let mut i = 0;
        while i + 3 < values.len() {
            quads.push((values[i], values[i+1], values[i+2], values[i+3]));
            i += 4;
        }
        quads
    } else {
        vec![(0, 0x10ffff, 0, 0xffffff)]
    };

    // Decode input to a stream of u32 codepoints. UCS-4 can hold any u32; for
    // other encodings codepoints fit in 0..=0x10FFFF.
    let codepoints: Vec<u32> = if enc_lower == "ucs-4" || enc_lower == "ucs-4be" {
        let mut out = Vec::new();
        let mut i = 0;
        while i + 3 < input_bytes.len() {
            let cp = ((input_bytes[i] as u32) << 24)
                | ((input_bytes[i+1] as u32) << 16)
                | ((input_bytes[i+2] as u32) << 8)
                | (input_bytes[i+3] as u32);
            out.push(cp);
            i += 4;
        }
        out
    } else if enc_lower == "ucs-4le" {
        let mut out = Vec::new();
        let mut i = 0;
        while i + 3 < input_bytes.len() {
            let cp = (input_bytes[i] as u32)
                | ((input_bytes[i+1] as u32) << 8)
                | ((input_bytes[i+2] as u32) << 16)
                | ((input_bytes[i+3] as u32) << 24);
            out.push(cp);
            i += 4;
        }
        out
    } else {
        let utf8_text: String = if enc_lower == "utf-8" || enc_lower == "utf8" {
            String::from_utf8_lossy(&input_bytes).to_string()
        } else if let Some(enc) = resolve_encoding(&enc_lower) {
            let (cow, _, _) = enc.decode(&input_bytes);
            cow.to_string()
        } else {
            String::from_utf8_lossy(&input_bytes).to_string()
        };
        utf8_text.chars().map(|c| c as u32).collect()
    };

    let mut out_cps: Vec<u32> = Vec::with_capacity(codepoints.len());
    let mut i = 0;
    while i < codepoints.len() {
        if codepoints[i] == b'&' as u32 && i + 1 < codepoints.len() && codepoints[i+1] == b'#' as u32 {
            let start = i;
            let mut j = i + 2;
            let is_hex = j < codepoints.len() && (codepoints[j] == b'x' as u32 || codepoints[j] == b'X' as u32);
            if is_hex { j += 1; }
            let num_start = j;
            let max_digits = if is_hex { 8 } else { 10 };
            let mut digit_count = 0usize;
            while j < codepoints.len() && digit_count < max_digits {
                let cp = codepoints[j];
                let ok = if is_hex {
                    cp <= 0x7F && (cp as u8 as char).is_ascii_hexdigit()
                } else {
                    cp <= 0x7F && (cp as u8 as char).is_ascii_digit()
                };
                if !ok { break; }
                j += 1;
                digit_count += 1;
            }
            let too_many = digit_count == max_digits && j < codepoints.len() && {
                let cp = codepoints[j];
                if is_hex {
                    cp <= 0x7F && (cp as u8 as char).is_ascii_hexdigit()
                } else {
                    cp <= 0x7F && (cp as u8 as char).is_ascii_digit()
                }
            };
            let parsed_ok = digit_count > 0 && !too_many;
            if parsed_ok {
                let num_str: String = codepoints[num_start..j].iter()
                    .map(|&c| c as u8 as char).collect();
                let parsed = if is_hex {
                    u32::from_str_radix(&num_str, 16).ok()
                } else {
                    num_str.parse::<u32>().ok()
                };
                if let Some(cp) = parsed {
                    let mut decoded: Option<u32> = None;
                    for &(range_start, range_end, offset, mask) in &convmap {
                        let actual = cp.wrapping_sub(offset);
                        if actual >= range_start && actual <= range_end {
                            decoded = Some(actual & mask);
                            break;
                        }
                    }
                    if let Some(final_cp) = decoded {
                        if j < codepoints.len() && codepoints[j] == b';' as u32 {
                            j += 1;
                        }
                        out_cps.push(final_cp);
                        i = j;
                        continue;
                    }
                }
            }
            out_cps.push(codepoints[start]);
            i = start + 1;
        } else {
            out_cps.push(codepoints[i]);
            i += 1;
        }
    }

    // Re-encode. UCS-4 keeps raw u32; others use char::from_u32, falling
    // back to '?' for invalid codepoints.
    let out_bytes: Vec<u8> = if enc_lower == "ucs-4" || enc_lower == "ucs-4be" {
        let mut out = Vec::with_capacity(out_cps.len() * 4);
        for cp in &out_cps {
            out.push((*cp >> 24) as u8);
            out.push((*cp >> 16) as u8);
            out.push((*cp >> 8) as u8);
            out.push(*cp as u8);
        }
        out
    } else if enc_lower == "ucs-4le" {
        let mut out = Vec::with_capacity(out_cps.len() * 4);
        for cp in &out_cps {
            out.push(*cp as u8);
            out.push((*cp >> 8) as u8);
            out.push((*cp >> 16) as u8);
            out.push((*cp >> 24) as u8);
        }
        out
    } else {
        let mut result = String::new();
        for cp in &out_cps {
            if let Some(c) = char::from_u32(*cp) {
                result.push(c);
            } else {
                result.push('?');
            }
        }
        if enc_lower == "utf-8" || enc_lower == "utf8" {
            result.into_bytes()
        } else if let Some(enc) = resolve_encoding(&enc_lower) {
            let (cow, _, _) = enc.encode(&result);
            cow.to_vec()
        } else {
            result.into_bytes()
        }
    };

    Ok(Value::String(PhpString::from_vec(out_bytes)))
}

fn mb_encode_numericentity_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let map = args.get(1);
    let _ = validate_encoding(_vm, "mb_encode_numericentity", 3, "$encoding", args, 2)?;
    let is_hex = args.get(3).map(|v| v.is_truthy()).unwrap_or(false);

    let text = String::from_utf8_lossy(s.as_bytes()).to_string();

    // Parse the conversion map (groups of 4: start, end, offset, mask)
    let mut ranges: Vec<(u32, u32, i32, u32)> = Vec::new();
    if let Some(Value::Array(arr)) = map {
        let arr = arr.borrow();
        // Validate: every element must be int
        for (_, v) in arr.iter() {
            if !matches!(v, Value::Long(_)) {
                let msg = "mb_encode_numericentity(): Argument #2 ($map) must only be composed of values of type int".to_string();
                let exc = _vm.create_exception(b"ValueError", &msg, 0);
                _vm.current_exception = Some(exc);
                return Err(VmError { message: msg, line: _vm.current_line });
            }
        }
        // Validate: length must be multiple of 4
        if arr.len() == 0 || arr.len() % 4 != 0 {
            let msg = "mb_encode_numericentity(): Argument #2 ($map) must have a multiple of 4 elements".to_string();
            let exc = _vm.create_exception(b"ValueError", &msg, 0);
            _vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: _vm.current_line });
        }
        let values: Vec<i64> = arr.iter().map(|(_, v)| v.to_long()).collect();
        for chunk in values.chunks(4) {
            if chunk.len() == 4 {
                ranges.push((chunk[0] as u32, chunk[1] as u32, chunk[2] as i32, chunk[3] as u32));
            }
        }
    }

    let mut result = String::new();
    for c in text.chars() {
        let cp = c as u32;
        let mut encoded = false;
        for &(start, end, offset, mask) in &ranges {
            if cp >= start && cp <= end {
                let encoded_cp = ((cp as i64 + offset as i64) & mask as i64) as u32;
                if is_hex {
                    result.push_str(&format!("&#x{:x};", encoded_cp));
                } else {
                    result.push_str(&format!("&#{};", encoded_cp));
                }
                encoded = true;
                break;
            }
        }
        if !encoded {
            result.push(c);
        }
    }

    Ok(Value::String(PhpString::from_vec(result.into_bytes())))
}

fn mb_decode_mimeheader_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let text = s.to_string_lossy();

    // Decode MIME encoded words: =?charset?encoding?encoded_text?=
    let mut result = String::new();
    let mut remaining = text.as_str();

    while let Some(start) = remaining.find("=?") {
        result.push_str(&remaining[..start]);
        remaining = &remaining[start + 2..];

        // Find charset
        if let Some(q1) = remaining.find('?') {
            let _charset = &remaining[..q1];
            remaining = &remaining[q1 + 1..];

            // Find encoding type
            if let Some(q2) = remaining.find('?') {
                let enc_type = &remaining[..q2];
                remaining = &remaining[q2 + 1..];

                // Find end marker
                if let Some(end) = remaining.find("?=") {
                    let encoded = &remaining[..end];
                    remaining = &remaining[end + 2..];

                    match enc_type.to_ascii_uppercase().as_str() {
                        "B" => {
                            // Base64
                            let decoded = base64_decode(encoded.as_bytes());
                            result.push_str(&String::from_utf8_lossy(&decoded));
                        }
                        "Q" => {
                            // Quoted-printable
                            let mut i = 0;
                            let bytes = encoded.as_bytes();
                            while i < bytes.len() {
                                if bytes[i] == b'=' && i + 2 < bytes.len() {
                                    if let Ok(byte) = u8::from_str_radix(
                                        &String::from_utf8_lossy(&bytes[i+1..i+3]), 16
                                    ) {
                                        result.push(byte as char);
                                        i += 3;
                                    } else {
                                        result.push(bytes[i] as char);
                                        i += 1;
                                    }
                                } else if bytes[i] == b'_' {
                                    result.push(' ');
                                    i += 1;
                                } else {
                                    result.push(bytes[i] as char);
                                    i += 1;
                                }
                            }
                        }
                        _ => {
                            result.push_str(encoded);
                        }
                    }
                    continue;
                }
            }
        }
        // If parsing failed, output as-is
        result.push_str("=?");
    }
    result.push_str(remaining);

    Ok(Value::String(PhpString::from_vec(result.into_bytes())))
}

fn mb_encode_mimeheader_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let charset_raw = args.get(1).map(|v| v.to_php_string().to_string_lossy()).unwrap_or_else(|| "UTF-8".to_string());
    // Normalize charset to canonical form (uppercase for standard encodings)
    let charset = charset_raw.to_ascii_uppercase();
    let transfer_enc = args.get(2).map(|v| v.to_php_string().to_string_lossy()).unwrap_or_else(|| "B".to_string());

    let bytes = s.as_bytes();
    // If all ASCII, no encoding needed
    if bytes.iter().all(|&b| b < 128 && b != b'\r' && b != b'\n') {
        return Ok(Value::String(s));
    }

    if transfer_enc.to_ascii_uppercase() == "B" {
        let encoded = base64_encode(bytes);
        let result = format!("=?{}?B?{}?=", charset, encoded);
        Ok(Value::String(PhpString::from_vec(result.into_bytes())))
    } else {
        // Q encoding
        let mut encoded = String::new();
        for &b in bytes {
            if b == b' ' {
                encoded.push('_');
            } else if b.is_ascii_alphanumeric() || b == b'!' || b == b'*' || b == b'+' || b == b'-' || b == b'/' {
                encoded.push(b as char);
            } else {
                encoded.push_str(&format!("={:02X}", b));
            }
        }
        let result = format!("=?{}?Q?{}?=", charset, encoded);
        Ok(Value::String(PhpString::from_vec(result.into_bytes())))
    }
}

fn mb_convert_variables_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // mb_convert_variables($to_encoding, $from_encoding, &...$vars)
    // Returns the encoding detected (from_encoding) or false
    let _to_enc = args.first().map(|v| v.to_php_string().to_string_lossy()).unwrap_or_default();
    let from_enc = args.get(1);

    let empty_msg = "mb_convert_variables(): Argument #2 ($from_encoding) must specify at least one encoding".to_string();
    let from = match from_enc {
        Some(Value::Array(arr)) => {
            let arr = arr.borrow();
            if arr.len() == 0 {
                let exc = _vm.create_exception(b"ValueError", &empty_msg, 0);
                _vm.current_exception = Some(exc);
                return Err(VmError { message: empty_msg, line: _vm.current_line });
            }
            arr.iter().next().map(|(_, v)| v.to_php_string().to_string_lossy()).unwrap_or_else(|| "UTF-8".to_string())
        }
        Some(v) => {
            let s = v.to_php_string().to_string_lossy();
            if s.is_empty() {
                let exc = _vm.create_exception(b"ValueError", &empty_msg, 0);
                _vm.current_exception = Some(exc);
                return Err(VmError { message: empty_msg, line: _vm.current_line });
            }
            s
        }
        None => "UTF-8".to_string(),
    };

    // For now, just return the from encoding name
    Ok(Value::String(PhpString::from_string(from)))
}

fn mb_parse_str_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy();
    // Parse query string into array
    let mut result = PhpArray::new();
    for pair in s.split('&') {
        let pair = pair.trim();
        if pair.is_empty() { continue; }
        let (key, value) = if let Some(eq) = pair.find('=') {
            (&pair[..eq], &pair[eq+1..])
        } else {
            (pair, "")
        };
        result.set(
            ArrayKey::String(PhpString::from_string(key.to_string())),
            Value::String(PhpString::from_string(value.to_string())),
        );
    }
    // This should set $result (second arg) but for now return true
    Ok(Value::True)
}

fn mb_scrub_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let _ = validate_encoding(_vm, "mb_scrub", 2, "$encoding", args, 1)?;
    let bytes = s.as_bytes();
    // Replace invalid UTF-8 sequences with replacement character
    let cleaned = String::from_utf8_lossy(bytes).to_string();
    Ok(Value::String(PhpString::from_vec(cleaned.into_bytes())))
}

/// PHP's default trim characters: " \t\n\r\0\v" plus Unicode whitespace.
/// PHP 8.4+ mb_trim defaults to Unicode whitespace (including BOM, NBSP, etc).
fn is_default_trim_char(c: char) -> bool {
    matches!(c,
        ' ' | '\t' | '\n' | '\r' | '\0' | '\x0B' | '\x0C'
        | '\u{00A0}' | '\u{2000}'..='\u{200A}' | '\u{2028}' | '\u{2029}'
        | '\u{202F}' | '\u{205F}' | '\u{3000}' | '\u{FEFF}'
        | '\u{0085}' | '\u{180E}'
    )
}

/// Common helper for mb_trim / mb_ltrim / mb_rtrim. `mode` is 0 (both), 1 (left), 2 (right).
fn mb_trim_impl(vm: &mut Vm, fn_name: &str, args: &[Value], mode: u8) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let enc_opt = validate_encoding(vm, fn_name, 3, "$encoding", args, 2)?;
    let default_enc = current_internal_encoding(vm);
    let enc_str: String = enc_opt.unwrap_or(default_enc);
    let enc_lower = enc_str.to_ascii_lowercase();
    let chars_to_trim = args.get(1).and_then(|v| match v {
        Value::Null => None,
        _ => Some(v.to_php_string().as_bytes().to_vec()),
    });

    let is_utf8 = enc_lower == "utf-8" || enc_lower == "utf8" || enc_lower == "ascii";

    // Convert input and trim chars to UTF-8 for processing.
    let text: String = if is_utf8 {
        String::from_utf8_lossy(s.as_bytes()).to_string()
    } else if let Some(c) = convert_encoding(s.as_bytes(), "UTF-8", &enc_str) {
        String::from_utf8_lossy(&c).to_string()
    } else {
        String::from_utf8_lossy(s.as_bytes()).to_string()
    };

    let trim_chars_str: Option<String> = chars_to_trim.map(|bytes| {
        if is_utf8 {
            String::from_utf8_lossy(&bytes).to_string()
        } else if let Some(c) = convert_encoding(&bytes, "UTF-8", &enc_str) {
            String::from_utf8_lossy(&c).to_string()
        } else {
            String::from_utf8_lossy(&bytes).to_string()
        }
    });

    let trimmed: &str = match trim_chars_str.as_deref() {
        Some(chars) if chars.is_empty() => &text,
        Some(chars) => {
            let trim_chars: Vec<char> = chars.chars().collect();
            match mode {
                1 => text.trim_start_matches(|c: char| trim_chars.contains(&c)),
                2 => text.trim_end_matches(|c: char| trim_chars.contains(&c)),
                _ => text.trim_matches(|c: char| trim_chars.contains(&c)),
            }
        }
        None => match mode {
            1 => text.trim_start_matches(is_default_trim_char),
            2 => text.trim_end_matches(is_default_trim_char),
            _ => text.trim_matches(is_default_trim_char),
        },
    };

    // Convert result back to the source encoding.
    let result_bytes: Vec<u8> = if is_utf8 {
        trimmed.as_bytes().to_vec()
    } else if let Some(b) = convert_encoding(trimmed.as_bytes(), &enc_str, "UTF-8") {
        b
    } else {
        trimmed.as_bytes().to_vec()
    };
    Ok(Value::String(PhpString::from_vec(result_bytes)))
}

fn mb_trim_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    mb_trim_impl(_vm, "mb_trim", args, 0)
}

fn mb_ltrim_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    mb_trim_impl(_vm, "mb_ltrim", args, 1)
}

fn mb_rtrim_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    mb_trim_impl(_vm, "mb_rtrim", args, 2)
}

fn mb_ucfirst_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let _ = validate_encoding(_vm, "mb_ucfirst", 2, "$encoding", args, 1)?;
    let text = String::from_utf8_lossy(s.as_bytes()).to_string();
    let mut chars = text.chars();
    let result = match chars.next() {
        Some(c) => {
            // mb_ucfirst uses titlecase mapping where available (Lt category),
            // so digraphs like ǆ become ǅ (not Ǆ).
            let mut s = if let Some(tc) = to_titlecase_char(c) {
                tc.to_string()
            } else {
                c.to_uppercase().to_string()
            };
            s.extend(chars);
            s
        }
        None => String::new(),
    };
    Ok(Value::String(PhpString::from_vec(result.into_bytes())))
}

fn mb_lcfirst_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let _ = validate_encoding(_vm, "mb_lcfirst", 2, "$encoding", args, 1)?;
    let text = String::from_utf8_lossy(s.as_bytes()).to_string();
    let mut chars = text.chars();
    let result = match chars.next() {
        Some(c) => {
            // Lt -> simple lowercase mapping; ordinary chars use Unicode lc.
            let mut s = if let Some(lc) = titlecase_to_lowercase(c) {
                lc.to_string()
            } else {
                c.to_lowercase().to_string()
            };
            s.extend(chars);
            s
        }
        None => String::new(),
    };
    Ok(Value::String(PhpString::from_vec(result.into_bytes())))
}

fn mb_ereg_impl(_vm: &mut Vm, args: &[Value], case_insensitive: bool) -> Result<Value, VmError> {
    let pattern = args.first().unwrap_or(&Value::Null);
    let pattern_str = if let Value::Long(n) = pattern {
        if let Some(c) = char::from_u32(*n as u32) {
            let mut buf = [0u8; 4];
            let s = c.encode_utf8(&mut buf);
            regex::escape(s)
        } else {
            return Ok(Value::False);
        }
    } else {
        let ps = pattern.to_php_string().to_string_lossy();
        if ps.is_empty() {
            let fn_name = if case_insensitive { "mb_eregi" } else { "mb_ereg" };
            let msg = format!("{}(): Argument #1 ($pattern) must not be empty", fn_name);
            let exc = _vm.create_exception(b"ValueError", &msg, 0);
            _vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: _vm.current_line });
        }
        ps
    };
    let string = args.get(1).unwrap_or(&Value::Null).to_php_string().to_string_lossy();

    let full_pattern = if case_insensitive {
        format!("(?i){}", pattern_str)
    } else {
        pattern_str
    };
    let re = match regex::Regex::new(&full_pattern) {
        Ok(r) => r,
        Err(_) => return Ok(Value::False),
    };

    if let Some(captures) = re.captures(&string) {
        if args.len() >= 3 {
            let mut arr = PhpArray::new();
            for (i, m) in captures.iter().enumerate() {
                if let Some(mat) = m {
                    arr.set(ArrayKey::Int(i as i64), Value::String(PhpString::from_string(mat.as_str().to_string())));
                } else {
                    arr.set(ArrayKey::Int(i as i64), Value::False);
                }
            }
            if let Some(var_ref) = args.get(2) {
                if let Value::Reference(r) = var_ref {
                    *r.borrow_mut() = Value::Array(Rc::new(RefCell::new(arr)));
                }
            }
        }
        let matched_len = captures.get(0).map(|m| m.as_str().len()).unwrap_or(0);
        Ok(Value::Long(matched_len as i64))
    } else {
        Ok(Value::False)
    }
}

fn mb_ereg_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    mb_ereg_impl(_vm, args, false)
}

fn mb_eregi_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    mb_ereg_impl(_vm, args, true)
}

fn mb_ereg_replace_impl(_vm: &mut Vm, args: &[Value], case_insensitive: bool) -> Result<Value, VmError> {
    let pattern = args.first().unwrap_or(&Value::Null);
    let pattern_str = if let Value::Long(n) = pattern {
        if let Some(c) = char::from_u32(*n as u32) {
            let mut buf = [0u8; 4];
            let s = c.encode_utf8(&mut buf);
            regex::escape(s)
        } else {
            return Ok(Value::String(args.get(2).unwrap_or(&Value::Null).to_php_string()));
        }
    } else {
        pattern.to_php_string().to_string_lossy()
    };
    let replacement = args.get(1).unwrap_or(&Value::Null).to_php_string().to_string_lossy();
    let string = args.get(2).unwrap_or(&Value::Null).to_php_string().to_string_lossy();
    let options = args.get(3).map(|v| v.to_php_string().to_string_lossy()).unwrap_or_default();

    let ci = case_insensitive || options.contains('i');
    let full_pattern = if ci {
        format!("(?i){}", pattern_str)
    } else {
        pattern_str
    };
    let re = match regex::Regex::new(&full_pattern) {
        Ok(r) => r,
        Err(_) => return Ok(Value::String(PhpString::from_string(string))),
    };

    // Convert PHP backrefs (\1) to Rust ($1)
    let rust_replacement = {
        let mut out = String::new();
        let bytes = replacement.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'\\' && i + 1 < bytes.len() && bytes[i+1].is_ascii_digit() {
                out.push('$');
                out.push(bytes[i+1] as char);
                i += 2;
            } else {
                out.push(bytes[i] as char);
                i += 1;
            }
        }
        out
    };
    let result = re.replace_all(&string, rust_replacement.as_str());
    Ok(Value::String(PhpString::from_string(result.to_string())))
}

fn mb_ereg_replace_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    mb_ereg_replace_impl(_vm, args, false)
}

fn mb_eregi_replace_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    mb_ereg_replace_impl(_vm, args, true)
}

fn mb_ereg_match_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let pattern = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy();
    let string = args.get(1).unwrap_or(&Value::Null).to_php_string().to_string_lossy();
    let options = args.get(2).map(|v| v.to_php_string().to_string_lossy()).unwrap_or_default();

    let mut flags = String::new();
    if options.contains('i') {
        flags.push_str("(?i)");
    }
    let full_pattern = format!("^{}{}", flags, pattern);
    match regex::Regex::new(&full_pattern) {
        Ok(re) => {
            if re.is_match(&string) {
                Ok(Value::True)
            } else {
                Ok(Value::False)
            }
        }
        Err(_) => Ok(Value::False),
    }
}

// ========== mb_ereg_search functions ==========
// These use global state stored in VM constants for the search state

fn get_ereg_search_state(vm: &Vm) -> (String, String, usize) {
    let string = vm.constants.get(b"__mb_ereg_search_string".as_ref())
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_default();
    let pattern = vm.constants.get(b"__mb_ereg_search_pattern".as_ref())
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_default();
    let pos = vm.constants.get(b"__mb_ereg_search_pos".as_ref())
        .map(|v| v.to_long() as usize)
        .unwrap_or(0);
    (string, pattern, pos)
}

fn set_ereg_search_pos(vm: &mut Vm, pos: usize) {
    vm.constants.insert(b"__mb_ereg_search_pos".to_vec(), Value::Long(pos as i64));
}

fn set_ereg_search_regs(vm: &mut Vm, regs: Value) {
    vm.constants.insert(b"__mb_ereg_search_regs".to_vec(), regs);
}

fn mb_ereg_search_init_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let string = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy();
    let pattern_arg = args.get(1);

    if let Some(pat_arg) = pattern_arg {
        if !matches!(pat_arg, Value::Null) {
            let pat = pat_arg.to_php_string();
            if pat.as_bytes().is_empty() {
                let msg = "mb_ereg_search_init(): Argument #2 ($pattern) must not be empty";
                let exc = _vm.create_exception(b"ValueError", msg, 0);
                _vm.current_exception = Some(exc);
                return Err(VmError { message: msg.to_string(), line: _vm.current_line });
            }
        }
    }

    _vm.constants.insert(b"__mb_ereg_search_string".to_vec(), Value::String(PhpString::from_string(string)));
    if let Some(pat_arg) = pattern_arg {
        if !matches!(pat_arg, Value::Null) {
            let pat = pat_arg.to_php_string().to_string_lossy();
            _vm.constants.insert(b"__mb_ereg_search_pattern".to_vec(), Value::String(PhpString::from_string(pat)));
        }
    }
    _vm.constants.insert(b"__mb_ereg_search_pos".to_vec(), Value::Long(0));
    Ok(Value::True)
}

fn mb_ereg_search_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let (string, mut pattern, pos) = get_ereg_search_state(_vm);
    if let Some(pat) = args.first() {
        pattern = pat.to_php_string().to_string_lossy();
        _vm.constants.insert(b"__mb_ereg_search_pattern".to_vec(), Value::String(PhpString::from_string(pattern.clone())));
    }
    if pattern.is_empty() || string.is_empty() {
        return Ok(Value::False);
    }

    let re = match regex::Regex::new(&pattern) {
        Ok(r) => r,
        Err(_) => return Ok(Value::False),
    };

    if pos > string.len() {
        return Ok(Value::False);
    }
    if let Some(m) = re.find(&string[pos..]) {
        // PHP/Oniguruma: zero-width matches do NOT advance the search position.
        let new_pos = pos + m.end();
        set_ereg_search_pos(_vm, new_pos);

        let captures = re.captures(&string[pos..]).unwrap();
        let mut arr = PhpArray::new();
        for (i, cap) in captures.iter().enumerate() {
            if let Some(c) = cap {
                arr.set(ArrayKey::Int(i as i64), Value::String(PhpString::from_string(c.as_str().to_string())));
            }
        }
        set_ereg_search_regs(_vm, Value::Array(Rc::new(RefCell::new(arr))));

        Ok(Value::True)
    } else {
        Ok(Value::False)
    }
}

fn mb_ereg_search_pos_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let (string, mut pattern, pos) = get_ereg_search_state(_vm);
    if let Some(pat) = args.first() {
        pattern = pat.to_php_string().to_string_lossy();
    }
    if pattern.is_empty() || string.is_empty() {
        return Ok(Value::False);
    }

    let re = match regex::Regex::new(&pattern) {
        Ok(r) => r,
        Err(_) => return Ok(Value::False),
    };

    if pos > string.len() {
        return Ok(Value::False);
    }
    if let Some(m) = re.find(&string[pos..]) {
        let match_start = pos + m.start();
        let match_len = m.len();
        let new_pos = pos + m.end();
        set_ereg_search_pos(_vm, new_pos);

        let mut arr = PhpArray::new();
        arr.push(Value::Long(match_start as i64));
        arr.push(Value::Long(match_len as i64));
        Ok(Value::Array(Rc::new(RefCell::new(arr))))
    } else {
        Ok(Value::False)
    }
}

fn mb_ereg_search_regs_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let (string, mut pattern, pos) = get_ereg_search_state(_vm);
    if let Some(pat) = args.first() {
        pattern = pat.to_php_string().to_string_lossy();
    }
    if pattern.is_empty() {
        let msg = "No pattern was provided";
        let exc = _vm.create_exception(b"Error", msg, 0);
        _vm.current_exception = Some(exc);
        return Err(VmError { message: msg.to_string(), line: _vm.current_line });
    }
    if string.is_empty() {
        return Ok(Value::False);
    }

    let re = match regex::Regex::new(&pattern) {
        Ok(r) => r,
        Err(_) => return Ok(Value::False),
    };

    if pos > string.len() {
        return Ok(Value::False);
    }
    if let Some(captures) = re.captures(&string[pos..]) {
        let m = captures.get(0).unwrap();
        let new_pos = pos + m.end();
        set_ereg_search_pos(_vm, new_pos);

        let mut arr = PhpArray::new();
        for (i, cap) in captures.iter().enumerate() {
            if let Some(c) = cap {
                arr.set(ArrayKey::Int(i as i64), Value::String(PhpString::from_string(c.as_str().to_string())));
            } else {
                arr.set(ArrayKey::Int(i as i64), Value::False);
            }
        }
        set_ereg_search_regs(_vm, Value::Array(Rc::new(RefCell::new(arr.clone()))));
        Ok(Value::Array(Rc::new(RefCell::new(arr))))
    } else {
        Ok(Value::False)
    }
}

fn mb_ereg_search_getregs_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let regs = _vm.constants.get(b"__mb_ereg_search_regs".as_ref()).cloned();
    match regs {
        Some(v) if !matches!(v, Value::Null) => Ok(v),
        _ => Ok(Value::False),
    }
}

fn mb_ereg_search_getpos_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let pos = _vm.constants.get(b"__mb_ereg_search_pos".as_ref())
        .map(|v| v.to_long())
        .unwrap_or(0);
    Ok(Value::Long(pos))
}

fn mb_ereg_search_setpos_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let pos = args.first().map(|v| v.to_long()).unwrap_or(0);
    if pos < 0 {
        return Ok(Value::False);
    }
    set_ereg_search_pos(_vm, pos as usize);
    Ok(Value::True)
}

fn mb_send_mail_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // Sending mail is not supported
    Ok(Value::False)
}

/// Escape non-ASCII bytes in a byte slice as `\xHH` for use with regex::bytes.
fn escape_pattern_bytes(bytes: &[u8]) -> String {
    let mut out = String::new();
    for &b in bytes {
        if b < 0x80 {
            out.push(b as char);
        } else {
            out.push_str(&format!("\\x{:02x}", b));
        }
    }
    out
}

fn mb_split_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let pattern_bytes = args.first().unwrap_or(&Value::Null).to_php_string();
    let string_bytes = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let limit = args.get(2).map(|v| v.to_long()).unwrap_or(-1);

    if pattern_bytes.as_bytes().is_empty() {
        // PHP's mb_split returns false on empty pattern (no exception).
        return Ok(Value::False);
    }

    let translated = escape_pattern_bytes(pattern_bytes.as_bytes());

    let opts = _vm.constants.get(b"__mb_regex_options".as_ref())
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_default();
    let multiline = opts.contains('m');
    let case_insensitive = opts.contains('i');

    let re = match regex::bytes::RegexBuilder::new(&translated)
        .unicode(false)
        .multi_line(multiline)
        .case_insensitive(case_insensitive)
        .build()
    {
        Ok(r) => r,
        Err(_) => {
            match regex::bytes::Regex::new(&translated) {
                Ok(r) => r,
                Err(_) => return Ok(Value::False),
            }
        }
    };

    let mut result = PhpArray::new();
    let s = string_bytes.as_bytes();

    let max_splits: Option<usize> = if limit > 0 {
        Some((limit as usize).saturating_sub(1))
    } else {
        None
    };

    let mut last_end: usize = 0;
    let mut splits_done: usize = 0;
    let mut first_iter = true;
    for m in re.find_iter(s) {
        if let Some(max) = max_splits {
            if splits_done >= max {
                break;
            }
        }
        if m.start() == m.end() && m.start() == last_end && !first_iter {
            continue;
        }
        if m.start() == m.end() && m.start() == 0 && first_iter {
            first_iter = false;
            last_end = m.end();
            continue;
        }
        let piece = &s[last_end..m.start()];
        result.push(Value::String(PhpString::from_bytes(piece)));
        last_end = m.end();
        splits_done += 1;
        first_iter = false;
    }
    let rest = &s[last_end..];
    result.push(Value::String(PhpString::from_bytes(rest)));

    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

// ========== Helper functions ==========

/// Get byte positions of each UTF-8 character
fn utf8_char_positions(bytes: &[u8]) -> Vec<usize> {
    let mut positions = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        positions.push(i);
        let b = bytes[i];
        if b < 0x80 { i += 1; }
        else if b < 0xE0 { i += 2; }
        else if b < 0xF0 { i += 3; }
        else { i += 4; }
        // Safety: don't go past the end
        if i > bytes.len() { i = bytes.len(); }
    }
    positions
}

/// Split bytes into UTF-8 character slices
fn utf8_chars(bytes: &[u8]) -> Vec<&[u8]> {
    let mut result = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        let expected_len = if b < 0x80 { 1 }
            else if (0xC2..=0xDF).contains(&b) { 2 }
            else if (0xE0..=0xEF).contains(&b) { 3 }
            else if (0xF0..=0xF4).contains(&b) { 4 }
            else { 0 };
        let len = if expected_len == 0 {
            1
        } else if i + expected_len > bytes.len() {
            1
        } else {
            let mut valid = true;
            for j in 1..expected_len {
                if (bytes[i+j] & 0xC0) != 0x80 {
                    valid = false;
                    break;
                }
            }
            if valid { expected_len } else { 1 }
        };
        let end = i + len;
        result.push(&bytes[i..end]);
        i = end;
    }
    result
}
