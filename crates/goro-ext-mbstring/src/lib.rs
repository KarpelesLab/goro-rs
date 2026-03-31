use std::cell::RefCell;
use std::rc::Rc;
use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

pub fn register(vm: &mut Vm) {
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
}

fn mb_detect_encoding(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(PhpString::from_bytes(b"UTF-8")))
}

fn mb_internal_encoding(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        Ok(Value::String(PhpString::from_bytes(b"UTF-8")))
    } else {
        Ok(Value::True)
    }
}

fn mb_strlen(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    // Simplified: count UTF-8 characters
    let count = String::from_utf8_lossy(s.as_bytes()).chars().count();
    Ok(Value::Long(count as i64))
}

fn mb_strtolower(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let lower: Vec<u8> = s
        .as_bytes()
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect();
    Ok(Value::String(PhpString::from_vec(lower)))
}

fn mb_strtoupper(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let upper: Vec<u8> = s
        .as_bytes()
        .iter()
        .map(|b| b.to_ascii_uppercase())
        .collect();
    Ok(Value::String(PhpString::from_vec(upper)))
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

fn mb_strpos(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let haystack = args.first().unwrap_or(&Value::Null).to_php_string();
    let needle = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let offset = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() {
        return Ok(Value::False);
    }
    // Convert byte offset from character offset (UTF-8)
    let chars: Vec<usize> = utf8_char_positions(h);
    let start_byte = if offset < 0 {
        let from = (chars.len() as i64 + offset).max(0) as usize;
        if from < chars.len() { chars[from] } else { h.len() }
    } else {
        let from = offset as usize;
        if from < chars.len() { chars[from] } else { h.len() }
    };
    if start_byte >= h.len() {
        return Ok(Value::False);
    }
    match h[start_byte..].windows(n.len()).position(|w| w == n) {
        Some(byte_pos) => {
            // Convert byte position back to character position
            let abs_byte_pos = start_byte + byte_pos;
            let char_pos = chars.iter().position(|&p| p == abs_byte_pos).unwrap_or(abs_byte_pos);
            Ok(Value::Long(char_pos as i64))
        }
        None => Ok(Value::False),
    }
}

fn mb_strrpos(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let haystack = args.first().unwrap_or(&Value::Null).to_php_string();
    let needle = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() { return Ok(Value::False); }
    match h.windows(n.len()).rposition(|w| w == n) {
        Some(byte_pos) => {
            let chars = utf8_char_positions(h);
            let char_pos = chars.iter().position(|&p| p == byte_pos).unwrap_or(byte_pos);
            Ok(Value::Long(char_pos as i64))
        }
        None => Ok(Value::False),
    }
}

fn mb_stripos(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let haystack = args.first().unwrap_or(&Value::Null).to_php_string();
    let needle = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let h_lower: Vec<u8> = haystack.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
    let n_lower: Vec<u8> = needle.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
    if n_lower.is_empty() { return Ok(Value::False); }
    let offset = args.get(2).map(|v| v.to_long()).unwrap_or(0) as usize;
    let chars = utf8_char_positions(&h_lower);
    let start = if offset < chars.len() { chars[offset] } else { h_lower.len() };
    match h_lower[start..].windows(n_lower.len()).position(|w| w == n_lower.as_slice()) {
        Some(byte_pos) => {
            let abs = start + byte_pos;
            let char_pos = chars.iter().position(|&p| p == abs).unwrap_or(abs);
            Ok(Value::Long(char_pos as i64))
        }
        None => Ok(Value::False),
    }
}

fn mb_strripos(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let haystack = args.first().unwrap_or(&Value::Null).to_php_string();
    let needle = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let h_lower: Vec<u8> = haystack.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
    let n_lower: Vec<u8> = needle.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
    if n_lower.is_empty() { return Ok(Value::False); }
    match h_lower.windows(n_lower.len()).rposition(|w| w == n_lower.as_slice()) {
        Some(byte_pos) => {
            let chars = utf8_char_positions(&h_lower);
            let char_pos = chars.iter().position(|&p| p == byte_pos).unwrap_or(byte_pos);
            Ok(Value::Long(char_pos as i64))
        }
        None => Ok(Value::False),
    }
}

fn mb_convert_encoding(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Simplified: just return the string as-is for UTF-8/ASCII/ISO-8859-1
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    Ok(Value::String(s))
}

fn mb_substitute_character(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        // Return current substitute character
        return Ok(Value::Long(0xFFFD)); // Unicode replacement character
    }
    // Setting - just return true
    Ok(Value::True)
}

fn mb_check_encoding(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    // Check if valid UTF-8
    let is_valid = std::str::from_utf8(s.as_bytes()).is_ok();
    Ok(if is_valid { Value::True } else { Value::False })
}

fn mb_substr_count(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let haystack = args.first().unwrap_or(&Value::Null).to_php_string();
    let needle = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() { return Ok(Value::Long(0)); }
    let count = h.windows(n.len()).filter(|w| *w == n).count();
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
    let h_lower: Vec<u8> = h.iter().map(|b| b.to_ascii_lowercase()).collect();
    let n_lower: Vec<u8> = needle.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
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
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() { return Ok(Value::False); }
    let search = n[0]; // mb_strrchr uses first byte of needle
    match h.iter().rposition(|&b| b == search) {
        Some(pos) => Ok(Value::String(PhpString::from_vec(h[pos..].to_vec()))),
        None => Ok(Value::False),
    }
}

fn mb_strrichr_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let haystack = args.first().unwrap_or(&Value::Null).to_php_string();
    let needle = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() { return Ok(Value::False); }
    let search = n[0].to_ascii_lowercase();
    match h.iter().rposition(|b| b.to_ascii_lowercase() == search) {
        Some(pos) => Ok(Value::String(PhpString::from_vec(h[pos..].to_vec()))),
        None => Ok(Value::False),
    }
}

fn mb_str_split_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let split_length = args.get(1).map(|v| v.to_long()).unwrap_or(1).max(1) as usize;
    let bytes = s.as_bytes();
    let mut result = PhpArray::new();
    // UTF-8 aware splitting
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
    let result: Vec<u8> = match mode {
        0 => bytes.iter().map(|b| b.to_ascii_uppercase()).collect(), // MB_CASE_UPPER
        1 => bytes.iter().map(|b| b.to_ascii_lowercase()).collect(), // MB_CASE_LOWER
        2 => {
            // MB_CASE_TITLE
            let mut cap_next = true;
            bytes.iter().map(|&b| {
                if cap_next && b.is_ascii_alphabetic() {
                    cap_next = false;
                    b.to_ascii_uppercase()
                } else {
                    if b == b' ' || b == b'\t' || b == b'\n' { cap_next = true; }
                    b.to_ascii_lowercase()
                }
            }).collect()
        }
        _ => bytes.to_vec(),
    };
    Ok(Value::String(PhpString::from_vec(result)))
}

fn mb_language_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        return Ok(Value::String(PhpString::from_bytes(b"neutral")));
    }
    Ok(Value::True)
}

fn mb_list_encodings_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut result = PhpArray::new();
    for enc in &["ASCII", "UTF-8", "ISO-8859-1", "ISO-8859-15", "Windows-1252", "EUC-JP", "SJIS", "UTF-16", "UTF-32"] {
        result.push(Value::String(PhpString::from_bytes(enc.as_bytes())));
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn mb_encoding_aliases_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let enc = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy().to_ascii_uppercase();
    let mut result = PhpArray::new();
    match enc.as_str() {
        "UTF-8" => { result.push(Value::String(PhpString::from_bytes(b"utf8"))); }
        "ASCII" => { result.push(Value::String(PhpString::from_bytes(b"us-ascii"))); }
        "ISO-8859-1" => { result.push(Value::String(PhpString::from_bytes(b"latin1"))); }
        _ => {}
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn mb_ord_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = s.as_bytes();
    if bytes.is_empty() { return Ok(Value::False); }
    // Decode first UTF-8 character
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

    // Compute start position (negative = from end)
    let start_byte = if start < 0 {
        (len + start).max(0) as usize
    } else {
        start.min(len) as usize
    };

    // Adjust start to UTF-8 character boundary (don't split a multi-byte char)
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
            // Adjust to UTF-8 boundary
            let mut eb = e;
            while eb > start_byte && eb < bytes.len() && (bytes[eb] & 0xC0) == 0x80 {
                eb -= 1;
            }
            eb
        }
        Some(l) => {
            let e = (start_byte as i64 + l).min(len) as usize;
            // Adjust to UTF-8 boundary
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
        // Return current detect order
        let arr = PhpArray::new();
        let mut result = arr;
        result.push(Value::String(PhpString::from_bytes(b"ASCII")));
        result.push(Value::String(PhpString::from_bytes(b"UTF-8")));
        Ok(Value::Array(Rc::new(RefCell::new(result))))
    } else {
        // Set detect order - we just accept it
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
            // Return all info as array
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
    // Setting regex encoding - just return true
    Ok(Value::True)
}

fn mb_http_output_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        return Ok(Value::String(PhpString::from_bytes(b"UTF-8")));
    }
    // Setting http output encoding - just return true
    Ok(Value::True)
}

fn mb_preferred_mime_name_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let encoding = args.first().unwrap_or(&Value::Null).to_php_string();
    let enc_lower: Vec<u8> = encoding.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
    match enc_lower.as_slice() {
        b"utf-8" | b"utf8" => Ok(Value::String(PhpString::from_bytes(b"UTF-8"))),
        b"iso-8859-1" | b"latin1" => Ok(Value::String(PhpString::from_bytes(b"ISO-8859-1"))),
        b"ascii" | b"us-ascii" => Ok(Value::String(PhpString::from_bytes(b"US-ASCII"))),
        b"shift_jis" | b"sjis" => Ok(Value::String(PhpString::from_bytes(b"Shift_JIS"))),
        b"euc-jp" => Ok(Value::String(PhpString::from_bytes(b"EUC-JP"))),
        b"iso-2022-jp" => Ok(Value::String(PhpString::from_bytes(b"ISO-2022-JP"))),
        _ => Ok(Value::False),
    }
}

fn mb_output_handler_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Just pass through the content without conversion
    let content = args.first().unwrap_or(&Value::Null).to_php_string();
    Ok(Value::String(content))
}

fn mb_str_pad_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // mb_str_pad is like str_pad but multibyte aware
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let length = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let pad_string = args.get(2).map(|v| v.to_php_string()).unwrap_or_else(|| PhpString::from_bytes(b" "));
    let pad_type = args.get(3).map(|v| v.to_long()).unwrap_or(1); // STR_PAD_RIGHT
    let _ = vm; // suppress unused warning
    // Count actual UTF-8 characters
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
        2 => { // STR_PAD_BOTH
            let left = pad_len / 2;
            let right = pad_len - left;
            let mut lp = String::new();
            for i in 0..left {
                lp.push(pad_chars[i % pad_chars.len()]);
            }
            let mut rp = String::new();
            for i in 0..right {
                rp.push(pad_chars[i % pad_chars.len()]);
            }
            format!("{}{}{}", lp, String::from_utf8_lossy(s_bytes), rp)
        }
        _ => format!("{}{}", String::from_utf8_lossy(s_bytes), pad), // STR_PAD_RIGHT
    };
    Ok(Value::String(PhpString::from_vec(result.into_bytes())))
}

fn mb_http_input_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // mb_http_input returns the HTTP input character encoding
    // In CLI mode this typically returns false
    if args.is_empty() {
        return Ok(Value::False);
    }
    let type_str = args.first().unwrap_or(&Value::Null).to_php_string();
    let _type_lower: Vec<u8> = type_str.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
    // In CLI context, there is no HTTP input encoding
    Ok(Value::False)
}

fn mb_regex_set_options_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // mb_regex_set_options gets/sets default options for mbregex functions
    if args.is_empty() {
        // Return current options string (default)
        return Ok(Value::String(PhpString::from_bytes(b"msr")));
    }
    // Setting options - return the previous options string
    Ok(Value::String(PhpString::from_bytes(b"msr")))
}

/// Helper: get byte positions of each UTF-8 character
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
    }
    positions
}

/// Helper: split bytes into UTF-8 character slices
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
