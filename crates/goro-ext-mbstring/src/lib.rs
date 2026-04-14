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
    vm.register_function(b"mb_send_mail", mb_send_mail_fn);

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
    vm.constants.insert(b"MB_CASE_FOLD".to_vec(), Value::Long(0)); // Same as UPPER for simplicity
    vm.constants.insert(b"MB_CASE_UPPER_SIMPLE".to_vec(), Value::Long(3));
    vm.constants.insert(b"MB_CASE_LOWER_SIMPLE".to_vec(), Value::Long(4));
    vm.constants.insert(b"MB_CASE_FOLD_SIMPLE".to_vec(), Value::Long(5));
    vm.constants.insert(b"MB_CASE_TITLE_SIMPLE".to_vec(), Value::Long(6));
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
        "iso-2022-jp" => Some(encoding_rs::ISO_2022_JP),
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

fn mb_internal_encoding(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() || matches!(args.first(), Some(Value::Null)) {
        // Return the currently configured internal encoding
        if let Some(val) = vm.constants.get(b"mbstring.internal_encoding".as_ref()) {
            let enc_str = val.to_php_string().to_string_lossy();
            if !enc_str.is_empty() {
                return Ok(Value::String(PhpString::from_string(enc_str)));
            }
        }
        Ok(Value::String(PhpString::from_bytes(b"UTF-8")))
    } else {
        // Set the internal encoding
        let enc = args[0].to_php_string().to_string_lossy();
        let enc_lower = enc.to_ascii_lowercase();
        // Accept encoding_rs encodings + known PHP mbstring encodings
        let is_valid = resolve_encoding(&enc).is_some()
            || matches!(enc_lower.as_str(),
                "utf-8" | "ascii" | "jis" | "utf-7" | "utf7"
                | "ucs-4" | "ucs-4be" | "ucs-4le" | "ucs-2" | "ucs-2be" | "ucs-2le"
                | "utf-16" | "utf-16be" | "utf-16le" | "utf-32" | "utf-32be" | "utf-32le"
                | "cp50220" | "cp50221" | "cp50222" | "iso-2022-jp-ms"
                | "uuencode" | "qprint" | "quoted-printable" | "html" | "html-entities"
                | "7bit" | "8bit" | "base64" | "auto"
                | "euc-jp-2004" | "macjapanese" | "cp932" | "cp51932"
                | "iso-8859-1" | "iso-8859-2" | "iso-8859-3" | "iso-8859-4"
                | "iso-8859-5" | "iso-8859-6" | "iso-8859-7" | "iso-8859-8"
                | "iso-8859-9" | "iso-8859-10" | "iso-8859-13" | "iso-8859-14"
                | "iso-8859-15" | "iso-8859-16"
            );
        if is_valid {
            vm.constants.insert(b"mbstring.internal_encoding".to_vec(), Value::String(PhpString::from_string(enc)));
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
    let encoding = args.get(1).map(|v| v.to_php_string().to_string_lossy());
    let bytes = s.as_bytes();

    // For encodings where a char can be > 1 byte, we need to convert first
    if let Some(ref enc) = encoding {
        let enc_lower = enc.to_ascii_lowercase();
        if enc_lower == "utf-8" || enc_lower == "utf8" || enc_lower == "ascii" {
            let count = String::from_utf8_lossy(bytes).chars().count();
            return Ok(Value::Long(count as i64));
        }
        // For other encodings, convert to UTF-8 then count
        if let Some(converted) = convert_encoding(bytes, "UTF-8", enc) {
            let count = String::from_utf8_lossy(&converted).chars().count();
            return Ok(Value::Long(count as i64));
        }
    }

    let count = String::from_utf8_lossy(bytes).chars().count();
    Ok(Value::Long(count as i64))
}

fn mb_strtolower(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    // Validate encoding parameter if provided
    if let Some(enc_arg) = args.get(1) {
        let enc = enc_arg.to_php_string().to_string_lossy();
        if enc.is_empty() {
            let msg = "mb_strtolower(): Argument #2 ($encoding) must be a valid encoding, \"\" given";
            let exc = _vm.create_exception(b"ValueError", msg, 0);
            _vm.current_exception = Some(exc);
            return Err(VmError { message: msg.to_string(), line: _vm.current_line });
        }
    }
    let bytes = s.as_bytes();
    if let Ok(s) = std::str::from_utf8(bytes) {
        let lower = s.to_lowercase();
        Ok(Value::String(PhpString::from_vec(lower.into_bytes())))
    } else {
        let lower: Vec<u8> = bytes.iter().map(|b| b.to_ascii_lowercase()).collect();
        Ok(Value::String(PhpString::from_vec(lower)))
    }
}

fn mb_strtoupper(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    if let Some(enc_arg) = args.get(1) {
        let enc = enc_arg.to_php_string().to_string_lossy();
        if enc.is_empty() {
            let msg = "mb_strtoupper(): Argument #2 ($encoding) must be a valid encoding, \"\" given";
            let exc = _vm.create_exception(b"ValueError", msg, 0);
            _vm.current_exception = Some(exc);
            return Err(VmError { message: msg.to_string(), line: _vm.current_line });
        }
    }
    let bytes = s.as_bytes();
    if let Ok(s) = std::str::from_utf8(bytes) {
        let upper = s.to_uppercase();
        Ok(Value::String(PhpString::from_vec(upper.into_bytes())))
    } else {
        let upper: Vec<u8> = bytes.iter().map(|b| b.to_ascii_uppercase()).collect();
        Ok(Value::String(PhpString::from_vec(upper)))
    }
}

fn mb_substr(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // UTF-8 aware substr
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = s.as_bytes();
    let chars: Vec<&[u8]> = utf8_chars(bytes);
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
        return Ok(Value::Long(pos));
    }

    let chars: Vec<usize> = utf8_char_positions(h);
    let char_count = chars.len() as i64;

    if offset < 0 {
        // Negative offset: search from start but only up to (char_count + offset) chars from end
        let end_char = char_count + offset;
        if end_char < 0 {
            let msg = "mb_strrpos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)";
            let exc = vm.create_exception(b"ValueError", msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg.to_string(), line: vm.current_line });
        }
        // Search in h[0..end_byte] for the last occurrence
        let end_byte = if (end_char as usize) < chars.len() { chars[end_char as usize] } else { h.len() };
        // Need enough bytes for needle
        if end_byte < n.len() {
            return Ok(Value::False);
        }
        match h[..end_byte].windows(n.len()).rposition(|w| w == n) {
            Some(byte_pos) => {
                let char_pos = chars.iter().position(|&p| p == byte_pos).unwrap_or(byte_pos);
                Ok(Value::Long(char_pos as i64))
            }
            None => Ok(Value::False),
        }
    } else {
        // Positive offset: search from offset char position to end
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
        return Ok(Value::Long(pos));
    }

    let h = h_str.as_bytes();
    let n = n_str.as_bytes();
    let chars = utf8_char_positions(h);
    let char_count = chars.len() as i64;

    if offset < 0 {
        let end_char = char_count + offset;
        if end_char < 0 {
            let msg = "mb_strripos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)";
            let exc = vm.create_exception(b"ValueError", msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg.to_string(), line: vm.current_line });
        }
        let end_byte = if (end_char as usize) < chars.len() { chars[end_char as usize] } else { h.len() };
        if end_byte < n.len() {
            return Ok(Value::False);
        }
        match h[..end_byte].windows(n.len()).rposition(|w| w == n) {
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
    let from = get_from_encoding(args);

    // Emit deprecation for Base64 and HTML-ENTITIES
    let to_lower = to_enc.to_ascii_lowercase();
    let from_lower = from.to_ascii_lowercase();
    if to_lower == "base64" || from_lower == "base64" {
        _vm.emit_deprecated("mb_convert_encoding(): Handling Base64 via mbstring is deprecated; use base64_encode/base64_decode instead");
    }
    if to_lower == "html-entities" || to_lower == "html" || from_lower == "html-entities" || from_lower == "html" {
        _vm.emit_deprecated("mb_convert_encoding(): Handling HTML entities via mbstring is deprecated; use htmlspecialchars, htmlentities, or mb_encode_numericentity/mb_decode_numericentity instead");
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
        // Return current substitute character as string "none" (default in PHP)
        return Ok(Value::String(PhpString::from_bytes(b"none")));
    }

    let arg = &args[0];
    match arg {
        Value::String(s) => {
            let lower = s.to_string_lossy().to_ascii_lowercase();
            match lower.as_str() {
                "none" | "long" | "entity" => Ok(Value::True),
                _ => Ok(Value::True),
            }
        }
        Value::Long(_) => Ok(Value::True),
        _ => Ok(Value::True),
    }
}

fn mb_check_encoding(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() || matches!(args.first(), Some(Value::Null)) {
        return Ok(Value::True);
    }
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let encoding = args.get(1).map(|v| v.to_php_string().to_string_lossy()).unwrap_or_else(|| "UTF-8".to_string());
    let enc_lower = encoding.to_ascii_lowercase();

    // Emit deprecation for HTML-ENTITIES
    if enc_lower == "html-entities" || enc_lower == "html" {
        _vm.emit_deprecated("mb_check_encoding(): Handling HTML entities via mbstring is deprecated; use htmlspecialchars, htmlentities, or mb_encode_numericentity/mb_decode_numericentity instead");
    }

    if enc_lower == "utf-8" || enc_lower == "utf8" {
        let is_valid = std::str::from_utf8(s.as_bytes()).is_ok();
        Ok(if is_valid { Value::True } else { Value::False })
    } else if enc_lower == "ascii" || enc_lower == "us-ascii" {
        let is_valid = s.as_bytes().iter().all(|&b| b < 128);
        Ok(if is_valid { Value::True } else { Value::False })
    } else {
        // For other encodings, just return true
        Ok(Value::True)
    }
}

fn mb_substr_count(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let haystack = args.first().unwrap_or(&Value::Null).to_php_string();
    let needle = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() {
        // PHP 8.x: mb_substr_count() with empty needle returns 0
        // But actually PHP raises ValueError for empty needle
        return Ok(Value::Long(0));
    }
    // Non-overlapping count (like PHP's substr_count)
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
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() { return Ok(Value::False); }
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
    let h = haystack.as_bytes();
    let h_lower: Vec<u8> = String::from_utf8_lossy(h).to_lowercase().into_bytes();
    let n_lower: Vec<u8> = String::from_utf8_lossy(needle.as_bytes()).to_lowercase().into_bytes();
    if n_lower.is_empty() { return Ok(Value::False); }
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
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() { return Ok(Value::False); }
    // mb_strrchr uses first character of needle
    let first_char = utf8_chars(n)[0];
    // Find last occurrence
    let chars = utf8_chars(h);
    let mut last_pos = None;
    let mut byte_pos = 0;
    for c in &chars {
        if *c == first_char {
            last_pos = Some(byte_pos);
        }
        byte_pos += c.len();
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
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() { return Ok(Value::False); }

    let first_char_lower = String::from_utf8_lossy(utf8_chars(n)[0]).to_lowercase();
    let h_str = String::from_utf8_lossy(h);
    let h_lower = h_str.to_lowercase();

    // Find last occurrence of first char (case insensitive) in byte positions
    let h_lower_bytes = h_lower.as_bytes();
    let fc_bytes = first_char_lower.as_bytes();

    let mut last_pos = None;
    for i in 0..h_lower_bytes.len() {
        if h_lower_bytes[i..].starts_with(fc_bytes) {
            last_pos = Some(i);
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

fn mb_str_split_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let split_length = args.get(1).map(|v| v.to_long()).unwrap_or(1);
    if split_length < 1 {
        let msg = "mb_str_split(): Argument #2 ($length) must be greater than 0";
        let exc = vm.create_exception(b"ValueError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg.to_string(), line: vm.current_line });
    }
    let split_length = split_length as usize;
    let bytes = s.as_bytes();
    let mut result = PhpArray::new();
    let chars: Vec<&[u8]> = utf8_chars(bytes);
    for chunk in chars.chunks(split_length) {
        let mut s = Vec::new();
        for c in chunk { s.extend_from_slice(c); }
        result.push(Value::String(PhpString::from_vec(s)));
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn mb_convert_case_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let mode = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let bytes = s.as_bytes();

    if let Ok(s) = std::str::from_utf8(bytes) {
        let result = match mode {
            0 | 3 => s.to_uppercase(), // MB_CASE_UPPER / MB_CASE_UPPER_SIMPLE
            1 | 4 | 5 => s.to_lowercase(), // MB_CASE_LOWER / MB_CASE_LOWER_SIMPLE / MB_CASE_FOLD_SIMPLE
            2 => {
                // MB_CASE_TITLE - capitalize first letter of each word
                let mut result = String::new();
                let mut cap_next = true;
                for c in s.chars() {
                    if cap_next && c.is_alphabetic() {
                        for uc in c.to_uppercase() {
                            result.push(uc);
                        }
                        cap_next = false;
                    } else {
                        for lc in c.to_lowercase() {
                            result.push(lc);
                        }
                        if !c.is_alphabetic() && c != '\'' {
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

fn mb_language_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() || matches!(args.first(), Some(Value::Null)) {
        return Ok(Value::String(PhpString::from_bytes(b"neutral")));
    }
    Ok(Value::True)
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
            result.push(Value::String(PhpString::from_bytes(b"iso-ir-6")));
            result.push(Value::String(PhpString::from_bytes(b"ANSI_X3.4-1986")));
            result.push(Value::String(PhpString::from_bytes(b"ISO_646.irv:1991")));
            result.push(Value::String(PhpString::from_bytes(b"US-ASCII")));
            result.push(Value::String(PhpString::from_bytes(b"ISO646-US")));
            result.push(Value::String(PhpString::from_bytes(b"us")));
            result.push(Value::String(PhpString::from_bytes(b"IBM367")));
            result.push(Value::String(PhpString::from_bytes(b"cp367")));
            result.push(Value::String(PhpString::from_bytes(b"csASCII")));
        }
        "iso-8859-1" | "iso8859-1" | "latin1" => {
            result.push(Value::String(PhpString::from_bytes(b"ISO_8859-1")));
            result.push(Value::String(PhpString::from_bytes(b"latin1")));
        }
        _ => {
            // Check if encoding exists
            if resolve_encoding(&enc_lower).is_none() {
                let msg = format!("mb_encoding_aliases(): Unknown encoding \"{}\"", enc);
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
    let bytes = s.as_bytes();
    if bytes.is_empty() { return Ok(Value::False); }
    if let Ok(s) = std::str::from_utf8(bytes) {
        if let Some(c) = s.chars().next() {
            return Ok(Value::Long(c as i64));
        }
    }
    Ok(Value::Long(bytes[0] as i64))
}

fn mb_chr_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let cp = args.first().map(|v| v.to_long()).unwrap_or(0);
    if let Some(c) = char::from_u32(cp as u32) {
        let mut buf = [0u8; 4];
        let s = c.encode_utf8(&mut buf);
        Ok(Value::String(PhpString::from_bytes(s.as_bytes())))
    } else {
        Ok(Value::False)
    }
}

fn mb_strcut_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let start = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let length = args.get(2).map(|v| v.to_long());
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
        _ => Ok(Value::False),
    }
}

fn mb_regex_encoding_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        return Ok(Value::String(PhpString::from_bytes(b"UTF-8")));
    }
    Ok(Value::True)
}

fn mb_http_output_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        return Ok(Value::String(PhpString::from_bytes(b"UTF-8")));
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
    let pad_string = args.get(2).map(|v| v.to_php_string()).unwrap_or_else(|| PhpString::from_bytes(b" "));
    let pad_type = args.get(3).map(|v| v.to_long()).unwrap_or(1); // STR_PAD_RIGHT
    let _ = vm;
    let s_bytes = s.as_bytes();
    let char_count = String::from_utf8_lossy(s_bytes).chars().count();
    if length as usize <= char_count || pad_string.as_bytes().is_empty() {
        return Ok(Value::String(s));
    }
    let pad_chars: Vec<char> = String::from_utf8_lossy(pad_string.as_bytes()).chars().collect();
    let pad_len = length as usize - char_count;
    let mut pad = String::new();
    for i in 0..pad_len {
        pad.push(pad_chars[i % pad_chars.len()]);
    }
    let result = match pad_type {
        0 => format!("{}{}", pad, String::from_utf8_lossy(s_bytes)), // STR_PAD_LEFT
        2 => {
            let left = pad_len / 2;
            let right = pad_len - left;
            let mut lp = String::new();
            for i in 0..left { lp.push(pad_chars[i % pad_chars.len()]); }
            let mut rp = String::new();
            for i in 0..right { rp.push(pad_chars[i % pad_chars.len()]); }
            format!("{}{}{}", lp, String::from_utf8_lossy(s_bytes), rp)
        }
        _ => format!("{}{}", String::from_utf8_lossy(s_bytes), pad), // STR_PAD_RIGHT
    };
    Ok(Value::String(PhpString::from_vec(result.into_bytes())))
}

fn mb_http_input_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}

fn mb_regex_set_options_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        return Ok(Value::String(PhpString::from_bytes(b"msr")));
    }
    Ok(Value::String(PhpString::from_bytes(b"msr")))
}

// ========== Newly added functions ==========

fn mb_strimwidth_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let start = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let width = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    let trim_marker = args.get(3).map(|v| v.to_php_string()).unwrap_or_else(|| PhpString::from_bytes(b""));

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
    let chars: Vec<char> = String::from_utf8_lossy(s.as_bytes()).chars().collect();
    let width: usize = chars.iter().map(|c| char_display_width(*c)).sum();
    Ok(Value::Long(width as i64))
}

fn mb_convert_kana_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let mode = args.get(1).map(|v| v.to_php_string().to_string_lossy()).unwrap_or_else(|| "KV".to_string());

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
    let text = String::from_utf8_lossy(s.as_bytes()).to_string();

    // Parse the convmap: array of [start, end, offset, mask] quadruples
    let convmap = if let Some(Value::Array(arr)) = args.get(1) {
        let arr_borrow = arr.borrow();
        let values: Vec<i64> = arr_borrow.iter().map(|(_, v)| v.to_long()).collect();
        // Group into quadruples
        let mut quads: Vec<(i64, i64, i64, i64)> = Vec::new();
        let mut i = 0;
        while i + 3 < values.len() {
            quads.push((values[i], values[i+1], values[i+2], values[i+3]));
            i += 4;
        }
        quads
    } else {
        vec![(0, 0x10ffff, 0, 0xffffff)] // default: decode all
    };

    // Parse numeric entities (&#xHH; and &#DD;) with convmap
    let mut result = String::new();
    let mut i = 0;
    let chars: Vec<char> = text.chars().collect();
    while i < chars.len() {
        if i + 2 < chars.len() && chars[i] == '&' && chars[i+1] == '#' {
            let start = i;
            i += 2;
            let is_hex = i < chars.len() && (chars[i] == 'x' || chars[i] == 'X');
            if is_hex { i += 1; }
            let num_start = i;
            let mut num = String::new();
            let mut valid_digits = true;
            while i < chars.len() && chars[i] != ';' {
                let c = chars[i];
                if is_hex {
                    if !c.is_ascii_hexdigit() { valid_digits = false; }
                } else {
                    if !c.is_ascii_digit() { valid_digits = false; }
                }
                num.push(c);
                i += 1;
            }
            if i < chars.len() && chars[i] == ';' && valid_digits && !num.is_empty() {
                i += 1;
                let codepoint = if is_hex {
                    u32::from_str_radix(&num, 16).ok()
                } else {
                    num.parse::<u32>().ok()
                };
                if let Some(cp) = codepoint {
                    // Apply convmap: find matching range and apply offset
                    let mut decoded = false;
                    for &(range_start, range_end, offset, mask) in &convmap {
                        let actual_cp = (cp as i64) - offset;
                        if actual_cp >= range_start && actual_cp <= range_end {
                            let final_cp = actual_cp & mask;
                            if let Some(c) = char::from_u32(final_cp as u32) {
                                result.push(c);
                                decoded = true;
                                break;
                            }
                        }
                    }
                    if decoded { continue; }
                }
            }
            // Invalid entity or not in convmap - output as-is
            for c in &chars[start..i] {
                result.push(*c);
            }
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }

    Ok(Value::String(PhpString::from_vec(result.into_bytes())))
}

fn mb_encode_numericentity_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let map = args.get(1);
    let is_hex = args.get(3).map(|v| v.is_truthy()).unwrap_or(false);

    let text = String::from_utf8_lossy(s.as_bytes()).to_string();

    // Parse the conversion map (groups of 4: start, end, offset, mask)
    let mut ranges: Vec<(u32, u32, i32, u32)> = Vec::new();
    if let Some(Value::Array(arr)) = map {
        let arr = arr.borrow();
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

    let from = match from_enc {
        Some(Value::Array(arr)) => {
            let arr = arr.borrow();
            arr.iter().next().map(|(_, v)| v.to_php_string().to_string_lossy()).unwrap_or_else(|| "UTF-8".to_string())
        }
        Some(v) => v.to_php_string().to_string_lossy(),
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
    let bytes = s.as_bytes();
    // Replace invalid UTF-8 sequences with replacement character
    let cleaned = String::from_utf8_lossy(bytes).to_string();
    Ok(Value::String(PhpString::from_vec(cleaned.into_bytes())))
}

fn mb_trim_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let chars_to_trim = args.get(1).map(|v| v.to_php_string().to_string_lossy());

    let text = String::from_utf8_lossy(s.as_bytes()).to_string();
    let trimmed = if let Some(chars) = chars_to_trim {
        let trim_chars: Vec<char> = chars.chars().collect();
        text.trim_matches(|c: char| trim_chars.contains(&c)).to_string()
    } else {
        text.trim().to_string()
    };
    Ok(Value::String(PhpString::from_vec(trimmed.into_bytes())))
}

fn mb_ltrim_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let chars_to_trim = args.get(1).map(|v| v.to_php_string().to_string_lossy());

    let text = String::from_utf8_lossy(s.as_bytes()).to_string();
    let trimmed = if let Some(chars) = chars_to_trim {
        let trim_chars: Vec<char> = chars.chars().collect();
        text.trim_start_matches(|c: char| trim_chars.contains(&c)).to_string()
    } else {
        text.trim_start().to_string()
    };
    Ok(Value::String(PhpString::from_vec(trimmed.into_bytes())))
}

fn mb_rtrim_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let chars_to_trim = args.get(1).map(|v| v.to_php_string().to_string_lossy());

    let text = String::from_utf8_lossy(s.as_bytes()).to_string();
    let trimmed = if let Some(chars) = chars_to_trim {
        let trim_chars: Vec<char> = chars.chars().collect();
        text.trim_end_matches(|c: char| trim_chars.contains(&c)).to_string()
    } else {
        text.trim_end().to_string()
    };
    Ok(Value::String(PhpString::from_vec(trimmed.into_bytes())))
}

fn mb_ucfirst_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let text = String::from_utf8_lossy(s.as_bytes()).to_string();
    let mut chars = text.chars();
    let result = match chars.next() {
        Some(c) => {
            let mut s = c.to_uppercase().to_string();
            s.extend(chars);
            s
        }
        None => String::new(),
    };
    Ok(Value::String(PhpString::from_vec(result.into_bytes())))
}

fn mb_lcfirst_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let text = String::from_utf8_lossy(s.as_bytes()).to_string();
    let mut chars = text.chars();
    let result = match chars.next() {
        Some(c) => {
            let mut s = c.to_lowercase().to_string();
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
            _vm.emit_warning_at(&format!("{}(): Argument #1 ($pattern) must not be empty", fn_name), _vm.current_line);
            return Ok(Value::False);
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
    let pattern = args.get(1).map(|v| v.to_php_string().to_string_lossy());

    _vm.constants.insert(b"__mb_ereg_search_string".to_vec(), Value::String(PhpString::from_string(string)));
    if let Some(pat) = pattern {
        _vm.constants.insert(b"__mb_ereg_search_pattern".to_vec(), Value::String(PhpString::from_string(pat)));
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
        let new_pos = pos + m.end();
        // If zero-width match, advance by one byte to avoid infinite loop
        let new_pos = if m.start() == m.end() { (new_pos + 1).min(string.len()) } else { new_pos };
        set_ereg_search_pos(_vm, new_pos);

        // Store match info for getregs
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
        let new_pos = if m.start() == m.end() { (new_pos + 1).min(string.len()) } else { new_pos };
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
    if let Some(captures) = re.captures(&string[pos..]) {
        let m = captures.get(0).unwrap();
        let new_pos = pos + m.end();
        let new_pos = if m.start() == m.end() { (new_pos + 1).min(string.len()) } else { new_pos };
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
        let len = if b < 0x80 { 1 } else if b < 0xE0 { 2 } else if b < 0xF0 { 3 } else { 4 };
        let end = (i + len).min(bytes.len());
        result.push(&bytes[i..end]);
        i = end;
    }
    result
}
