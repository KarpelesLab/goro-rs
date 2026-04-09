use std::cell::RefCell;
use std::rc::Rc;
use goro_core::array::{ArrayKey, PhpArray};
use goro_core::opcode::ParamType;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

pub fn register(vm: &mut Vm) {
    vm.register_function(b"strlen", strlen);
    vm.register_function(b"strtolower", strtolower);
    vm.register_function(b"strtoupper", strtoupper);
    vm.register_function(b"trim", trim);
    vm.register_function(b"ltrim", ltrim);
    vm.register_function(b"rtrim", rtrim);
    vm.register_function(b"substr", substr);
    vm.register_function(b"strpos", strpos);
    vm.register_function(b"str_contains", str_contains);
    vm.register_function(b"str_starts_with", str_starts_with);
    vm.register_function(b"str_ends_with", str_ends_with);
    vm.register_function(b"str_repeat", str_repeat);
    vm.register_function(b"str_replace", str_replace);
    vm.register_function(b"explode", explode);
    vm.register_function(b"implode", implode);
    vm.register_function(b"join", implode); // alias
    vm.register_function(b"chr", chr);
    vm.register_function(b"ord", ord);
    vm.register_function(b"sprintf", sprintf);
    vm.register_function(b"nl2br", nl2br);
    vm.register_function(b"chunk_split", chunk_split);
    vm.register_function(b"str_pad", str_pad);
    vm.register_function(b"str_word_count", str_word_count);
    vm.register_function(b"strtolower", strtolower);
    vm.register_function(b"ucfirst", ucfirst);
    vm.register_function(b"lcfirst", lcfirst);
    vm.register_function(b"ucwords", ucwords);
    vm.register_function(b"strrev", strrev);
    vm.register_function(b"addslashes", addslashes);
    vm.register_function(b"stripslashes", stripslashes);
    vm.register_function(b"addcslashes", addcslashes);
    vm.register_function(b"stripcslashes", stripcslashes);
    vm.register_function(b"str_rot13", str_rot13);
    vm.register_function(b"strip_tags", strip_tags);
    vm.register_function(b"quoted_printable_encode", quoted_printable_encode);
    vm.register_function(b"quoted_printable_decode", quoted_printable_decode);
    vm.register_function(b"convert_uuencode", convert_uuencode);
    vm.register_function(b"convert_uudecode", convert_uudecode);
    vm.register_function(b"str_getcsv", str_getcsv);
    vm.register_function(b"rtrim", rtrim); // alias chop
    vm.register_function(b"chop", rtrim);
    vm.register_function(b"strtr", strtr);
    vm.register_function(b"str_shuffle", str_shuffle);
    vm.register_function(b"substr_compare", substr_compare);
    vm.register_function(b"similar_text", similar_text);
    vm.register_function(b"soundex", soundex);
    vm.register_function(b"metaphone", metaphone);
    vm.register_function(b"levenshtein", levenshtein);
    vm.register_function(b"count_chars", count_chars);
    vm.register_function(b"str_split", str_split_fn);
    vm.register_function(b"strrchr", strrchr);
    vm.register_function(b"strstr", strstr_fn);
    vm.register_function(b"stristr", stristr);
    vm.register_function(b"strpbrk", strpbrk_fn);
    vm.register_function(b"strnatcmp", strnatcmp_fn);
    vm.register_function(b"strnatcasecmp", strnatcasecmp_fn);
    vm.register_function(b"strcmp", strcmp_fn);
    vm.register_function(b"strncmp", strncmp_fn);
    vm.register_function(b"strcasecmp", strcasecmp_fn);
    vm.register_function(b"strncasecmp", strncasecmp_fn);
    vm.register_function(b"vprintf", vprintf_fn);
    vm.register_function(b"printf", printf_fn);
    vm.register_function(b"strtok", strtok_fn);
    vm.register_function(b"strspn", strspn);
    vm.register_function(b"strcspn", strcspn);
    vm.register_function(b"vsprintf", vsprintf);
    vm.register_function(b"substr_count", substr_count);
    vm.register_function(b"str_ireplace", str_ireplace);
    vm.register_function(b"wordwrap", wordwrap);
    vm.register_function(b"strrpos", strrpos);
    vm.register_function(b"stripos", stripos);
    vm.register_function(b"strripos", strripos);
    vm.register_function(b"substr_replace", substr_replace);
    vm.register_function(b"pack", pack_fn);
    vm.register_function(b"unpack", unpack_fn);
    vm.register_function(b"hex2bin", hex2bin);
    vm.register_function(b"bin2hex", bin2hex);
    vm.register_function(b"crc32", crc32_fn);
    vm.register_function(b"str_increment", str_increment);
    vm.register_function(b"str_decrement", str_decrement);
    vm.register_function(b"quotemeta", quotemeta_fn);
    vm.register_function(b"utf8_decode", utf8_decode_fn);
    vm.register_function(b"utf8_encode", utf8_encode_fn);
    vm.register_function(b"get_html_translation_table", get_html_translation_table_fn);
    vm.register_function(b"html_entity_decode", html_entity_decode_fn);
    vm.register_function(b"strip_tags", strip_tags_fn);
    vm.register_function(b"nl2br", nl2br_fn);
    vm.register_function(b"str_getcsv", str_getcsv_fn);
    vm.register_function(b"str_word_count", str_word_count_fn);
    vm.register_function(b"convert_uuencode", convert_uuencode_fn);
    vm.register_function(b"convert_uudecode", convert_uudecode_fn);
    vm.register_function(b"quoted_printable_encode", quoted_printable_encode_fn);
    vm.register_function(b"quoted_printable_decode", quoted_printable_decode_fn);
    vm.register_function(b"strcoll", strcoll_fn);
    vm.register_function(b"money_format", money_format_fn);
    vm.register_function(b"settype", settype_fn);
    vm.register_function(b"crypt", crypt_fn);
    vm.register_function(b"password_hash", password_hash_fn);
    vm.register_function(b"password_verify", password_verify_fn);
    vm.register_function(b"password_get_info", password_get_info_fn);
    vm.register_function(b"password_needs_rehash", password_needs_rehash_fn);
    vm.register_function(b"password_algos", password_algos_fn);

    // Register parameter types for strict_types enforcement on builtins
    let s = || Some(ParamType::Simple(b"string".to_vec()));
    let i = || Some(ParamType::Simple(b"int".to_vec()));
    vm.register_builtin_param_types(b"strlen", vec![s()]);
    vm.register_builtin_param_types(b"ord", vec![s()]);
    vm.register_builtin_param_types(b"chr", vec![i()]);
    vm.register_builtin_param_types(b"strtolower", vec![s()]);
    vm.register_builtin_param_types(b"strtoupper", vec![s()]);
    vm.register_builtin_param_types(b"trim", vec![s(), s()]);
    vm.register_builtin_param_types(b"ltrim", vec![s(), s()]);
    vm.register_builtin_param_types(b"rtrim", vec![s(), s()]);
    vm.register_builtin_param_types(b"chop", vec![s(), s()]);
    vm.register_builtin_param_types(b"substr", vec![s(), i(), i()]);
    vm.register_builtin_param_types(b"strpos", vec![s(), s(), i()]);
    vm.register_builtin_param_types(b"str_contains", vec![s(), s()]);
    vm.register_builtin_param_types(b"str_starts_with", vec![s(), s()]);
    vm.register_builtin_param_types(b"str_ends_with", vec![s(), s()]);
    vm.register_builtin_param_types(b"str_repeat", vec![s(), i()]);
    vm.register_builtin_param_types(b"explode", vec![s(), s(), i()]);
    vm.register_builtin_param_types(b"implode", vec![s(), None]);
    vm.register_builtin_param_types(b"join", vec![s(), None]);
    vm.register_builtin_param_types(b"nl2br", vec![s(), None]);
    vm.register_builtin_param_types(b"str_pad", vec![s(), i(), s(), i()]);
    vm.register_builtin_param_types(b"ucfirst", vec![s()]);
    vm.register_builtin_param_types(b"lcfirst", vec![s()]);
    vm.register_builtin_param_types(b"ucwords", vec![s(), s()]);
    vm.register_builtin_param_types(b"strrev", vec![s()]);
    vm.register_builtin_param_types(b"addslashes", vec![s()]);
    vm.register_builtin_param_types(b"stripslashes", vec![s()]);
    vm.register_builtin_param_types(b"addcslashes", vec![s(), s()]);
    vm.register_builtin_param_types(b"stripcslashes", vec![s()]);
    vm.register_builtin_param_types(b"str_rot13", vec![s()]);
    vm.register_builtin_param_types(b"str_word_count", vec![s(), i(), s()]);
    vm.register_builtin_param_types(b"strcmp", vec![s(), s()]);
    vm.register_builtin_param_types(b"strncmp", vec![s(), s(), i()]);
    vm.register_builtin_param_types(b"strcasecmp", vec![s(), s()]);
    vm.register_builtin_param_types(b"strncasecmp", vec![s(), s(), i()]);
    vm.register_builtin_param_types(b"substr_count", vec![s(), s(), i(), i()]);
    vm.register_builtin_param_types(b"substr_compare", vec![s(), s(), i(), i(), None]);
    vm.register_builtin_param_types(b"strstr", vec![s(), s(), None]);
    vm.register_builtin_param_types(b"stristr", vec![s(), s(), None]);
    vm.register_builtin_param_types(b"strrchr", vec![s(), s()]);
    vm.register_builtin_param_types(b"strrpos", vec![s(), s(), i()]);
    vm.register_builtin_param_types(b"stripos", vec![s(), s(), i()]);
    vm.register_builtin_param_types(b"strripos", vec![s(), s(), i()]);
    vm.register_builtin_param_types(b"hex2bin", vec![s()]);
    vm.register_builtin_param_types(b"bin2hex", vec![s()]);
    vm.register_builtin_param_types(b"str_split", vec![s(), i()]);
    vm.register_builtin_param_types(b"str_shuffle", vec![s()]);
    vm.register_builtin_param_types(b"soundex", vec![s()]);
    vm.register_builtin_param_types(b"metaphone", vec![s(), i()]);
    vm.register_builtin_param_types(b"quotemeta", vec![s()]);
    vm.register_builtin_param_types(b"str_increment", vec![s()]);
    vm.register_builtin_param_types(b"str_decrement", vec![s()]);
}

fn strlen(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    Ok(Value::Long(s.len() as i64))
}

fn strtolower(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let lower: Vec<u8> = s
        .as_bytes()
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect();
    Ok(Value::String(PhpString::from_vec(lower)))
}

fn strtoupper(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let upper: Vec<u8> = s
        .as_bytes()
        .iter()
        .map(|b| b.to_ascii_uppercase())
        .collect();
    Ok(Value::String(PhpString::from_vec(upper)))
}

/// Expand PHP charlist ranges like "A..Z" → all chars A-Z
fn expand_charlist(charlist: &[u8]) -> Vec<u8> {
    expand_charlist_inner(charlist, None, "")
}

/// Expand PHP charlist ranges with optional warning emission
fn expand_charlist_warn(charlist: &[u8], vm: &mut Vm, func_name: &str) -> Vec<u8> {
    let vm_ptr = vm as *mut Vm;
    expand_charlist_inner(charlist, Some(vm_ptr), func_name)
}

fn expand_charlist_inner(charlist: &[u8], vm_ptr: Option<*mut Vm>, func_name: &str) -> Vec<u8> {
    let mut result = Vec::new();
    let mut i = 0;
    while i < charlist.len() {
        if i + 1 < charlist.len()
            && charlist[i] == b'.'
            && i + 2 < charlist.len()
            && charlist[i + 1] == b'.'
        {
            // ".." at start - no character to the left
            if i == 0 {
                if let Some(vm_ptr) = vm_ptr {
                    let vm = unsafe { &mut *vm_ptr };
                    vm.emit_warning(&format!("{}(): Invalid '..'-range, no character to the left of '..'", func_name));
                }
                // Skip the ".." and the character after
                if i + 2 < charlist.len() {
                    i += 3;
                } else {
                    i += 2;
                }
                continue;
            }
        }
        if i + 2 < charlist.len()
            && charlist[i + 1] == b'.'
            && charlist[i + 2] == b'.'
        {
            if i + 3 >= charlist.len() {
                // ".." at end - no character to the right
                if let Some(vm_ptr) = vm_ptr {
                    let vm = unsafe { &mut *vm_ptr };
                    vm.emit_warning(&format!("{}(): Invalid '..'-range, no character to the right of '..'", func_name));
                }
                result.push(charlist[i]);
                i += 3;
                continue;
            }
            // Range: X..Y
            let from = charlist[i];
            let to = charlist[i + 3];
            if from <= to {
                for c in from..=to {
                    result.push(c);
                }
            } else {
                // Invalid range - from > to
                if let Some(vm_ptr) = vm_ptr {
                    let vm = unsafe { &mut *vm_ptr };
                    vm.emit_warning(&format!("{}(): Invalid '..'-range, '..'-range needs to be incrementing", func_name));
                }
                result.push(from);
            }
            i += 4;
            // Check if this is followed by another ".." immediately (like "a..b..c")
            if i < charlist.len() && i + 1 < charlist.len()
                && charlist[i] == b'.'
                && i + 1 < charlist.len()
                && charlist[i + 1] == b'.'
            {
                if let Some(vm_ptr) = vm_ptr {
                    let vm = unsafe { &mut *vm_ptr };
                    vm.emit_warning(&format!("{}(): Invalid '..'-range", func_name));
                }
                // Skip the extra ".." and any trailing char
                i += 2;
                if i < charlist.len() {
                    i += 1;
                }
            }
        } else {
            result.push(charlist[i]);
            i += 1;
        }
    }
    result
}

fn trim(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = s.as_bytes();
    let chars = args
        .get(1)
        .map(|v| {
            let s = v.to_php_string();
            expand_charlist_warn(s.as_bytes(), vm, "trim")
        })
        .unwrap_or_else(|| b" \t\n\r\0\x0B".to_vec());

    let start = bytes
        .iter()
        .position(|b| !chars.contains(b))
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|b| !chars.contains(b))
        .map(|i| i + 1)
        .unwrap_or(start);
    Ok(Value::String(PhpString::from_vec(
        bytes[start..end].to_vec(),
    )))
}

fn ltrim(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = s.as_bytes();
    let chars = args
        .get(1)
        .map(|v| {
            let s = v.to_php_string();
            expand_charlist_warn(s.as_bytes(), vm, "ltrim")
        })
        .unwrap_or_else(|| b" \t\n\r\0\x0B".to_vec());
    let start = bytes
        .iter()
        .position(|b| !chars.contains(b))
        .unwrap_or(bytes.len());
    Ok(Value::String(PhpString::from_vec(bytes[start..].to_vec())))
}

fn rtrim(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = s.as_bytes();
    let chars = args
        .get(1)
        .map(|v| {
            let s = v.to_php_string();
            expand_charlist_warn(s.as_bytes(), vm, "rtrim")
        })
        .unwrap_or_else(|| b" \t\n\r\0\x0B".to_vec());
    let end = bytes
        .iter()
        .rposition(|b| !chars.contains(b))
        .map(|i| i + 1)
        .unwrap_or(0);
    Ok(Value::String(PhpString::from_vec(bytes[..end].to_vec())))
}

fn substr(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = s.as_bytes();
    let len = bytes.len() as i64;

    let mut start = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    if start < 0 {
        start = (len + start).max(0);
    }
    let start = start.min(len) as usize;

    let length = args.get(2).and_then(|v| if matches!(v, Value::Null) { None } else { Some(v.to_long()) });
    let end = match length {
        Some(l) if l < 0 => {
            let end_pos = len + l;
            if end_pos <= start as i64 { start } else { end_pos as usize }
        }
        Some(l) if l >= 0 => {
            let end_pos = start as i64 + l;
            if end_pos > bytes.len() as i64 { bytes.len() } else { end_pos as usize }
        }
        Some(_) => start, // shouldn't happen but safety fallback
        None => bytes.len(),
    };

    if start >= bytes.len() || start >= end {
        return Ok(Value::String(PhpString::empty()));
    }

    Ok(Value::String(PhpString::from_vec(
        bytes[start..end].to_vec(),
    )))
}

fn strpos(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let haystack = args.first().unwrap_or(&Value::Null).to_php_string();
    let needle = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let offset_val = args.get(2).map(|v| v.to_long()).unwrap_or(0);

    let h = haystack.as_bytes();
    let n = needle.as_bytes();

    // Handle negative offset
    let offset = if offset_val < 0 {
        let abs = (-offset_val) as usize;
        if abs > h.len() {
            let msg = "strpos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)";
            let exc = vm.create_exception(b"ValueError", msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg.into(), line: vm.current_line });
        }
        h.len() - abs
    } else {
        let o = offset_val as usize;
        if o > h.len() {
            let msg = "strpos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)";
            let exc = vm.create_exception(b"ValueError", msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg.into(), line: vm.current_line });
        }
        o
    };

    if n.is_empty() {
        return Ok(Value::Long(offset as i64));
    }

    if offset >= h.len() || n.len() > h.len() {
        return Ok(Value::False);
    }

    let end = h.len() - n.len();
    if offset > end {
        return Ok(Value::False);
    }

    for i in offset..=end {
        if &h[i..i + n.len()] == n {
            return Ok(Value::Long(i as i64));
        }
    }

    Ok(Value::False)
}

fn str_contains(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let haystack = args.first().unwrap_or(&Value::Null).to_php_string();
    let needle = args.get(1).unwrap_or(&Value::Null).to_php_string();

    if needle.is_empty() {
        return Ok(Value::True);
    }

    let h = haystack.as_bytes();
    let n = needle.as_bytes();

    if n.len() > h.len() {
        return Ok(Value::False);
    }

    for i in 0..=(h.len() - n.len()) {
        if &h[i..i + n.len()] == n {
            return Ok(Value::True);
        }
    }

    Ok(Value::False)
}

fn str_starts_with(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let haystack = args.first().unwrap_or(&Value::Null).to_php_string();
    let prefix = args.get(1).unwrap_or(&Value::Null).to_php_string();
    Ok(if haystack.as_bytes().starts_with(prefix.as_bytes()) {
        Value::True
    } else {
        Value::False
    })
}

fn str_ends_with(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let haystack = args.first().unwrap_or(&Value::Null).to_php_string();
    let suffix = args.get(1).unwrap_or(&Value::Null).to_php_string();
    Ok(if haystack.as_bytes().ends_with(suffix.as_bytes()) {
        Value::True
    } else {
        Value::False
    })
}

fn str_repeat(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let times = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    if times < 0 {
        let msg = "str_repeat(): Argument #2 ($times) must be greater than or equal to 0".to_string();
        let exc = vm.throw_type_error(msg.clone());
        if let Value::Object(obj) = &exc {
            obj.borrow_mut().class_name = b"ValueError".to_vec();
        }
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    let total_len = s.len().saturating_mul(times as usize);
    if total_len > 128 * 1024 * 1024 {
        // 128MB string limit
        return Ok(Value::String(PhpString::empty()));
    }
    let repeated = s.as_bytes().repeat(times as usize);
    Ok(Value::String(PhpString::from_vec(repeated)))
}

/// Helper: single string search/replace, returns (result, count_of_replacements)
fn str_replace_single(haystack: &[u8], needle: &[u8], replacement: &[u8]) -> (Vec<u8>, i64) {
    if needle.is_empty() {
        return (haystack.to_vec(), 0);
    }
    let mut result = Vec::new();
    let mut count = 0i64;
    let mut i = 0;
    while i < haystack.len() {
        if i + needle.len() <= haystack.len() && &haystack[i..i + needle.len()] == needle {
            result.extend_from_slice(replacement);
            i += needle.len();
            count += 1;
        } else {
            result.push(haystack[i]);
            i += 1;
        }
    }
    (result, count)
}

fn str_replace(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let search_val = args.first().unwrap_or(&Value::Null);
    let replace_val = args.get(1).unwrap_or(&Value::Null);
    let subject_val = args.get(2).unwrap_or(&Value::Null);
    let count_ref = args.get(3);

    let mut total_count = 0i64;

    // Build search/replace pairs
    let pairs: Vec<(PhpString, PhpString)> = match search_val {
        Value::Array(search_arr) => {
            let search_arr = search_arr.borrow();
            let is_replace_array = matches!(replace_val, Value::Array(_));
            let replace_values: Vec<PhpString> = match replace_val {
                Value::Array(replace_arr) => {
                    let replace_arr = replace_arr.borrow();
                    replace_arr.values().map(|v| v.to_php_string()).collect()
                }
                _ => vec![replace_val.to_php_string()],
            };
            search_arr
                .values()
                .enumerate()
                .map(|(i, sv)| {
                    let rv = if is_replace_array {
                        replace_values.get(i).cloned().unwrap_or_else(PhpString::empty)
                    } else {
                        // When replace is a scalar, use it for all search values
                        replace_values[0].clone()
                    };
                    (sv.to_php_string(), rv)
                })
                .collect()
        }
        _ => {
            vec![(search_val.to_php_string(), replace_val.to_php_string())]
        }
    };

    let result = match subject_val {
        Value::Array(subject_arr) => {
            let subject_arr = subject_arr.borrow();
            let mut result_arr = PhpArray::new();
            for (key, val) in subject_arr.iter() {
                let mut current = val.to_php_string().as_bytes().to_vec();
                for (needle, replacement) in &pairs {
                    let (new_val, cnt) = str_replace_single(&current, needle.as_bytes(), replacement.as_bytes());
                    total_count += cnt;
                    current = new_val;
                }
                result_arr.set(key.clone(), Value::String(PhpString::from_vec(current)));
            }
            Value::Array(Rc::new(RefCell::new(result_arr)))
        }
        _ => {
            let mut current = subject_val.to_php_string().as_bytes().to_vec();
            for (needle, replacement) in &pairs {
                let (new_val, cnt) = str_replace_single(&current, needle.as_bytes(), replacement.as_bytes());
                total_count += cnt;
                current = new_val;
            }
            Value::String(PhpString::from_vec(current))
        }
    };

    // Set count if provided as reference
    if let Some(Value::Reference(r)) = count_ref {
        *r.borrow_mut() = Value::Long(total_count);
    }

    Ok(result)
}

fn explode(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    use goro_core::array::PhpArray;
    use std::cell::RefCell;
    use std::rc::Rc;

    let delimiter = args.first().unwrap_or(&Value::Null).to_php_string();
    let string = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let limit = args.get(2).map(|v| v.to_long()).unwrap_or(i64::MAX);

    let d = delimiter.as_bytes();
    let s = string.as_bytes();

    if d.is_empty() {
        let msg = "explode(): Argument #1 ($separator) must not be empty";
        let exc = _vm.create_exception(b"ValueError", msg, 0);
        _vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg.into(),
            line: 0,
        });
    }

    // First, split without limit to get all pieces
    let mut pieces: Vec<Vec<u8>> = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i + d.len() <= s.len() {
        if &s[i..i + d.len()] == d {
            pieces.push(s[start..i].to_vec());
            i += d.len();
            start = i;
        } else {
            i += 1;
        }
    }
    pieces.push(s[start..].to_vec());

    let mut arr = PhpArray::new();
    if limit > 0 {
        let limit = limit as usize;
        if limit >= pieces.len() {
            for p in pieces {
                arr.push(Value::String(PhpString::from_vec(p)));
            }
        } else {
            // Return first (limit-1) pieces, then rest joined
            for p in &pieces[..limit - 1] {
                arr.push(Value::String(PhpString::from_vec(p.clone())));
            }
            // Join remaining pieces with delimiter
            let mut rest = Vec::new();
            for (j, p) in pieces[limit - 1..].iter().enumerate() {
                if j > 0 {
                    rest.extend_from_slice(d);
                }
                rest.extend_from_slice(p);
            }
            arr.push(Value::String(PhpString::from_vec(rest)));
        }
    } else if limit < 0 {
        // Negative limit: return all except last -limit elements
        let drop = (-limit) as usize;
        if drop >= pieces.len() {
            // Return empty array - but PHP returns array with empty string? No, empty array
            // Actually PHP returns empty array when limit drops all
        } else {
            for p in &pieces[..pieces.len() - drop] {
                arr.push(Value::String(PhpString::from_vec(p.clone())));
            }
        }
    } else {
        // limit == 0: treated as 1
        arr.push(Value::String(PhpString::from_vec(s.to_vec())));
    }

    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

fn implode(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let (glue, pieces) = if args.len() >= 2 {
        // Check if first arg is array (implode(array) or implode(array, separator))
        if matches!(&args[0], Value::Array(_)) && !matches!(&args[1], Value::Array(_)) {
            // implode(array, separator) - wrong order, PHP accepts it
            (args[1].to_php_string(), &args[0])
        } else {
            (args[0].to_php_string(), &args[1])
        }
    } else if let Some(first) = args.first() {
        if matches!(first, Value::Array(_)) {
            // implode(array) - no glue
            (PhpString::empty(), first)
        } else {
            // implode(string) with no array - this is an error in PHP 8
            let type_name = Vm::value_type_name(first);
            if matches!(first, Value::String(_)) {
                let msg = "implode(): If argument #1 ($separator) is of type string, argument #2 ($array) must be of type array, null given".to_string();
                let exc = vm.create_exception(b"TypeError", &msg, 0);
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg, line: vm.current_line });
            } else {
                let msg = format!("implode(): Argument #1 ($separator) must be of type string|array, {} given", type_name);
                let exc = vm.create_exception(b"TypeError", &msg, 0);
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg, line: vm.current_line });
            }
        }
    } else {
        let msg = "implode() expects at least 1 argument, 0 given".to_string();
        let exc = vm.create_exception(b"ArgumentCountError", &msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    };

    match pieces {
        Value::Array(arr) => {
            let values: Vec<Value> = {
                let arr = arr.borrow();
                arr.values().cloned().collect()
            };
            let parts: Vec<Vec<u8>> = values
                .iter()
                .map(|v| vm.value_to_string(v).as_bytes().to_vec())
                .collect();
            let mut result = Vec::new();
            for (i, part) in parts.iter().enumerate() {
                if i > 0 {
                    result.extend_from_slice(glue.as_bytes());
                }
                result.extend_from_slice(part);
            }
            Ok(Value::String(PhpString::from_vec(result)))
        }
        Value::Null | Value::Undef => {
            // Two-arg case: implode(glue, null) - deprecated in PHP 8.x
            vm.emit_deprecated_at(
                "implode(): Passing null to parameter #2 ($array) of type array is deprecated",
                vm.current_line,
            );
            Ok(Value::String(PhpString::empty()))
        }
        _ => {
            let type_name = match pieces {
                Value::True | Value::False => "bool",
                Value::Long(_) => "int",
                Value::Double(_) => "float",
                Value::String(_) => "string",
                Value::Object(_) => "object",
                _ => "unknown",
            };
            let msg = format!("implode(): Argument #2 ($array) must be of type ?array, {} given", type_name);
            let exc = vm.create_exception(b"TypeError", &msg, 0);
            vm.current_exception = Some(exc);
            Err(VmError { message: msg, line: vm.current_line })
        }
    }
}

fn chr(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        let msg = "chr() expects exactly 1 argument, 0 given";
        let exc = vm.create_exception(b"TypeError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg.into(), line: vm.current_line });
    }
    if args.len() > 1 {
        let msg = format!("chr() expects exactly 1 argument, {} given", args.len());
        let exc = vm.create_exception(b"TypeError", &msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    let n = args[0].to_long();
    if n < 0 || n > 255 {
        vm.emit_deprecated_at(
            "chr(): Providing a value not in-between 0 and 255 is deprecated, this is because a byte value must be in the [0, 255] interval. The value used will be constrained using % 256",
            vm.current_line,
        );
    }
    let code = ((n % 256 + 256) % 256) as u8;
    Ok(Value::String(PhpString::from_bytes(&[code])))
}

fn ord(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let code = s.as_bytes().first().copied().unwrap_or(0);
    Ok(Value::Long(code as i64))
}

/// Validate that the format argument to sprintf/printf is a string (not array/resource/non-Stringable object)
fn validate_sprintf_format_arg(vm: &mut Vm, func_name: &str, format_arg: Option<&Value>) -> Result<(), VmError> {
    if let Some(arg) = format_arg {
        match arg {
            Value::Array(_) => {
                let msg = format!("{}(): Argument #1 ($format) must be of type string, array given", func_name);
                let exc = vm.create_exception(b"TypeError", &msg, 0);
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg, line: vm.current_line });
            }
            Value::Object(obj) => {
                // Check if the object implements __toString (Stringable)
                let class_name = obj.borrow().class_name.clone();
                let class_lower: Vec<u8> = class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                let has_tostring = vm.classes.get(&class_lower)
                    .map(|c| c.get_method(b"__tostring").is_some())
                    .unwrap_or(false);
                if !has_tostring {
                    let class_display = String::from_utf8_lossy(&class_name).to_string();
                    let msg = format!("{}(): Argument #1 ($format) must be of type string, {} given", func_name, class_display);
                    let exc = vm.create_exception(b"TypeError", &msg, 0);
                    vm.current_exception = Some(exc);
                    return Err(VmError { message: msg, line: vm.current_line });
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Count the number of format specifiers in a sprintf format string and validate arg count
fn validate_sprintf_arg_count(vm: &mut Vm, _func_name: &str, args: &[Value]) -> Result<(), VmError> {
    if args.is_empty() {
        return Ok(());
    }
    let format = args[0].to_php_string();
    let format_bytes = format.as_bytes();
    let mut max_arg = 0usize;
    let mut seq_arg = 0usize;
    let mut i = 0;

    while i < format_bytes.len() {
        if format_bytes[i] == b'%' {
            i += 1;
            if i >= format_bytes.len() {
                // % at end of string with no specifier
                let msg = "Missing format specifier at end of string";
                let exc = vm.create_exception(b"ValueError", msg, 0);
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg.into(), line: vm.current_line });
            }
            if format_bytes[i] == b'%' {
                i += 1;
                continue;
            }
            // Check for position specifier N$
            let save_i = i;
            let mut num = 0usize;
            let mut has_num = false;
            while i < format_bytes.len() && format_bytes[i].is_ascii_digit() {
                num = num * 10 + (format_bytes[i] - b'0') as usize;
                has_num = true;
                i += 1;
            }
            if has_num && i < format_bytes.len() && format_bytes[i] == b'$' {
                if num > max_arg {
                    max_arg = num;
                }
                i += 1;
            } else {
                i = save_i;
                seq_arg += 1;
                if seq_arg > max_arg {
                    max_arg = seq_arg;
                }
            }
            // Skip flags, width, precision, length modifiers, and type
            while i < format_bytes.len() && matches!(format_bytes[i], b'-' | b'+' | b' ' | b'0' | b'\'') {
                if format_bytes[i] == b'\'' {
                    i += 1; // skip pad char
                }
                i += 1;
            }
            // Width
            while i < format_bytes.len() && format_bytes[i].is_ascii_digit() {
                i += 1;
            }
            // Precision
            if i < format_bytes.len() && format_bytes[i] == b'.' {
                i += 1;
                while i < format_bytes.len() && format_bytes[i].is_ascii_digit() {
                    i += 1;
                }
            }
            // Length modifiers
            while i < format_bytes.len() && matches!(format_bytes[i], b'l' | b'h' | b'L' | b'q' | b'j' | b'z' | b't') {
                i += 1;
            }
            // Type specifier - if missing, it's an error
            if i >= format_bytes.len() {
                let msg = "Missing format specifier at end of string";
                let exc = vm.create_exception(b"ValueError", msg, 0);
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg.into(), line: vm.current_line });
            }
            i += 1;
        } else {
            i += 1;
        }
    }

    let available = args.len() - 1; // subtract the format arg
    if max_arg > available {
        let msg = format!("{} arguments are required, {} given", max_arg + 1, available + 1);
        let exc = vm.create_exception(b"ArgumentCountError", &msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    Ok(())
}

fn sprintf(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        let msg = "sprintf() expects at least 1 argument, 0 given";
        let exc = vm.create_exception(b"ArgumentCountError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg.into(), line: vm.current_line });
    }
    // TypeError for non-string format argument (array, resource)
    validate_sprintf_format_arg(vm, "sprintf", args.first())?;
    // Check argument count
    validate_sprintf_arg_count(vm, "sprintf", args)?;
    let result = do_sprintf(args);
    Ok(Value::String(PhpString::from_vec(result.into_bytes())))
}

/// Shared sprintf implementation used by both sprintf() and printf()
pub fn do_sprintf(args: &[Value]) -> String {
    if args.is_empty() {
        return String::new();
    }
    let format = args[0].to_php_string();
    let format_bytes = format.as_bytes();

    let mut result = String::new();
    let mut arg_idx = 1;
    let mut i = 0;

    while i < format_bytes.len() {
        if format_bytes[i] == b'%' {
            i += 1;
            if i >= format_bytes.len() {
                break;
            }
            if format_bytes[i] == b'%' {
                result.push('%');
                i += 1;
                continue;
            }

            // Parse format specifier: %[argnum$][flags][width][.precision]type
            // Check for argument position: %N$ (e.g., %1$s, %2$d)
            let mut use_arg_idx = arg_idx;
            {
                let save_i = i;
                let mut num = 0usize;
                let mut has_num = false;
                while i < format_bytes.len() && format_bytes[i].is_ascii_digit() {
                    num = num * 10 + (format_bytes[i] - b'0') as usize;
                    has_num = true;
                    i += 1;
                }
                if has_num && i < format_bytes.len() && format_bytes[i] == b'$' {
                    use_arg_idx = num; // 1-based maps directly to args[] since args[0] is format string
                    i += 1; // skip $
                } else {
                    i = save_i; // not a position specifier, backtrack
                }
            }

            // Flags: -, +, space, 0, '
            let mut pad_char = b' ';
            let mut left_align = false;
            let mut show_sign = false;
            let mut pad_zero = false;

            // Flags
            loop {
                if i >= format_bytes.len() {
                    break;
                }
                match format_bytes[i] {
                    b'-' => {
                        left_align = true;
                        i += 1;
                    }
                    b'+' => {
                        show_sign = true;
                        i += 1;
                    }
                    b'0' => {
                        pad_zero = true;
                        pad_char = b'0';
                        i += 1;
                    }
                    b' ' => {
                        i += 1;
                    } // space flag
                    b'\'' => {
                        i += 1;
                        if i < format_bytes.len() {
                            pad_char = format_bytes[i];
                            i += 1;
                        }
                    }
                    _ => break,
                }
            }

            // Width (cap at 1MB to prevent OOM)
            let mut width: usize = 0;
            while i < format_bytes.len() && format_bytes[i].is_ascii_digit() {
                width = width.saturating_mul(10).saturating_add((format_bytes[i] - b'0') as usize);
                i += 1;
            }
            if width > 1_000_000 {
                width = 1_000_000;
            }

            // Precision (cap at 1MB to prevent OOM)
            let mut precision: Option<usize> = None;
            if i < format_bytes.len() && format_bytes[i] == b'.' {
                i += 1;
                let mut prec: usize = 0;
                while i < format_bytes.len() && format_bytes[i].is_ascii_digit() {
                    prec = prec.saturating_mul(10).saturating_add((format_bytes[i] - b'0') as usize);
                    i += 1;
                }
                if prec > 4096 {
                    prec = 4096;
                }
                precision = Some(prec);
            }

            if i >= format_bytes.len() {
                break;
            }
            // Skip length modifiers (l, ll, h, etc.) - PHP ignores them
            while i < format_bytes.len() && matches!(format_bytes[i], b'l' | b'h' | b'L' | b'q' | b'j' | b'z' | b't') {
                i += 1;
            }
            if i >= format_bytes.len() {
                break;
            }
            let spec = format_bytes[i];
            i += 1;

            let arg = args.get(use_arg_idx).unwrap_or(&Value::Null);
            if use_arg_idx == arg_idx {
                arg_idx += 1;
            }

            let formatted = match spec {
                b's' => {
                    let s = arg.to_php_string().to_string_lossy();
                    if let Some(prec) = precision {
                        s.chars().take(prec).collect::<String>()
                    } else {
                        s
                    }
                }
                b'd' => {
                    let n = arg.to_long();
                    if show_sign && n >= 0 {
                        format!("+{}", n)
                    } else {
                        n.to_string()
                    }
                }
                b'f' | b'F' => {
                    let f = arg.to_double();
                    let prec = precision.unwrap_or(6);
                    let s = format!("{:.prec$}", f);
                    if show_sign && !s.starts_with('-') && !f.is_nan() {
                        format!("+{}", s)
                    } else {
                        s
                    }
                }
                b'e' => {
                    let f = arg.to_double();
                    let prec = precision.unwrap_or(6);
                    let s = format!("{:.prec$e}", f);
                    // PHP always shows +/- in exponent, Rust doesn't show +
                    let s = php_fix_exponent(&s);
                    if show_sign && !s.starts_with('-') && !f.is_nan() {
                        format!("+{}", s)
                    } else {
                        s
                    }
                }
                b'E' => {
                    let f = arg.to_double();
                    let prec = precision.unwrap_or(6);
                    let s = format!("{:.prec$E}", f);
                    let s = php_fix_exponent(&s);
                    if show_sign && !s.starts_with('-') && !f.is_nan() {
                        format!("+{}", s)
                    } else {
                        s
                    }
                }
                b'g' | b'G' => {
                    let f = arg.to_double();
                    let prec = if precision == Some(0) { 1 } else { precision.unwrap_or(6) };
                    // PHP %g: use shorter of %e and %f, with significant digits
                    // The precision specifies number of significant digits
                    let s = if f == 0.0 {
                        if f.is_sign_negative() {
                            "-0".to_string()
                        } else {
                            "0".to_string()
                        }
                    } else if f.is_nan() {
                        "NAN".to_string()
                    } else if f.is_infinite() {
                        if f > 0.0 { "INF".to_string() } else { "-INF".to_string() }
                    } else {
                        // Use Rust's formatting with significant digits
                        let abs = f.abs();
                        let exp = abs.log10().floor() as i32;
                        if exp >= -(1 as i32) && exp < prec as i32 {
                            // Use fixed notation
                            let decimal_digits = if prec as i32 > exp + 1 { (prec as i32 - exp - 1) as usize } else { 0 };
                            let s = format!("{:.decimal_digits$}", f);
                            // Remove trailing zeros after decimal point (PHP %g behavior)
                            if s.contains('.') {
                                let s = s.trim_end_matches('0');
                                let s = s.trim_end_matches('.');
                                s.to_string()
                            } else {
                                s
                            }
                        } else {
                            // Use scientific notation
                            let decimal_digits = if prec > 1 { prec - 1 } else { 0 };
                            let s = format!("{:.decimal_digits$e}", f);
                            let s = php_fix_exponent(&s);
                            // Remove trailing zeros in mantissa
                            if let Some(e_pos) = s.find(|c: char| c == 'e' || c == 'E') {
                                let mantissa = &s[..e_pos];
                                let exponent = &s[e_pos..];
                                if mantissa.contains('.') {
                                    let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
                                    format!("{}{}", mantissa, exponent)
                                } else {
                                    s
                                }
                            } else {
                                s
                            }
                        }
                    };
                    let s = if spec == b'G' { s.to_uppercase() } else { s };
                    if show_sign && !s.starts_with('-') && !f.is_nan() {
                        format!("+{}", s)
                    } else {
                        s
                    }
                }
                b'x' => format!("{:x}", arg.to_long()),
                b'X' => format!("{:X}", arg.to_long()),
                b'o' => format!("{:o}", arg.to_long()),
                b'b' => format!("{:b}", arg.to_long()),
                b'c' => String::from(arg.to_long() as u8 as char),
                b'u' => format!("{}", arg.to_long() as u64),
                b'%' => {
                    // `%` as type specifier (after flags/width): output literal `%` and consume arg
                    "%".to_string()
                }
                _ => {
                    arg_idx -= 1;
                    format!("%{}", spec as char)
                }
            };

            // Apply width and padding
            if width > 0 && formatted.len() < width {
                let padding = width - formatted.len();
                if left_align {
                    result.push_str(&formatted);
                    for _ in 0..padding {
                        result.push(pad_char as char);
                    }
                } else {
                    // For zero-padding with sign, put sign before zeros
                    if pad_zero && (formatted.starts_with('-') || formatted.starts_with('+')) {
                        result.push(formatted.chars().next().unwrap());
                        for _ in 0..padding {
                            result.push('0');
                        }
                        result.push_str(&formatted[1..]);
                    } else {
                        for _ in 0..padding {
                            result.push(pad_char as char);
                        }
                        result.push_str(&formatted);
                    }
                }
            } else {
                result.push_str(&formatted);
            }
        } else {
            result.push(format_bytes[i] as char);
            i += 1;
        }
    }

    result
}

/// Fix scientific notation exponent to always show sign (e.g., e2 -> e+2)
fn php_fix_exponent(s: &str) -> String {
    // Find 'e' or 'E' and check if the next char is a digit (no sign)
    if let Some(e_pos) = s.rfind(|c| c == 'e' || c == 'E') {
        let after = &s[e_pos + 1..];
        if !after.is_empty() && after.as_bytes()[0].is_ascii_digit() {
            // Insert '+' after e/E
            format!("{}+{}", &s[..e_pos + 1], after)
        } else {
            s.to_string()
        }
    } else {
        s.to_string()
    }
}

fn nl2br(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let is_xhtml = args.get(1).map(|v| v.is_truthy()).unwrap_or(true);
    let br = if is_xhtml { b"<br />" as &[u8] } else { b"<br>" };
    let bytes = s.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' {
            result.extend_from_slice(br);
            // \r\n is a single newline
            if i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
                result.push(b'\r');
                result.push(b'\n');
                i += 2;
            } else {
                result.push(b'\r');
                i += 1;
            }
        } else if bytes[i] == b'\n' {
            result.extend_from_slice(br);
            // \n\r is also a single newline (rare but PHP handles it)
            if i + 1 < bytes.len() && bytes[i + 1] == b'\r' {
                result.push(b'\n');
                result.push(b'\r');
                i += 2;
            } else {
                result.push(b'\n');
                i += 1;
            }
        } else {
            result.push(bytes[i]);
            i += 1;
        }
    }
    Ok(Value::String(PhpString::from_vec(result)))
}

fn chunk_split(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let body = args.first().unwrap_or(&Value::Null).to_php_string();
    // PHP 8: chunklen must be int, not float
    if let Some(v) = args.get(1) {
        if matches!(v.deref(), Value::Double(_)) {
            let msg = "chunk_split(): Argument #2 ($length) must be of type int, float given".to_string();
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
        }
    }
    let chunklen = args.get(1).map(|v| v.to_long()).unwrap_or(76);
    let end = args
        .get(2)
        .map(|v| v.to_php_string())
        .unwrap_or_else(|| PhpString::from_bytes(b"\r\n"));

    // PHP 8.0+: chunklen must be >= 1 (throws ValueError)
    if chunklen < 1 {
        let msg = "chunk_split(): Argument #2 ($length) must be greater than 0".to_string();
        let exc = vm.throw_type_error(msg.clone());
        if let Value::Object(obj) = &exc {
            obj.borrow_mut().class_name = b"ValueError".to_vec();
        }
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    let chunklen = chunklen as usize;

    let bytes = body.as_bytes();
    if bytes.is_empty() {
        // Empty body returns just the end separator
        return Ok(Value::String(end));
    }
    let mut result = Vec::new();
    for chunk in bytes.chunks(chunklen) {
        result.extend_from_slice(chunk);
        result.extend_from_slice(end.as_bytes());
    }
    Ok(Value::String(PhpString::from_vec(result)))
}

fn str_pad(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let input = args.first().unwrap_or(&Value::Null).to_php_string();
    let length = args.get(1).map(|v| v.to_long()).unwrap_or(0) as usize;
    let pad_string = match args.get(2) {
        Some(Value::Null) | None => PhpString::from_bytes(b" "),
        Some(v) => v.to_php_string(),
    };
    let pad_type = args.get(3).map(|v| v.to_long()).unwrap_or(1); // STR_PAD_RIGHT=1

    let pad_bytes = pad_string.as_bytes();
    if pad_bytes.is_empty() {
        let msg = "str_pad(): Argument #3 ($pad_string) must not be empty";
        let exc = vm.create_exception(b"ValueError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg.to_string(), line: 0 });
    }

    // Validate pad_type
    if pad_type != 0 && pad_type != 1 && pad_type != 2 {
        let msg = "str_pad(): Argument #4 ($pad_type) must be STR_PAD_LEFT, STR_PAD_RIGHT, or STR_PAD_BOTH";
        let exc = vm.create_exception(b"ValueError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg.to_string(), line: 0 });
    }

    let bytes = input.as_bytes();
    if bytes.len() >= length || length > 128 * 1024 * 1024 {
        return Ok(Value::String(input));
    }

    let pad_needed = length - bytes.len();

    let mut padding = Vec::new();
    while padding.len() < pad_needed {
        padding.push(pad_bytes[padding.len() % pad_bytes.len()]);
    }

    let mut result = Vec::with_capacity(length);
    match pad_type {
        0 => {
            // STR_PAD_LEFT
            result.extend_from_slice(&padding);
            result.extend_from_slice(bytes);
        }
        2 => {
            // STR_PAD_BOTH
            let left = pad_needed / 2;
            result.extend_from_slice(&padding[..left]);
            result.extend_from_slice(bytes);
            result.extend_from_slice(&padding[..pad_needed - left]);
        }
        _ => {
            // STR_PAD_RIGHT (default, 1)
            result.extend_from_slice(bytes);
            result.extend_from_slice(&padding);
        }
    }
    Ok(Value::String(PhpString::from_vec(result)))
}

fn str_word_count(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let s_str = s.to_string_lossy();
    let count = s_str.split_whitespace().count();
    Ok(Value::Long(count as i64))
}

fn ucfirst(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let mut bytes = s.as_bytes().to_vec();
    if let Some(first) = bytes.first_mut() {
        *first = first.to_ascii_uppercase();
    }
    Ok(Value::String(PhpString::from_vec(bytes)))
}

fn lcfirst(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let mut bytes = s.as_bytes().to_vec();
    if let Some(first) = bytes.first_mut() {
        *first = first.to_ascii_lowercase();
    }
    Ok(Value::String(PhpString::from_vec(bytes)))
}

fn ucwords(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let delimiters = args.get(1).map(|v| v.to_php_string());
    let mut bytes = s.as_bytes().to_vec();
    let mut capitalize_next = true;
    let delim_bytes: Vec<u8> = if let Some(ref d) = delimiters {
        d.as_bytes().to_vec()
    } else {
        b" \t\r\n\x0B\x0C".to_vec()
    };
    for b in &mut bytes {
        if delim_bytes.contains(b) {
            capitalize_next = true;
        } else if capitalize_next {
            *b = b.to_ascii_uppercase();
            capitalize_next = false;
        }
    }
    Ok(Value::String(PhpString::from_vec(bytes)))
}

fn strrev(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let mut bytes = s.as_bytes().to_vec();
    bytes.reverse();
    Ok(Value::String(PhpString::from_vec(bytes)))
}

fn addslashes(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let mut result = Vec::new();
    for &b in s.as_bytes() {
        match b {
            b'\'' | b'"' | b'\\' => {
                result.push(b'\\');
                result.push(b);
            }
            0 => {
                result.push(b'\\');
                result.push(b'0');
            }
            _ => result.push(b),
        }
    }
    Ok(Value::String(PhpString::from_vec(result)))
}

fn stripslashes(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = s.as_bytes();
    let mut result = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            match bytes[i + 1] {
                b'0' => {
                    result.push(0);
                    i += 2;
                }
                ch => {
                    result.push(ch);
                    i += 2;
                }
            }
        } else {
            result.push(bytes[i]);
            i += 1;
        }
    }
    Ok(Value::String(PhpString::from_vec(result)))
}

fn addcslashes(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let charlist_val = args.get(1).unwrap_or(&Value::Null);
    // TypeError if charlist is an array
    if matches!(charlist_val, Value::Array(_)) {
        let msg = "addcslashes(): Argument #2 ($characters) must be of type string, array given".to_string();
        let exc = vm.throw_type_error(msg.clone());
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: 0,
        });
    }
    let charlist = charlist_val.to_php_string();
    let chars = charlist.as_bytes();
    // Expand ranges (e.g., "a..z" means all chars from a to z)
    let mut char_set = [false; 256];
    let mut i = 0;
    while i < chars.len() {
        if i + 3 < chars.len() && chars[i + 1] == b'.' && chars[i + 2] == b'.' {
            let start = chars[i];
            let end = chars[i + 3];
            if start <= end {
                for c in start..=end {
                    char_set[c as usize] = true;
                }
            } else {
                // Invalid range - warn and set each individual character
                vm.emit_warning(&format!(
                    "addcslashes(): Invalid '..'-range, '..'-range needs to be incrementing"
                ));
                char_set[start as usize] = true;
                char_set[b'.' as usize] = true;
                char_set[end as usize] = true;
            }
            i += 4;
        } else {
            char_set[chars[i] as usize] = true;
            i += 1;
        }
    }
    let mut result = Vec::new();
    for &b in s.as_bytes() {
        if char_set[b as usize] {
            match b {
                b'\n' => result.extend_from_slice(b"\\n"),
                b'\r' => result.extend_from_slice(b"\\r"),
                b'\t' => result.extend_from_slice(b"\\t"),
                0x07 => result.extend_from_slice(b"\\a"), // bell
                0x08 => result.extend_from_slice(b"\\b"), // backspace
                0x0B => result.extend_from_slice(b"\\v"), // vertical tab
                0x0C => result.extend_from_slice(b"\\f"), // form feed
                0x1B => result.extend_from_slice(b"\\e"), // escape
                0 => result.extend_from_slice(b"\\000"),
                _ if b < 0x20 || b == 0x7F => {
                    // Other control chars: use octal
                    result.extend_from_slice(format!("\\{:03o}", b).as_bytes());
                }
                _ => {
                    result.push(b'\\');
                    result.push(b);
                }
            }
        } else {
            result.push(b);
        }
    }
    Ok(Value::String(PhpString::from_vec(result)))
}

fn stripcslashes(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = s.as_bytes();
    let mut result = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            match bytes[i + 1] {
                b'n' => {
                    result.push(b'\n');
                    i += 2;
                }
                b'r' => {
                    result.push(b'\r');
                    i += 2;
                }
                b't' => {
                    result.push(b'\t');
                    i += 2;
                }
                b'v' => {
                    result.push(0x0B);
                    i += 2;
                }
                b'a' => {
                    result.push(0x07);
                    i += 2;
                }
                b'b' => {
                    result.push(0x08);
                    i += 2;
                }
                b'f' => {
                    result.push(0x0C);
                    i += 2;
                }
                b'\\' => {
                    result.push(b'\\');
                    i += 2;
                }
                b'0'..=b'7' => {
                    // Octal
                    let mut oct = vec![bytes[i + 1]];
                    let mut j = i + 2;
                    while j < bytes.len() && oct.len() < 3 && bytes[j] >= b'0' && bytes[j] <= b'7' {
                        oct.push(bytes[j]);
                        j += 1;
                    }
                    let s_oct: String = oct.iter().map(|&b| b as char).collect();
                    result.push(u8::from_str_radix(&s_oct, 8).unwrap_or(0));
                    i = j;
                }
                b'x' => {
                    // Hex
                    let mut j = i + 2;
                    let mut hex = Vec::new();
                    while j < bytes.len() && hex.len() < 2 && bytes[j].is_ascii_hexdigit() {
                        hex.push(bytes[j]);
                        j += 1;
                    }
                    if !hex.is_empty() {
                        let s_hex: String = hex.iter().map(|&b| b as char).collect();
                        result.push(u8::from_str_radix(&s_hex, 16).unwrap_or(0));
                    }
                    i = j;
                }
                ch => {
                    result.push(ch);
                    i += 2;
                }
            }
        } else {
            result.push(bytes[i]);
            i += 1;
        }
    }
    Ok(Value::String(PhpString::from_vec(result)))
}

fn str_rot13(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let result: Vec<u8> = s
        .as_bytes()
        .iter()
        .map(|&b| match b {
            b'a'..=b'm' | b'A'..=b'M' => b + 13,
            b'n'..=b'z' | b'N'..=b'Z' => b - 13,
            _ => b,
        })
        .collect();
    Ok(Value::String(PhpString::from_vec(result)))
}

fn strip_tags(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let allowed = args.get(1);

    // Build set of allowed tag names (lowercase)
    let mut allowed_tags: Vec<Vec<u8>> = Vec::new();
    if let Some(allowed_val) = allowed {
        match allowed_val {
            Value::String(allowed_str) => {
                // Parse "<b><p><i>" format
                let ab = allowed_str.as_bytes();
                let mut j = 0;
                while j < ab.len() {
                    if ab[j] == b'<' {
                        j += 1;
                        let start = j;
                        while j < ab.len() && ab[j] != b'>' && ab[j] != b' ' {
                            j += 1;
                        }
                        if start < j {
                            allowed_tags.push(ab[start..j].to_ascii_lowercase());
                        }
                        while j < ab.len() && ab[j] != b'>' {
                            j += 1;
                        }
                        if j < ab.len() { j += 1; }
                    } else {
                        j += 1;
                    }
                }
            }
            Value::Array(arr) => {
                // Array of tag names ["b", "p", "i"]
                let arr = arr.borrow();
                for (_, v) in arr.iter() {
                    let tag = v.to_php_string().to_string_lossy().to_ascii_lowercase();
                    allowed_tags.push(tag.into_bytes());
                }
            }
            _ => {}
        }
    }

    let bytes = s.as_bytes();
    let mut result = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        // PHP strip_tags strips NUL bytes
        if bytes[i] == 0 {
            i += 1;
            continue;
        }
        if bytes[i] == b'<' {
            // Check if this could be a tag: must be followed by ?, !, /, %, or alpha
            if i + 1 < bytes.len() {
                let next = bytes[i + 1];
                if next != b'?' && next != b'!' && next != b'/' && next != b'%' && !next.is_ascii_alphabetic() {
                    // Not a tag, output as literal
                    result.push(bytes[i]);
                    i += 1;
                    continue;
                }
            } else {
                // < at end of string - output literally
                result.push(bytes[i]);
                i += 1;
                continue;
            }

            // Check for PHP/XML processing instructions and ASP-style tags
            if i + 1 < bytes.len() && (bytes[i + 1] == b'?' || bytes[i + 1] == b'%') {
                // Check if this is an XML processing instruction: <?xml (case-insensitive)
                // <?xml ends at first >, all other <? tags end at ?>
                let is_xml_pi = i + 4 < bytes.len()
                    && bytes[i + 2..i + 5].eq_ignore_ascii_case(b"xml")
                    && (i + 5 >= bytes.len() || !bytes[i + 5].is_ascii_alphanumeric());
                if is_xml_pi {
                    // XML PI - skip until first >
                    i += 2;
                    while i < bytes.len() {
                        if bytes[i] == b'>' {
                            i += 1;
                            break;
                        }
                        i += 1;
                    }
                } else {
                    // PHP/ASP tag - skip until ?> or %> respectively
                    let close_char = bytes[i + 1]; // ? or %
                    i += 2;
                    while i < bytes.len() {
                        if bytes[i] == close_char && i + 1 < bytes.len() && bytes[i + 1] == b'>' {
                            i += 2;
                            break;
                        }
                        i += 1;
                    }
                }
                continue;
            }

            // Check for HTML comments: <!-- ... -->
            if i + 3 < bytes.len() && bytes[i + 1] == b'!' && bytes[i + 2] == b'-' && bytes[i + 3] == b'-' {
                // HTML comment - skip until -->
                i += 4;
                while i < bytes.len() {
                    if bytes[i] == b'-' && i + 2 < bytes.len() && bytes[i + 1] == b'-' && bytes[i + 2] == b'>' {
                        i += 3;
                        break;
                    }
                    i += 1;
                }
                continue;
            }

            // Check for SGML/XML declarations: <! ... >
            if i + 1 < bytes.len() && bytes[i + 1] == b'!' {
                // Skip until >
                i += 2;
                while i < bytes.len() && bytes[i] != b'>' {
                    i += 1;
                }
                if i < bytes.len() { i += 1; }
                continue;
            }

            // Find the end of the tag
            let tag_start = i;
            i += 1;
            let is_closing = i < bytes.len() && bytes[i] == b'/';
            if is_closing { i += 1; }

            // Extract tag name
            let name_start = i;
            while i < bytes.len() && bytes[i] != b'>' && bytes[i] != b' ' && bytes[i] != b'\t' && bytes[i] != b'\n' && bytes[i] != b'\r' && bytes[i] != b'/' {
                i += 1;
            }
            let tag_name = &bytes[name_start..i];
            let tag_name_lower = tag_name.to_ascii_lowercase();

            // Skip to end of tag, handling quotes
            let mut in_quote: u8 = 0;
            while i < bytes.len() {
                if in_quote != 0 {
                    if bytes[i] == in_quote {
                        in_quote = 0;
                    }
                } else if bytes[i] == b'"' || bytes[i] == b'\'' {
                    in_quote = bytes[i];
                } else if bytes[i] == b'>' {
                    break;
                }
                i += 1;
            }
            if i < bytes.len() { i += 1; } // skip >

            // Check if this tag is allowed
            if !allowed_tags.is_empty() && !tag_name_lower.is_empty() && allowed_tags.iter().any(|t| t == &tag_name_lower) {
                result.extend_from_slice(&bytes[tag_start..i]);
            }
            // Otherwise, the tag is stripped (nothing added to result)
        } else {
            result.push(bytes[i]);
            i += 1;
        }
    }
    Ok(Value::String(PhpString::from_vec(result)))
}

fn quoted_printable_encode(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = s.as_bytes();
    let mut result = Vec::new();
    let mut line_len: usize = 0;

    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];

        // Handle CRLF and LF line breaks - output as-is and reset line counter
        if b == b'\r' && i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
            // Trim trailing whitespace before line break
            while result.last() == Some(&b' ') || result.last() == Some(&b'\t') {
                let ws = result.pop().unwrap();
                // Need to encode the trailing whitespace
                let encoded = format!("={:02X}", ws);
                result.extend_from_slice(encoded.as_bytes());
            }
            result.push(b'\r');
            result.push(b'\n');
            line_len = 0;
            i += 2;
            continue;
        }
        if b == b'\n' {
            while result.last() == Some(&b' ') || result.last() == Some(&b'\t') {
                let ws = result.pop().unwrap();
                let encoded = format!("={:02X}", ws);
                result.extend_from_slice(encoded.as_bytes());
            }
            result.push(b'\n');
            line_len = 0;
            i += 1;
            continue;
        }

        // Determine the encoded form of this byte
        let is_printable = ((33..=126).contains(&b) && b != b'=') || b == b'\t' || b == b' ';
        let char_len = if is_printable { 1 } else { 3 };

        // Check if we need a soft line break (max line length is 76 chars)
        // We need room for the char plus potentially "=\r\n" soft break (3 chars)
        if line_len + char_len > 75 {
            result.extend_from_slice(b"=\r\n");
            line_len = 0;
        }

        if is_printable {
            result.push(b);
            line_len += 1;
        } else {
            result.push(b'=');
            result.extend_from_slice(format!("{:02X}", b).as_bytes());
            line_len += 3;
        }

        i += 1;
    }

    // Trim trailing whitespace at end of message
    while result.last() == Some(&b' ') || result.last() == Some(&b'\t') {
        let ws = result.pop().unwrap();
        let encoded = format!("={:02X}", ws);
        result.extend_from_slice(encoded.as_bytes());
    }

    Ok(Value::String(PhpString::from_vec(result)))
}

fn quoted_printable_decode(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = s.as_bytes();
    let mut result = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'='
            && i + 2 < bytes.len()
            && bytes[i + 1].is_ascii_hexdigit()
            && bytes[i + 2].is_ascii_hexdigit()
        {
            let hex: String = [bytes[i + 1] as char, bytes[i + 2] as char]
                .iter()
                .collect();
            result.push(u8::from_str_radix(&hex, 16).unwrap_or(0));
            i += 3;
        } else {
            result.push(bytes[i]);
            i += 1;
        }
    }
    Ok(Value::String(PhpString::from_vec(result)))
}

fn convert_uuencode(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return Ok(Value::False);
    }
    let mut result = Vec::new();
    for chunk in bytes.chunks(45) {
        // Length byte
        result.push((chunk.len() as u8) + 32);
        // Encode 3 bytes -> 4 chars
        let mut i = 0;
        while i < chunk.len() {
            let b0 = chunk[i];
            let b1 = if i + 1 < chunk.len() { chunk[i + 1] } else { 0 };
            let b2 = if i + 2 < chunk.len() { chunk[i + 2] } else { 0 };
            result.push(((b0 >> 2) & 0x3F) + 32);
            result.push((((b0 << 4) | (b1 >> 4)) & 0x3F) + 32);
            result.push((((b1 << 2) | (b2 >> 6)) & 0x3F) + 32);
            result.push((b2 & 0x3F) + 32);
            i += 3;
        }
        result.push(b'\n');
    }
    // End marker
    result.push(b' '); // zero-length line (32 = space = 0 + 32)
    result.push(b'\n');
    // Convert spaces to backticks (PHP convention)
    for b in &mut result {
        if *b == 32 { *b = b'`'; }
    }
    Ok(Value::String(PhpString::from_vec(result)))
}

fn convert_uudecode(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = s.as_bytes();
    let mut result = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        // Read length byte
        let len_byte = if bytes[i] == b'`' { 0u8 } else { bytes[i].wrapping_sub(32) & 0x3F };
        if len_byte == 0 { break; }
        i += 1;
        // Validate: check that we have enough encoded data on this line
        let expected_encoded = ((len_byte as usize + 2) / 3) * 4;
        let line_start = i;
        let mut line_end = i;
        while line_end < bytes.len() && bytes[line_end] != b'\n' && bytes[line_end] != b'\r' {
            line_end += 1;
        }
        let available = line_end - line_start;
        if available < expected_encoded {
            _vm.emit_warning("convert_uudecode(): Argument #1 ($data) is not a valid uuencoded string");
            return Ok(Value::False);
        }
        let mut decoded = 0usize;
        while decoded < len_byte as usize && i + 3 < bytes.len() {
            let c0 = if bytes[i] == b'`' { 0 } else { (bytes[i] - 32) & 0x3F };
            let c1 = if bytes[i+1] == b'`' { 0 } else { (bytes[i+1] - 32) & 0x3F };
            let c2 = if bytes[i+2] == b'`' { 0 } else { (bytes[i+2] - 32) & 0x3F };
            let c3 = if bytes[i+3] == b'`' { 0 } else { (bytes[i+3] - 32) & 0x3F };
            if decoded < len_byte as usize { result.push((c0 << 2) | (c1 >> 4)); decoded += 1; }
            if decoded < len_byte as usize { result.push((c1 << 4) | (c2 >> 2)); decoded += 1; }
            if decoded < len_byte as usize { result.push((c2 << 6) | c3); decoded += 1; }
            i += 4;
        }
        // Skip to next line
        while i < bytes.len() && bytes[i] != b'\n' { i += 1; }
        if i < bytes.len() { i += 1; }
    }
    Ok(Value::String(PhpString::from_vec(result)))
}

fn str_getcsv(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    use goro_core::array::PhpArray;
    use std::cell::RefCell;
    use std::rc::Rc;

    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let sep = args
        .get(1)
        .map(|v| v.to_php_string())
        .unwrap_or_else(|| PhpString::from_bytes(b","));
    let delim = sep.as_bytes().first().copied().unwrap_or(b',');

    let mut result = PhpArray::new();
    let mut current = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut in_quotes = false;

    while i < bytes.len() {
        if bytes[i] == b'"' && !in_quotes {
            in_quotes = true;
            i += 1;
        } else if bytes[i] == b'"' && in_quotes {
            if i + 1 < bytes.len() && bytes[i + 1] == b'"' {
                current.push(b'"');
                i += 2;
            } else {
                in_quotes = false;
                i += 1;
            }
        } else if bytes[i] == delim && !in_quotes {
            result.push(Value::String(PhpString::from_vec(current.clone())));
            current.clear();
            i += 1;
        } else {
            current.push(bytes[i]);
            i += 1;
        }
    }
    result.push(Value::String(PhpString::from_vec(current)));

    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn strtr(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let subject = args.first().unwrap_or(&Value::Null).to_php_string();

    // Two forms: strtr($str, $from, $to) or strtr($str, $replacements_array)
    if args.len() >= 3 {
        let from = args[1].to_php_string();
        let to = args[2].to_php_string();
        let from_bytes = from.as_bytes();
        let to_bytes = to.as_bytes();
        let mut result: Vec<u8> = subject.as_bytes().to_vec();
        let min_len = from_bytes.len().min(to_bytes.len());
        for byte in &mut result {
            for i in 0..min_len {
                if *byte == from_bytes[i] {
                    *byte = to_bytes[i];
                    break;
                }
            }
        }
        Ok(Value::String(PhpString::from_vec(result)))
    } else if let Some(Value::Array(replacements)) = args.get(1) {
        let replacements = replacements.borrow();
        // Sort by key length descending for correct replacement order
        let mut pairs: Vec<(Vec<u8>, Vec<u8>)> = replacements
            .iter()
            .filter_map(|(k, v)| {
                let key = match k {
                    goro_core::array::ArrayKey::String(s) => s.as_bytes().to_vec(),
                    goro_core::array::ArrayKey::Int(n) => n.to_string().into_bytes(),
                };
                if key.is_empty() {
                    None
                } else {
                    Some((key, v.to_php_string().as_bytes().to_vec()))
                }
            })
            .collect();
        pairs.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

        let src = subject.as_bytes();
        let mut result = Vec::with_capacity(src.len());
        let mut i = 0;
        while i < src.len() {
            let mut replaced = false;
            for (from, to) in &pairs {
                if i + from.len() <= src.len() && &src[i..i + from.len()] == from.as_slice() {
                    result.extend_from_slice(to);
                    i += from.len();
                    replaced = true;
                    break;
                }
            }
            if !replaced {
                result.push(src[i]);
                i += 1;
            }
        }
        Ok(Value::String(PhpString::from_vec(result)))
    } else if args.len() == 2 {
        // strtr($str, $replace_pairs) - second arg must be an array
        let val = &args[1];
        let type_name = goro_core::vm::Vm::value_type_name(val);
        let msg = format!("strtr(): Argument #2 ($from) must be of type array, {} given", type_name);
        let exc = _vm.throw_type_error(msg.clone());
        _vm.current_exception = Some(exc);
        Err(goro_core::vm::VmError { message: msg, line: _vm.current_line })
    } else {
        Ok(Value::String(subject))
    }
}

fn str_shuffle(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let mut bytes = s.as_bytes().to_vec();
    // Simple shuffle using time-based seed
    use std::time::SystemTime;
    let seed = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    let len = bytes.len();
    if len > 1 {
        for i in (1..len).rev() {
            let j = ((seed.wrapping_mul(i as u64 + 1).wrapping_add(37)) % (i as u64 + 1)) as usize;
            bytes.swap(i, j);
        }
    }
    Ok(Value::String(PhpString::from_vec(bytes)))
}

fn substr_compare(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let main_str = args.first().unwrap_or(&Value::Null).to_php_string();
    let str2 = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let offset = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    // length is nullable: null means no length limit
    let length = match args.get(3) {
        Some(Value::Null) | None => None,
        Some(v) => Some(v.to_long()),
    };
    let case_insensitive = args.get(4).map(|v| v.is_truthy()).unwrap_or(false);

    let main_bytes = main_str.as_bytes();
    let main_len = main_bytes.len() as i64;

    // Check negative length first (ValueError)
    if let Some(l) = length {
        if l < 0 {
            let msg = "substr_compare(): Argument #4 ($length) must be greater than or equal to 0".to_string();
            let exc = vm.throw_type_error(msg.clone());
            if let Value::Object(obj) = &exc {
                obj.borrow_mut().class_name = b"ValueError".to_vec();
            }
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
        }
    }

    // Resolve negative offset
    let start = if offset < 0 {
        let resolved = main_len + offset;
        if resolved < 0 {
            let msg = "substr_compare(): Argument #3 ($offset) must be contained in argument #1 ($haystack)".to_string();
            let exc = vm.throw_type_error(msg.clone());
            if let Value::Object(obj) = &exc {
                obj.borrow_mut().class_name = b"ValueError".to_vec();
            }
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
        }
        resolved as usize
    } else {
        if offset > main_len {
            let msg = "substr_compare(): Argument #3 ($offset) must be contained in argument #1 ($haystack)".to_string();
            let exc = vm.throw_type_error(msg.clone());
            if let Value::Object(obj) = &exc {
                obj.borrow_mut().class_name = b"ValueError".to_vec();
            }
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
        }
        offset as usize
    };

    // If length is explicitly 0, result is always 0
    if let Some(0) = length {
        return Ok(Value::Long(0));
    }

    let sub = &main_bytes[start..];
    let cmp_len = match length {
        Some(l) if l > 0 => l as usize,
        _ => sub.len().max(str2.len()),
    };

    let a = &sub[..cmp_len.min(sub.len())];
    let b = &str2.as_bytes()[..cmp_len.min(str2.len())];

    if case_insensitive {
        let a_lower: Vec<u8> = a.iter().map(|c| c.to_ascii_lowercase()).collect();
        let b_lower: Vec<u8> = b.iter().map(|c| c.to_ascii_lowercase()).collect();
        Ok(Value::Long(crate::misc::php_strcmp(&a_lower, &b_lower)))
    } else {
        Ok(Value::Long(crate::misc::php_strcmp(a, b)))
    }
}

fn similar_text(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s1 = args.first().unwrap_or(&Value::Null).to_php_string();
    let s2 = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let percent_ref = args.get(2);

    let a = s1.as_bytes();
    let b = s2.as_bytes();

    fn longest_common(a: &[u8], b: &[u8], pos_a: &mut usize, pos_b: &mut usize) -> usize {
        let mut max_len = 0usize;
        *pos_a = 0;
        *pos_b = 0;
        for i in 0..a.len() {
            for j in 0..b.len() {
                let mut l = 0;
                while i + l < a.len() && j + l < b.len() && a[i + l] == b[j + l] {
                    l += 1;
                }
                if l > max_len {
                    max_len = l;
                    *pos_a = i;
                    *pos_b = j;
                }
            }
        }
        max_len
    }

    fn similar_chars(a: &[u8], b: &[u8]) -> usize {
        let mut pos_a = 0;
        let mut pos_b = 0;
        let max_len = longest_common(a, b, &mut pos_a, &mut pos_b);
        if max_len == 0 {
            return 0;
        }
        let mut sum = max_len;
        if pos_a > 0 && pos_b > 0 {
            sum += similar_chars(&a[..pos_a], &b[..pos_b]);
        }
        if pos_a + max_len < a.len() && pos_b + max_len < b.len() {
            sum += similar_chars(&a[pos_a + max_len..], &b[pos_b + max_len..]);
        }
        sum
    }

    let sim = similar_chars(a, b) as i64;
    let total = (a.len() + b.len()) as f64;

    if let Some(Value::Reference(r)) = percent_ref {
        let pct = if total > 0.0 { (sim as f64 * 200.0) / total } else { 0.0 };
        *r.borrow_mut() = Value::Double(pct);
    }

    Ok(Value::Long(sim))
}

fn soundex(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = s.as_bytes();

    let code = |c: u8| -> u8 {
        match c.to_ascii_uppercase() {
            b'B' | b'F' | b'P' | b'V' => b'1',
            b'C' | b'G' | b'J' | b'K' | b'Q' | b'S' | b'X' | b'Z' => b'2',
            b'D' | b'T' => b'3',
            b'L' => b'4',
            b'M' | b'N' => b'5',
            b'R' => b'6',
            _ => b'0',
        }
    };

    // Find the first alphabetic character
    let first_alpha = bytes.iter().position(|b| b.is_ascii_alphabetic());
    if first_alpha.is_none() {
        // No alphabetic characters - return "0000" (PHP 8.5 behavior)
        return Ok(Value::String(PhpString::from_bytes(b"0000")));
    }
    let first_idx = first_alpha.unwrap();
    let mut result = String::new();
    result.push((bytes[first_idx]).to_ascii_uppercase() as char);

    let mut last = code(bytes[first_idx]);
    for &b in &bytes[first_idx + 1..] {
        if !b.is_ascii_alphabetic() {
            continue;
        }
        let c = code(b);
        if c != b'0' && c != last {
            result.push(c as char);
            if result.len() == 4 {
                break;
            }
        }
        last = c;
    }
    while result.len() < 4 {
        result.push('0');
    }

    Ok(Value::String(PhpString::from_string(result)))
}

fn metaphone(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let word = args.first().unwrap_or(&Value::Null).to_php_string();
    let max_phonemes = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let word_str = word.to_string_lossy().to_uppercase();
    let chars: Vec<char> = word_str.chars().filter(|c| c.is_ascii_alphabetic()).collect();
    if chars.is_empty() {
        return Ok(Value::String(PhpString::empty()));
    }
    let mut result = String::new();
    let max = if max_phonemes > 0 { max_phonemes as usize } else { usize::MAX };
    let len = chars.len();

    // Helper closures
    let at = |i: usize| -> char { if i < len { chars[i] } else { '\0' } };
    let is_vowel = |c: char| -> bool { matches!(c, 'A' | 'E' | 'I' | 'O' | 'U') };

    let mut i = 0;
    let mut last = '\0';
    // Skip initial silent letters
    match (at(0), at(1)) {
        ('A', 'E') | ('G', 'N') | ('K', 'N') | ('P', 'N') | ('W', 'R') => i = 1,
        ('W', 'H') => {
            // WH at start: produce W, skip H
            result.push('W');
            last = 'W';
            i = 2;
        },
        _ => {}
    }
    while i < len && result.len() < max {
        let c = at(i);
        if c == last && c != 'C' {
            i += 1;
            continue;
        }

        match c {
            'A' | 'E' | 'I' | 'O' | 'U' => {
                if i == 0 || (i == 1 && chars[0] == 'W' && chars[1] == 'H') {
                    result.push(c);
                    last = c;
                }
            }
            'B' => {
                if i == 0 || at(i.wrapping_sub(1)) != 'M' {
                    result.push('B');
                    last = 'B';
                }
            }
            'C' => {
                if at(i + 1) == 'H' {
                    result.push('X');
                    last = 'X';
                    i += 1; // skip H
                } else if at(i + 1) == 'I' || at(i + 1) == 'E' || at(i + 1) == 'Y' {
                    if at(i + 1) == 'I' && at(i + 2) == 'A' {
                        result.push('X');
                        last = 'X';
                    } else if i > 0 && at(i - 1) == 'S' {
                        // SC[IEY] - skip
                        last = 'S';
                    } else {
                        result.push('S');
                        last = 'S';
                    }
                } else {
                    result.push('K');
                    last = 'K';
                }
            }
            'D' => {
                if at(i + 1) == 'G' && (at(i + 2) == 'I' || at(i + 2) == 'E' || at(i + 2) == 'Y') {
                    result.push('J');
                    last = 'J';
                } else {
                    result.push('T');
                    last = 'T';
                }
            }
            'F' => { result.push('F'); last = 'F'; }
            'G' => {
                if i + 1 < len && at(i + 1) == 'H' && i + 2 < len && !is_vowel(at(i + 2)) {
                    // GH not followed by vowel - skip
                    last = 'G';
                } else if i > 0 && ((i + 1 >= len) || (at(i + 1) == 'N' && (i + 2 >= len || at(i + 2) == 'E' && i + 3 >= len))) {
                    // Silent G at end
                    last = 'G';
                } else if i > 0 && i + 1 < len && at(i - 1) == 'D' && (at(i + 1) == 'E' || at(i + 1) == 'I' || at(i + 1) == 'Y') {
                    // DGE, DGI, DGY - already handled as J
                    last = 'G';
                } else {
                    if i > 0 && at(i + 1) == 'H' && is_vowel(at(i + 2)) {
                        // GH before vowel
                        last = 'G';
                    } else if at(i + 1) != 'H' || (i + 2 < len && is_vowel(at(i + 2))) {
                        result.push('K');
                        last = 'K';
                    } else {
                        last = 'G';
                    }
                }
            }
            'H' => {
                if is_vowel(at(i + 1)) && (i == 0 || !is_vowel(at(i - 1))) {
                    result.push('H');
                    last = 'H';
                }
            }
            'J' => { result.push('J'); last = 'J'; }
            'K' => {
                if i == 0 || at(i - 1) != 'C' {
                    result.push('K');
                    last = 'K';
                }
            }
            'L' => { result.push('L'); last = 'L'; }
            'M' => { result.push('M'); last = 'M'; }
            'N' => { result.push('N'); last = 'N'; }
            'P' => {
                if at(i + 1) == 'H' {
                    result.push('F');
                    last = 'F';
                    i += 1;
                } else {
                    result.push('P');
                    last = 'P';
                }
            }
            'Q' => { result.push('K'); last = 'K'; }
            'R' => { result.push('R'); last = 'R'; }
            'S' => {
                if at(i + 1) == 'H' || (at(i + 1) == 'I' && (at(i + 2) == 'A' || at(i + 2) == 'O')) {
                    result.push('X');
                    last = 'X';
                    if at(i + 1) == 'H' { i += 1; }
                } else {
                    result.push('S');
                    last = 'S';
                }
            }
            'T' => {
                if at(i + 1) == 'H' {
                    result.push('0');
                    last = '0';
                    i += 1;
                } else if at(i + 1) == 'I' && (at(i + 2) == 'A' || at(i + 2) == 'O') {
                    result.push('X');
                    last = 'X';
                } else {
                    result.push('T');
                    last = 'T';
                }
            }
            'V' => { result.push('F'); last = 'F'; }
            'W' | 'Y' => {
                if i + 1 < len && is_vowel(at(i + 1)) {
                    result.push(c);
                    last = c;
                }
            }
            'X' => {
                result.push('K');
                if result.len() < max { result.push('S'); }
                last = 'S';
            }
            'Z' => { result.push('S'); last = 'S'; }
            _ => {}
        }
        i += 1;
    }
    Ok(Value::String(PhpString::from_string(result)))
}

fn levenshtein(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s1 = args
        .first()
        .unwrap_or(&Value::Null)
        .to_php_string()
        .to_string_lossy();
    let s2 = args
        .get(1)
        .unwrap_or(&Value::Null)
        .to_php_string()
        .to_string_lossy();

    // Optional costs: insertion_cost, replacement_cost, deletion_cost
    let ins_cost = args.get(2).map(|v| v.to_long() as usize).unwrap_or(1);
    let rep_cost = args.get(3).map(|v| v.to_long() as usize).unwrap_or(1);
    let del_cost = args.get(4).map(|v| v.to_long() as usize).unwrap_or(1);

    let len1 = s1.len();
    let len2 = s2.len();
    let mut matrix = vec![vec![0usize; len2 + 1]; len1 + 1];

    for i in 0..=len1 {
        matrix[i][0] = i * del_cost;
    }
    for j in 0..=len2 {
        matrix[0][j] = j * ins_cost;
    }

    for i in 1..=len1 {
        for j in 1..=len2 {
            let cost = if s1.as_bytes()[i - 1] == s2.as_bytes()[j - 1] {
                0
            } else {
                rep_cost
            };
            matrix[i][j] = (matrix[i - 1][j] + del_cost)
                .min(matrix[i][j - 1] + ins_cost)
                .min(matrix[i - 1][j - 1] + cost);
        }
    }

    Ok(Value::Long(matrix[len1][len2] as i64))
}

fn count_chars(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let mode = args.get(1).map(|v| v.to_long()).unwrap_or(0);

    if mode < 0 || mode > 4 {
        let msg = "count_chars(): Argument #2 ($mode) must be between 0 and 4 (inclusive)".to_string();
        let exc = vm.throw_type_error(msg.clone());
        if let Value::Object(obj) = &exc {
            obj.borrow_mut().class_name = b"ValueError".to_vec();
        }
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }

    let mut counts = [0i64; 256];
    for &b in s.as_bytes() {
        counts[b as usize] += 1;
    }

    match mode {
        3 => {
            // Return string with all unique bytes used
            let mut chars = Vec::new();
            for i in 0..256 {
                if counts[i] > 0 {
                    chars.push(i as u8);
                }
            }
            Ok(Value::String(PhpString::from_vec(chars)))
        }
        4 => {
            // Return string with all bytes NOT used
            let mut chars = Vec::new();
            for i in 0..256 {
                if counts[i] == 0 {
                    chars.push(i as u8);
                }
            }
            Ok(Value::String(PhpString::from_vec(chars)))
        }
        _ => {
            let mut result = PhpArray::new();
            match mode {
                0 => {
                    for i in 0..256 {
                        result.set(
                            goro_core::array::ArrayKey::Int(i as i64),
                            Value::Long(counts[i]),
                        );
                    }
                }
                1 => {
                    for i in 0..256 {
                        if counts[i] > 0 {
                            result.set(
                                goro_core::array::ArrayKey::Int(i as i64),
                                Value::Long(counts[i]),
                            );
                        }
                    }
                }
                2 => {
                    for i in 0..256 {
                        if counts[i] == 0 {
                            result.set(goro_core::array::ArrayKey::Int(i as i64), Value::Long(0));
                        }
                    }
                }
                _ => unreachable!(),
            }
            Ok(Value::Array(Rc::new(RefCell::new(result))))
        }
    }
}

fn str_split_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    use goro_core::array::PhpArray;
    use std::cell::RefCell;
    use std::rc::Rc;

    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let len = args.get(1).map(|v| v.to_long()).unwrap_or(1).max(1) as usize;
    let bytes = s.as_bytes();
    let mut result = PhpArray::new();
    if bytes.is_empty() {
        result.push(Value::String(PhpString::empty()));
    } else {
        for chunk in bytes.chunks(len) {
            result.push(Value::String(PhpString::from_vec(chunk.to_vec())));
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn strstr_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let haystack = args.first().unwrap_or(&Value::Null).to_php_string();
    let needle = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let before_needle = args.get(2).map(|v| v.is_truthy()).unwrap_or(false);
    if needle.is_empty() {
        // PHP 8.0+: empty needle matches at position 0
        if before_needle {
            return Ok(Value::String(PhpString::empty()));
        } else {
            return Ok(Value::String(haystack));
        }
    }
    if let Some(pos) = haystack
        .as_bytes()
        .windows(needle.len())
        .position(|w| w == needle.as_bytes())
    {
        if before_needle {
            Ok(Value::String(PhpString::from_vec(
                haystack.as_bytes()[..pos].to_vec(),
            )))
        } else {
            Ok(Value::String(PhpString::from_vec(
                haystack.as_bytes()[pos..].to_vec(),
            )))
        }
    } else {
        Ok(Value::False)
    }
}

fn strpbrk_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let haystack = args.first().unwrap_or(&Value::Null).to_php_string();
    let char_list = args.get(1).unwrap_or(&Value::Null).to_php_string();
    if char_list.is_empty() {
        let msg = "strpbrk(): Argument #2 ($characters) must be a non-empty string";
        let exc = vm.create_exception(b"ValueError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg.into(), line: vm.current_line });
    }
    let h = haystack.as_bytes();
    let chars = char_list.as_bytes();
    for (i, &b) in h.iter().enumerate() {
        if chars.contains(&b) {
            return Ok(Value::String(PhpString::from_vec(h[i..].to_vec())));
        }
    }
    Ok(Value::False)
}

fn strnatcmp_impl(a: &[u8], b: &[u8], case_insensitive: bool) -> i64 {
    let mut ai = 0;
    let mut bi = 0;
    while ai < a.len() && bi < b.len() {
        // Skip leading spaces
        while ai < a.len() && a[ai] == b' ' {
            ai += 1;
        }
        while bi < b.len() && b[bi] == b' ' {
            bi += 1;
        }
        if ai >= a.len() || bi >= b.len() {
            break;
        }
        // Both are digits - compare numerically
        if a[ai].is_ascii_digit() && b[bi].is_ascii_digit() {
            // Skip leading zeros
            let mut a_leading_zeros = 0;
            let mut b_leading_zeros = 0;
            while ai + a_leading_zeros < a.len() && a[ai + a_leading_zeros] == b'0' {
                a_leading_zeros += 1;
            }
            while bi + b_leading_zeros < b.len() && b[bi + b_leading_zeros] == b'0' {
                b_leading_zeros += 1;
            }
            // Extract digit runs (without leading zeros)
            let a_start = ai + a_leading_zeros;
            let b_start = bi + b_leading_zeros;
            let mut a_end = a_start;
            let mut b_end = b_start;
            while a_end < a.len() && a[a_end].is_ascii_digit() {
                a_end += 1;
            }
            while b_end < b.len() && b[b_end].is_ascii_digit() {
                b_end += 1;
            }
            let a_num_len = a_end - a_start;
            let b_num_len = b_end - b_start;
            // Different lengths means different magnitude
            if a_num_len != b_num_len {
                if a_num_len == 0 && b_num_len == 0 {
                    // Both are all zeros - compare by number of zeros
                    if a_leading_zeros != b_leading_zeros {
                        return if a_leading_zeros < b_leading_zeros { -1 } else { 1 };
                    }
                } else {
                    return if a_num_len < b_num_len { -1 } else { 1 };
                }
            } else {
                // Same length - compare digit by digit
                for k in 0..a_num_len {
                    if a[a_start + k] != b[b_start + k] {
                        return if a[a_start + k] < b[b_start + k] { -1 } else { 1 };
                    }
                }
                // Same numeric value - fewer leading zeros comes first
                if a_leading_zeros != b_leading_zeros {
                    return if a_leading_zeros < b_leading_zeros { -1 } else { 1 };
                }
            }
            ai = a_end;
            bi = b_end;
        } else {
            let ca = if case_insensitive { a[ai].to_ascii_lowercase() } else { a[ai] };
            let cb = if case_insensitive { b[bi].to_ascii_lowercase() } else { b[bi] };
            if ca != cb {
                return if ca < cb { -1 } else { 1 };
            }
            ai += 1;
            bi += 1;
        }
    }
    if ai < a.len() {
        1
    } else if bi < b.len() {
        -1
    } else {
        0
    }
}

fn strnatcmp_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = args.first().unwrap_or(&Value::Null).to_php_string();
    let b = args.get(1).unwrap_or(&Value::Null).to_php_string();
    Ok(Value::Long(strnatcmp_impl(a.as_bytes(), b.as_bytes(), false)))
}

fn strnatcasecmp_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = args.first().unwrap_or(&Value::Null).to_php_string();
    let b = args.get(1).unwrap_or(&Value::Null).to_php_string();
    Ok(Value::Long(strnatcmp_impl(a.as_bytes(), b.as_bytes(), true)))
}

fn strcmp_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = args.first().unwrap_or(&Value::Null).to_php_string();
    let b = args.get(1).unwrap_or(&Value::Null).to_php_string();
    Ok(Value::Long(match a.as_bytes().cmp(b.as_bytes()) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }))
}

fn strncmp_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = args.first().unwrap_or(&Value::Null).to_php_string();
    let b = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let len_raw = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    if len_raw < 0 {
        let msg = "strncmp(): Argument #3 ($length) must be greater than or equal to 0".to_string();
        let exc = vm.throw_type_error(msg.clone());
        if let Value::Object(obj) = &exc {
            obj.borrow_mut().class_name = b"ValueError".to_vec();
        }
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    let len = len_raw as usize;
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    let a_sub_len = len.min(a_bytes.len());
    let b_sub_len = len.min(b_bytes.len());
    // Compare byte-by-byte (returns actual difference like PHP)
    let compare_len = a_sub_len.min(b_sub_len);
    for i in 0..compare_len {
        if a_bytes[i] != b_bytes[i] {
            return Ok(Value::Long(a_bytes[i] as i64 - b_bytes[i] as i64));
        }
    }
    Ok(Value::Long(a_sub_len as i64 - b_sub_len as i64))
}

fn strcasecmp_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = args.first().unwrap_or(&Value::Null).to_php_string();
    let b = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    let min_len = a_bytes.len().min(b_bytes.len());
    for i in 0..min_len {
        let ca = a_bytes[i].to_ascii_lowercase();
        let cb = b_bytes[i].to_ascii_lowercase();
        if ca != cb {
            return Ok(Value::Long(ca as i64 - cb as i64));
        }
    }
    Ok(Value::Long(a_bytes.len() as i64 - b_bytes.len() as i64))
}

fn strncasecmp_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = args.first().unwrap_or(&Value::Null).to_php_string();
    let b = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let len_raw = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    if len_raw < 0 {
        let msg = "strncasecmp(): Argument #3 ($length) must be greater than or equal to 0".to_string();
        let exc = vm.throw_type_error(msg.clone());
        if let Value::Object(obj) = &exc {
            obj.borrow_mut().class_name = b"ValueError".to_vec();
        }
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    let len = len_raw as usize;
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    let a_sub_len = len.min(a_bytes.len());
    let b_sub_len = len.min(b_bytes.len());
    // Compare byte-by-byte like PHP does (returns actual difference)
    let compare_len = a_sub_len.min(b_sub_len);
    for i in 0..compare_len {
        let ca = a_bytes[i].to_ascii_lowercase();
        let cb = b_bytes[i].to_ascii_lowercase();
        if ca != cb {
            return Ok(Value::Long(ca as i64 - cb as i64));
        }
    }
    // If all compared bytes are equal, compare by length (normalized to -1/0/1)
    let len_diff = a_sub_len as i64 - b_sub_len as i64;
    Ok(Value::Long(if len_diff < 0 { -1 } else if len_diff > 0 { 1 } else { 0 }))
}

fn vprintf_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        let msg = "vprintf() expects at least 1 argument, 0 given";
        let exc = vm.create_exception(b"ArgumentCountError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg.into(), line: vm.current_line });
    }
    // Check format arg type
    validate_sprintf_format_arg(vm, "vprintf", args.first())?;
    let format = args.first().unwrap_or(&Value::Null).to_php_string();
    let arr = args.get(1).unwrap_or(&Value::Null);
    let fmt_args: Vec<Value> = if let Value::Array(a) = arr {
        a.borrow().values().cloned().collect()
    } else {
        vec![]
    };
    let mut all_args = vec![Value::String(format)];
    all_args.extend(fmt_args);
    validate_sprintf_arg_count(vm, "vprintf", &all_args)?;
    let result = do_sprintf(&all_args);
    let s_bytes = result.as_bytes();
    vm.write_output(s_bytes);
    Ok(Value::Long(s_bytes.len() as i64))
}

fn printf_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        let msg = "printf() expects at least 1 argument, 0 given";
        let exc = vm.create_exception(b"ArgumentCountError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg.into(), line: vm.current_line });
    }
    validate_sprintf_format_arg(vm, "printf", args.first())?;
    validate_sprintf_arg_count(vm, "printf", args)?;
    let result = do_sprintf(args);
    let s_bytes = result.as_bytes();
    vm.write_output(s_bytes);
    Ok(Value::Long(s_bytes.len() as i64))
}

fn strrchr(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let haystack = args.first().unwrap_or(&Value::Null).to_php_string();
    let needle = args.get(1).unwrap_or(&Value::Null);
    let before_needle = args.get(2).map(|v| v.is_truthy()).unwrap_or(false);
    let needle_byte = match needle {
        Value::String(s) if !s.is_empty() => s.as_bytes()[0],
        Value::Long(n) => *n as u8,
        _ => return Ok(Value::False),
    };
    if let Some(pos) = haystack.as_bytes().iter().rposition(|&b| b == needle_byte) {
        if before_needle {
            Ok(Value::String(PhpString::from_vec(
                haystack.as_bytes()[..pos].to_vec(),
            )))
        } else {
            Ok(Value::String(PhpString::from_vec(
                haystack.as_bytes()[pos..].to_vec(),
            )))
        }
    } else {
        Ok(Value::False)
    }
}

fn stristr(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let haystack = args.first().unwrap_or(&Value::Null).to_php_string();
    let needle = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let before_needle = args.get(2).map(|v| v.is_truthy()).unwrap_or(false);
    if needle.is_empty() {
        // PHP 8.0+: empty needle matches at position 0
        if before_needle {
            return Ok(Value::String(PhpString::empty()));
        } else {
            return Ok(Value::String(haystack));
        }
    }
    let h_lower: Vec<u8> = haystack
        .as_bytes()
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect();
    let n_lower: Vec<u8> = needle
        .as_bytes()
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect();
    if let Some(pos) = h_lower
        .windows(n_lower.len())
        .position(|w| w == n_lower.as_slice())
    {
        if before_needle {
            Ok(Value::String(PhpString::from_vec(
                haystack.as_bytes()[..pos].to_vec(),
            )))
        } else {
            Ok(Value::String(PhpString::from_vec(
                haystack.as_bytes()[pos..].to_vec(),
            )))
        }
    } else {
        Ok(Value::False)
    }
}

thread_local! {
    static STRTOK_STATE: RefCell<(Vec<u8>, usize)> = RefCell::new((Vec::new(), 0));
}

fn strtok_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // strtok has two forms:
    // strtok(string, delimiters) - initializes with new string
    // strtok(delimiters) - continues tokenizing the current string
    let (set_new, delim) = if args.len() >= 2 {
        // strtok(string, delimiters) - set new string
        let s = args[0].to_php_string();
        let d = args[1].to_php_string();
        STRTOK_STATE.with(|state| {
            let mut st = state.borrow_mut();
            st.0 = s.as_bytes().to_vec();
            st.1 = 0;
        });
        (true, d)
    } else {
        // strtok(delimiters) - continue with saved string
        let d = args.first().unwrap_or(&Value::Null).to_php_string();
        (false, d)
    };
    let _ = set_new;
    let delim_bytes = delim.as_bytes();

    STRTOK_STATE.with(|state| {
        let mut st = state.borrow_mut();
        let mut pos = st.1;
        let s_len = st.0.len();

        // Skip leading delimiter characters
        while pos < s_len && delim_bytes.contains(&st.0[pos]) {
            pos += 1;
        }

        if pos >= s_len {
            st.1 = pos;
            return Ok(Value::False);
        }

        // Find next delimiter
        let start = pos;
        while pos < s_len && !delim_bytes.contains(&st.0[pos]) {
            pos += 1;
        }

        let token = st.0[start..pos].to_vec();

        // Move past the delimiter for next call
        st.1 = if pos < s_len { pos + 1 } else { pos };

        Ok(Value::String(PhpString::from_vec(token)))
    })
}

fn compute_substr_range(s_len: usize, offset: i64, length: Option<i64>) -> (usize, usize) {
    let start = if offset >= 0 {
        (offset as usize).min(s_len)
    } else {
        s_len.saturating_sub((-offset) as usize)
    };
    let end = match length {
        Some(l) if l < 0 => {
            let end_pos = s_len as i64 + l;
            if end_pos <= start as i64 { start } else { end_pos as usize }
        }
        Some(l) => {
            let end_pos = start as i64 + l;
            if end_pos > s_len as i64 { s_len } else if end_pos < start as i64 { start } else { end_pos as usize }
        }
        None => s_len,
    };
    (start, end)
}

fn strspn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let chars = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let offset = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    let length = args.get(3).map(|v| v.to_long());
    let s_bytes = s.as_bytes();
    let chars_bytes = chars.as_bytes();
    let (start, end) = compute_substr_range(s_bytes.len(), offset, length);
    if start >= end {
        return Ok(Value::Long(0));
    }
    let count = s_bytes[start..end]
        .iter()
        .take_while(|b| chars_bytes.contains(b))
        .count();
    Ok(Value::Long(count as i64))
}

fn strcspn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let chars = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let offset = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    let length = args.get(3).map(|v| v.to_long());
    let s_bytes = s.as_bytes();
    let chars_bytes = chars.as_bytes();
    let (start, end) = compute_substr_range(s_bytes.len(), offset, length);
    if start >= end {
        return Ok(Value::Long(0));
    }
    let count = s_bytes[start..end]
        .iter()
        .take_while(|b| !chars_bytes.contains(b))
        .count();
    Ok(Value::Long(count as i64))
}

fn vsprintf(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        let msg = "vsprintf() expects at least 1 argument, 0 given";
        let exc = vm.create_exception(b"ArgumentCountError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg.into(), line: vm.current_line });
    }
    validate_sprintf_format_arg(vm, "vsprintf", args.first())?;
    let format = args.first().unwrap_or(&Value::Null).to_php_string();
    let arr = args.get(1).unwrap_or(&Value::Null);
    let fmt_args: Vec<Value> = if let Value::Array(a) = arr {
        a.borrow().values().cloned().collect()
    } else {
        vec![]
    };
    let mut all_args = vec![Value::String(format)];
    all_args.extend(fmt_args);
    validate_sprintf_arg_count(vm, "vsprintf", &all_args)?;
    let result = do_sprintf(&all_args);
    Ok(Value::String(PhpString::from_vec(result.into_bytes())))
}

fn substr_count(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let haystack = args.first().unwrap_or(&Value::Null).to_php_string();
    let needle = args.get(1).unwrap_or(&Value::Null).to_php_string();
    if needle.is_empty() {
        // PHP 8: throw ValueError for empty needle
        let msg = "substr_count(): Argument #2 ($needle) must not be empty";
        let exc = vm.create_exception(b"ValueError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg.into(), line: vm.current_line });
    }
    let h_bytes = haystack.as_bytes();
    let h_len = h_bytes.len() as i64;
    let raw_offset = args.get(2).map(|v| v.to_long()).unwrap_or(0);

    // Resolve offset (support negative)
    let start = if raw_offset < 0 {
        let s = h_len + raw_offset;
        if s < 0 {
            let msg = "substr_count(): Argument #3 ($offset) must be contained in argument #1 ($haystack)";
            let exc = vm.create_exception(b"ValueError", msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg.into(), line: vm.current_line });
        }
        s as usize
    } else {
        if raw_offset > h_len {
            let msg = "substr_count(): Argument #3 ($offset) must be contained in argument #1 ($haystack)";
            let exc = vm.create_exception(b"ValueError", msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg.into(), line: vm.current_line });
        }
        raw_offset as usize
    };

    let end = match args.get(3) {
        Some(len_val) if !matches!(len_val, Value::Null) => {
            let len = len_val.to_long();
            if len < 0 {
                let e = h_len + len;
                if e < start as i64 {
                    let msg = "substr_count(): Argument #4 ($length) must be contained in argument #1 ($haystack)";
                    let exc = vm.create_exception(b"ValueError", msg, 0);
                    vm.current_exception = Some(exc);
                    return Err(VmError { message: msg.into(), line: vm.current_line });
                }
                e as usize
            } else {
                let e = start + len as usize;
                if e > h_bytes.len() {
                    let msg = "substr_count(): Argument #4 ($length) must be contained in argument #1 ($haystack)";
                    let exc = vm.create_exception(b"ValueError", msg, 0);
                    vm.current_exception = Some(exc);
                    return Err(VmError { message: msg.into(), line: vm.current_line });
                }
                e
            }
        }
        _ => h_bytes.len(),
    };

    if start >= end || needle.len() > end - start {
        return Ok(Value::Long(0));
    }

    let search_bytes = &h_bytes[start..end];
    let count = search_bytes
        .windows(needle.len())
        .filter(|w| *w == needle.as_bytes())
        .count();
    Ok(Value::Long(count as i64))
}

fn str_ireplace(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let search_val = args.first().unwrap_or(&Value::Null);
    let replace_val = args.get(1).unwrap_or(&Value::Null);
    let subject_val = args.get(2).unwrap_or(&Value::Null);

    // Build search/replace pairs (same logic as str_replace but case-insensitive)
    let pairs: Vec<(PhpString, PhpString)> = match (search_val, replace_val) {
        (Value::Array(sa), Value::Array(ra)) => {
            let sa = sa.borrow();
            let ra = ra.borrow();
            let search_vals: Vec<_> = sa.values().collect();
            let replace_vals: Vec<_> = ra.values().collect();
            let mut pairs = Vec::new();
            for (i, sv) in search_vals.iter().enumerate() {
                let rv = replace_vals.get(i).map(|v| v.to_php_string()).unwrap_or_else(|| PhpString::empty());
                pairs.push((sv.to_php_string(), rv));
            }
            pairs
        }
        (Value::Array(sa), _) => {
            let sa = sa.borrow();
            sa.values().map(|sv| (sv.to_php_string(), replace_val.to_php_string())).collect()
        }
        _ => {
            vec![(search_val.to_php_string(), replace_val.to_php_string())]
        }
    };

    let result = match subject_val {
        Value::Array(subject_arr) => {
            let subject_arr = subject_arr.borrow();
            let mut result_arr = PhpArray::new();
            for (key, val) in subject_arr.iter() {
                let mut current = val.to_php_string().as_bytes().to_vec();
                for (needle, replacement) in &pairs {
                    let (new_val, _) = str_ireplace_single(&current, needle.as_bytes(), replacement.as_bytes());
                    current = new_val;
                }
                result_arr.set(key.clone(), Value::String(PhpString::from_vec(current)));
            }
            Value::Array(Rc::new(RefCell::new(result_arr)))
        }
        _ => {
            let mut current = subject_val.to_php_string().as_bytes().to_vec();
            for (needle, replacement) in &pairs {
                let (new_val, _) = str_ireplace_single(&current, needle.as_bytes(), replacement.as_bytes());
                current = new_val;
            }
            Value::String(PhpString::from_vec(current))
        }
    };

    Ok(result)
}

fn str_ireplace_single(haystack: &[u8], needle: &[u8], replacement: &[u8]) -> (Vec<u8>, i64) {
    if needle.is_empty() {
        return (haystack.to_vec(), 0);
    }
    let needle_lower: Vec<u8> = needle.iter().map(|b| b.to_ascii_lowercase()).collect();
    let mut result = Vec::new();
    let mut count = 0i64;
    let mut i = 0;
    while i < haystack.len() {
        if i + needle_lower.len() <= haystack.len() {
            let window: Vec<u8> = haystack[i..i + needle_lower.len()]
                .iter()
                .map(|b| b.to_ascii_lowercase())
                .collect();
            if window == needle_lower {
                result.extend_from_slice(replacement);
                i += needle_lower.len();
                count += 1;
                continue;
            }
        }
        result.push(haystack[i]);
        i += 1;
    }
    (result, count)
}

fn wordwrap(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let width = args.get(1).map(|v| v.to_long()).unwrap_or(75);
    let brk = args
        .get(2)
        .map(|v| v.to_php_string())
        .unwrap_or_else(|| PhpString::from_bytes(b"\n"));
    let cut_long = args.get(3).map(|v| v.is_truthy()).unwrap_or(false);

    if width == 0 && cut_long {
        let msg = "wordwrap(): Argument #4 ($cut_long_words) cannot be true when argument #2 ($width) is 0";
        let exc = vm.create_exception(b"ValueError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg.into(), line: vm.current_line });
    }

    let width = if cut_long { width.max(1) as usize } else { width.max(0) as usize };
    let bytes = s.as_bytes();
    let brk_bytes = brk.as_bytes();

    if brk_bytes.is_empty() {
        let msg = "wordwrap(): Argument #3 ($break) must not be empty".to_string();
        let exc = vm.throw_type_error(msg.clone());
        if let Value::Object(obj) = &exc {
            obj.borrow_mut().class_name = b"ValueError".to_vec();
        }
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }

    if bytes.is_empty() {
        return Ok(Value::String(PhpString::empty()));
    }

    let width = width.max(1) as usize;

    // PHP's wordwrap algorithm (mirrors PHP's C implementation for multi-char breaks):
    // 1. Detect existing break sequences in text and copy through.
    // 2. For spaces: if line_len >= width, replace space with break.
    // 3. For cut_long: if line_len >= width and no valid space, force cut.
    // 4. For word boundary: if line_len >= width and valid space, break at lastspace.

    let mut result = Vec::with_capacity(bytes.len() + bytes.len() / width * brk_bytes.len());
    let mut laststart: usize = 0;
    let mut lastspace: usize = 0;
    let mut current: usize = 0;

    while current < bytes.len() {
        let ch = bytes[current];

        // Check for existing break string in text
        if ch == brk_bytes[0]
            && current + brk_bytes.len() <= bytes.len()
            && &bytes[current..current + brk_bytes.len()] == brk_bytes
        {
            // Copy everything from laststart through the break string
            result.extend_from_slice(&bytes[laststart..current + brk_bytes.len()]);
            current += brk_bytes.len();
            laststart = current;
            lastspace = current;
            continue;
        } else if ch == b' ' {
            if current - laststart >= width {
                // Line is long enough - break at this space (replace space with break)
                result.extend_from_slice(&bytes[laststart..current]);
                result.extend_from_slice(brk_bytes);
                laststart = current + 1;
            }
            lastspace = current;
        } else if current - laststart >= width && cut_long && laststart >= lastspace {
            // Force cut at current position (no valid space available)
            result.extend_from_slice(&bytes[laststart..current]);
            result.extend_from_slice(brk_bytes);
            laststart = current;
            lastspace = current;
            continue; // re-examine current character (like PHP's chk--)
        } else if current - laststart >= width && laststart < lastspace {
            // Break at last recorded space
            result.extend_from_slice(&bytes[laststart..lastspace]);
            result.extend_from_slice(brk_bytes);
            laststart = lastspace + 1;
            lastspace = laststart;
        }

        current += 1;
    }

    // Append remaining
    if laststart < bytes.len() {
        result.extend_from_slice(&bytes[laststart..]);
    }

    Ok(Value::String(PhpString::from_vec(result)))
}

fn strrpos(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let haystack = args.first().unwrap_or(&Value::Null).to_php_string();
    let needle = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let offset = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() {
        // Empty needle: return position based on offset
        if offset >= 0 {
            let start = offset as usize;
            if start > h.len() {
                let msg = "strrpos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)";
                let exc = vm.create_exception(b"ValueError", msg, 0);
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg.into(), line: vm.current_line });
            }
            return Ok(Value::Long(h.len() as i64));
        } else {
            let abs = (-offset) as usize;
            if abs > h.len() {
                let msg = "strrpos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)";
                let exc = vm.create_exception(b"ValueError", msg, 0);
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg.into(), line: vm.current_line });
            }
            return Ok(Value::Long((h.len() - abs) as i64));
        }
    }
    if offset >= 0 {
        let start = offset as usize;
        if start > h.len() {
            let msg = "strrpos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)";
            let exc = vm.create_exception(b"ValueError", msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg.into(), line: vm.current_line });
        }
        if n.len() > h.len() || start >= h.len() {
            return Ok(Value::False);
        }
        if let Some(pos) = h[start..].windows(n.len()).rposition(|w| w == n) {
            Ok(Value::Long((start + pos) as i64))
        } else {
            Ok(Value::False)
        }
    } else {
        // Negative offset: search from beginning, but limit end position
        let abs = (-offset) as usize;
        if abs > h.len() {
            let msg = "strrpos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)";
            let exc = vm.create_exception(b"ValueError", msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg.into(), line: vm.current_line });
        }
        let end = h.len() - abs;
        if end == 0 || n.len() > end {
            return Ok(Value::False);
        }
        if let Some(pos) = h[..end].windows(n.len()).rposition(|w| w == n) {
            Ok(Value::Long(pos as i64))
        } else {
            Ok(Value::False)
        }
    }
}

fn stripos(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let haystack = args.first().unwrap_or(&Value::Null).to_php_string();
    let needle = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let offset_val = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    let h: Vec<u8> = haystack
        .as_bytes()
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect();
    let n: Vec<u8> = needle
        .as_bytes()
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect();
    // Handle negative offset
    let offset = if offset_val < 0 {
        let abs = (-offset_val) as usize;
        if abs > h.len() {
            let msg = "stripos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)";
            let exc = vm.create_exception(b"ValueError", msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg.into(), line: vm.current_line });
        }
        h.len() - abs
    } else {
        let o = offset_val as usize;
        if o > h.len() {
            let msg = "stripos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)";
            let exc = vm.create_exception(b"ValueError", msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg.into(), line: vm.current_line });
        }
        o
    };
    if n.is_empty() {
        return Ok(Value::Long(offset as i64));
    }
    if offset >= h.len() {
        return Ok(Value::False);
    }
    if let Some(pos) = h[offset..].windows(n.len()).position(|w| w == n.as_slice()) {
        Ok(Value::Long((offset + pos) as i64))
    } else {
        Ok(Value::False)
    }
}

fn strripos(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let haystack = args.first().unwrap_or(&Value::Null).to_php_string();
    let needle = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let offset = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    let h: Vec<u8> = haystack
        .as_bytes()
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect();
    let n: Vec<u8> = needle
        .as_bytes()
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect();
    if n.is_empty() {
        if offset >= 0 {
            let start = offset as usize;
            if start > h.len() {
                let msg = "strripos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)";
                let exc = vm.create_exception(b"ValueError", msg, 0);
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg.into(), line: vm.current_line });
            }
            return Ok(Value::Long(h.len() as i64));
        } else {
            let abs = (-offset) as usize;
            if abs > h.len() {
                let msg = "strripos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)";
                let exc = vm.create_exception(b"ValueError", msg, 0);
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg.into(), line: vm.current_line });
            }
            return Ok(Value::Long((h.len() - abs) as i64));
        }
    }
    if offset >= 0 {
        let start = offset as usize;
        if start > h.len() {
            let msg = "strripos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)";
            let exc = vm.create_exception(b"ValueError", msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg.into(), line: vm.current_line });
        }
        // Search from start to end, find last occurrence
        if start >= h.len() {
            return Ok(Value::False);
        }
        if let Some(pos) = h[start..].windows(n.len()).rposition(|w| w == n.as_slice()) {
            Ok(Value::Long((start + pos) as i64))
        } else {
            Ok(Value::False)
        }
    } else {
        // Negative offset: search from beginning, but limit end position
        let abs = (-offset) as usize;
        if abs > h.len() {
            let msg = "strripos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)";
            let exc = vm.create_exception(b"ValueError", msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg.into(), line: vm.current_line });
        }
        let end = h.len() - abs;
        if end == 0 || n.len() > end {
            return Ok(Value::False);
        }
        if let Some(pos) = h[..end].windows(n.len()).rposition(|w| w == n.as_slice()) {
            Ok(Value::Long(pos as i64))
        } else {
            Ok(Value::False)
        }
    }
}

fn substr_replace_single(s: &[u8], replacement: &[u8], start: i64, length: Option<i64>) -> Vec<u8> {
    let len = s.len() as i64;
    let start_idx = if start < 0 {
        (len + start).max(0) as usize
    } else {
        start.min(len) as usize
    };
    let end_idx = match length {
        Some(l) if l < 0 => (len + l).max(start_idx as i64) as usize,
        Some(l) => (start_idx + l as usize).min(s.len()),
        None => s.len(),
    };
    let mut result = s[..start_idx].to_vec();
    result.extend_from_slice(replacement);
    if end_idx < s.len() {
        result.extend_from_slice(&s[end_idx..]);
    }
    result
}

fn substr_replace(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let string_arg = args.first().unwrap_or(&Value::Null);
    let replacement_arg = args.get(1).unwrap_or(&Value::Null);
    let start_arg = args.get(2).unwrap_or(&Value::Null);
    let length_arg = args.get(3);

    // If first argument is an array, process each element
    if let Value::Array(arr) = string_arg {
        let arr = arr.borrow();
        let mut result = PhpArray::new();

        for (i, (key, val)) in arr.iter().enumerate() {
            let s = val.to_php_string();

            // Get replacement for this element
            let repl = if let Value::Array(repl_arr) = replacement_arg {
                let repl_arr = repl_arr.borrow();
                repl_arr.iter().nth(i)
                    .map(|(_, v)| v.to_php_string())
                    .unwrap_or(PhpString::empty())
            } else {
                replacement_arg.to_php_string()
            };

            // Get start for this element
            let start = if let Value::Array(start_arr) = start_arg {
                let start_arr = start_arr.borrow();
                start_arr.iter().nth(i)
                    .map(|(_, v)| v.to_long())
                    .unwrap_or(0)
            } else {
                start_arg.to_long()
            };

            // Get length for this element
            let length = length_arg.map(|la| {
                if let Value::Array(len_arr) = la {
                    let len_arr = len_arr.borrow();
                    len_arr.iter().nth(i)
                        .map(|(_, v)| v.to_long())
                } else {
                    Some(la.to_long())
                }
            }).flatten();

            let replaced = substr_replace_single(s.as_bytes(), repl.as_bytes(), start, length);
            result.set(key.clone(), Value::String(PhpString::from_vec(replaced)));
        }

        Ok(Value::Array(std::rc::Rc::new(std::cell::RefCell::new(result))))
    } else {
        let s = string_arg.to_php_string();
        let replacement = replacement_arg.to_php_string();
        let start = start_arg.to_long();
        let length = length_arg.map(|v| v.to_long());
        let result = substr_replace_single(s.as_bytes(), replacement.as_bytes(), start, length);
        Ok(Value::String(PhpString::from_vec(result)))
    }
}


fn hex2bin(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let hex = s.as_bytes();
    if hex.len() % 2 != 0 {
        _vm.emit_warning("hex2bin(): Hexadecimal input string must have an even length");
        return Ok(Value::False);
    }
    let mut result = Vec::new();
    for chunk in hex.chunks(2) {
        let hi = match chunk[0] {
            b'0'..=b'9' => chunk[0] - b'0',
            b'a'..=b'f' => chunk[0] - b'a' + 10,
            b'A'..=b'F' => chunk[0] - b'A' + 10,
            _ => return Ok(Value::False),
        };
        let lo = match chunk[1] {
            b'0'..=b'9' => chunk[1] - b'0',
            b'a'..=b'f' => chunk[1] - b'a' + 10,
            b'A'..=b'F' => chunk[1] - b'A' + 10,
            _ => return Ok(Value::False),
        };
        result.push(hi * 16 + lo);
    }
    Ok(Value::String(PhpString::from_vec(result)))
}

fn bin2hex(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let hex: String = s.as_bytes().iter().map(|b| format!("{:02x}", b)).collect();
    Ok(Value::String(PhpString::from_string(hex)))
}

fn crc32_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    // CRC-32 implementation
    let mut crc: u32 = 0xFFFFFFFF;
    for &byte in s.as_bytes() {
        crc ^= byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
        }
    }
    let result = crc ^ 0xFFFFFFFF;
    Ok(Value::Long(result as i64))
}

fn str_increment(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = s.as_bytes();
    if bytes.is_empty() || !bytes.iter().all(|b| b.is_ascii_alphanumeric()) {
        return Ok(Value::String(s));
    }
    let mut result = bytes.to_vec();
    let mut carry = true;
    for i in (0..result.len()).rev() {
        if !carry {
            break;
        }
        carry = false;
        match result[i] {
            b'z' => {
                result[i] = b'a';
                carry = true;
            }
            b'Z' => {
                result[i] = b'A';
                carry = true;
            }
            b'9' => {
                result[i] = b'0';
                carry = true;
            }
            _ => {
                result[i] += 1;
            }
        }
    }
    if carry {
        let prefix = match result[0] {
            b'a'..=b'z' => b'a',
            b'A'..=b'Z' => b'A',
            _ => b'1',
        };
        result.insert(0, prefix);
    }
    Ok(Value::String(PhpString::from_vec(result)))
}

fn pack_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let format = args.first().unwrap_or(&Value::Null).to_php_string();
    let fmt = format.as_bytes();
    let mut result = Vec::new();
    let mut arg_idx = 1;
    let mut i = 0;

    while i < fmt.len() {
        let code = fmt[i];
        i += 1;

        // Parse optional repeat count
        let mut count = 0u32;
        let mut has_count = false;
        if code == b'a' || code == b'A' || code == b'H' || code == b'h' || code == b'Z' {
            // For string codes, count means padding length; * means full string
            if i < fmt.len() && fmt[i] == b'*' {
                count = u32::MAX;
                has_count = true;
                i += 1;
            } else {
                while i < fmt.len() && fmt[i].is_ascii_digit() {
                    count = count * 10 + (fmt[i] - b'0') as u32;
                    has_count = true;
                    i += 1;
                }
                if !has_count {
                    count = 1;
                }
            }
        } else if i < fmt.len() && fmt[i] == b'*' {
            count = u32::MAX; // Repeat for all remaining args
            i += 1;
        } else {
            while i < fmt.len() && fmt[i].is_ascii_digit() {
                count = count * 10 + (fmt[i] - b'0') as u32;
                has_count = true;
                i += 1;
            }
            if !has_count {
                count = 1;
            }
        }

        let repeat = if count == u32::MAX {
            args.len() - arg_idx
        } else {
            count as usize
        };

        match code {
            b'C' | b'c' => {
                for _ in 0..repeat {
                    let v = args.get(arg_idx).map(|v| v.to_long()).unwrap_or(0);
                    result.push(v as u8);
                    arg_idx += 1;
                }
            }
            b'S' | b'v' => {
                // unsigned short (16-bit LE)
                for _ in 0..repeat {
                    let v = args.get(arg_idx).map(|v| v.to_long()).unwrap_or(0) as u16;
                    result.extend_from_slice(&v.to_le_bytes());
                    arg_idx += 1;
                }
            }
            b'n' => {
                // unsigned short (16-bit BE)
                for _ in 0..repeat {
                    let v = args.get(arg_idx).map(|v| v.to_long()).unwrap_or(0) as u16;
                    result.extend_from_slice(&v.to_be_bytes());
                    arg_idx += 1;
                }
            }
            b'L' | b'V' => {
                // unsigned long (32-bit LE)
                for _ in 0..repeat {
                    let v = args.get(arg_idx).map(|v| v.to_long()).unwrap_or(0) as u32;
                    result.extend_from_slice(&v.to_le_bytes());
                    arg_idx += 1;
                }
            }
            b'N' => {
                // unsigned long (32-bit BE)
                for _ in 0..repeat {
                    let v = args.get(arg_idx).map(|v| v.to_long()).unwrap_or(0) as u32;
                    result.extend_from_slice(&v.to_be_bytes());
                    arg_idx += 1;
                }
            }
            b'Q' | b'P' => {
                // unsigned long long (64-bit LE)
                for _ in 0..repeat {
                    let v = args.get(arg_idx).map(|v| v.to_long()).unwrap_or(0) as u64;
                    result.extend_from_slice(&v.to_le_bytes());
                    arg_idx += 1;
                }
            }
            b'J' => {
                // unsigned long long (64-bit BE)
                for _ in 0..repeat {
                    let v = args.get(arg_idx).map(|v| v.to_long()).unwrap_or(0) as u64;
                    result.extend_from_slice(&v.to_be_bytes());
                    arg_idx += 1;
                }
            }
            b'f' | b'g' => {
                // float (LE)
                for _ in 0..repeat {
                    let v = args.get(arg_idx).map(|v| v.to_double()).unwrap_or(0.0) as f32;
                    result.extend_from_slice(&v.to_le_bytes());
                    arg_idx += 1;
                }
            }
            b'd' | b'e' => {
                // double (LE)
                for _ in 0..repeat {
                    let v = args.get(arg_idx).map(|v| v.to_double()).unwrap_or(0.0);
                    result.extend_from_slice(&v.to_le_bytes());
                    arg_idx += 1;
                }
            }
            b'a' | b'A' => {
                // NUL-padded / space-padded string
                let s = args
                    .get(arg_idx)
                    .map(|v| v.to_php_string())
                    .unwrap_or_else(PhpString::empty);
                arg_idx += 1;
                let pad = if code == b'A' { b' ' } else { b'\0' };
                if count == u32::MAX {
                    // a*/A* = full string length
                    result.extend_from_slice(s.as_bytes());
                } else {
                    let len = count as usize;
                    if s.len() >= len {
                        result.extend_from_slice(&s.as_bytes()[..len]);
                    } else {
                        result.extend_from_slice(s.as_bytes());
                        for _ in 0..(len - s.len()) {
                            result.push(pad);
                        }
                    }
                }
            }
            b'H' | b'h' => {
                // Hex string
                let s = args
                    .get(arg_idx)
                    .map(|v| v.to_php_string())
                    .unwrap_or_else(PhpString::empty);
                arg_idx += 1;
                let hex = s.as_bytes();
                let nibbles = (count as usize).min(hex.len());
                if code == b'H' {
                    // High nibble first
                    for j in (0..nibbles).step_by(2) {
                        let hi = hex_val(hex.get(j).copied().unwrap_or(b'0'));
                        let lo = hex_val(hex.get(j + 1).copied().unwrap_or(b'0'));
                        result.push(hi * 16 + lo);
                    }
                } else {
                    // Low nibble first
                    for j in (0..nibbles).step_by(2) {
                        let lo = hex_val(hex.get(j).copied().unwrap_or(b'0'));
                        let hi = hex_val(hex.get(j + 1).copied().unwrap_or(b'0'));
                        result.push(hi * 16 + lo);
                    }
                }
            }
            b'x' => {
                // NUL byte
                for _ in 0..repeat {
                    result.push(0);
                }
            }
            b'Z' => {
                // NUL-padded string (NUL terminated)
                let s = args
                    .get(arg_idx)
                    .map(|v| v.to_php_string())
                    .unwrap_or_else(PhpString::empty);
                arg_idx += 1;
                if count == u32::MAX {
                    // Z* = full string + NUL terminator
                    result.extend_from_slice(s.as_bytes());
                    result.push(0);
                } else {
                    let len = count as usize;
                    if len == 0 {
                        // Z0 = empty (no output)
                    } else if s.len() >= len {
                        result.extend_from_slice(&s.as_bytes()[..len - 1]);
                        result.push(0);
                    } else {
                        result.extend_from_slice(s.as_bytes());
                        for _ in 0..(len - s.len()) {
                            result.push(0);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    Ok(Value::String(PhpString::from_vec(result)))
}

fn hex_val(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => 0,
    }
}

fn unpack_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    use goro_core::array::{ArrayKey, PhpArray};
    use std::cell::RefCell;
    use std::rc::Rc;

    let format = args.first().unwrap_or(&Value::Null).to_php_string();
    let data = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let offset = args.get(2).map(|v| v.to_long()).unwrap_or(0) as usize;
    let bytes = &data.as_bytes()[offset.min(data.len())..];
    let fmt = format.as_bytes();
    let mut result = PhpArray::new();
    let mut pos = 0;
    let mut i = 0;
    let mut field_num = 1u32;

    while i < fmt.len() {
        let code = fmt[i];
        i += 1;

        // Parse count
        let mut count = 1u32;
        let mut overflow = false;
        if i < fmt.len() && fmt[i] == b'*' {
            count = u32::MAX;
            i += 1;
        } else {
            let mut has_digits = false;
            let mut n = 0u64;
            while i < fmt.len() && fmt[i].is_ascii_digit() {
                n = n.saturating_mul(10).saturating_add((fmt[i] - b'0') as u64);
                has_digits = true;
                i += 1;
            }
            if has_digits {
                if n > i32::MAX as u64 {
                    overflow = true;
                }
                count = n as u32;
            }
        }
        if overflow {
            _vm.emit_warning(&format!("unpack(): Type {}: integer overflow", code as char));
            return Ok(Value::False);
        }

        // Parse optional name
        let name = if i < fmt.len() && fmt[i] != b'/' {
            let start = i;
            while i < fmt.len() && fmt[i] != b'/' {
                i += 1;
            }
            Some(String::from_utf8_lossy(&fmt[start..i]).to_string())
        } else {
            None
        };
        if i < fmt.len() && fmt[i] == b'/' {
            i += 1;
        }

        match code {
            b'C' | b'c' => {
                let repeat = if count == u32::MAX {
                    bytes.len() - pos
                } else {
                    count as usize
                };
                for j in 0..repeat {
                    if pos < bytes.len() {
                        let v = if code == b'c' {
                            bytes[pos] as i8 as i64
                        } else {
                            bytes[pos] as i64
                        };
                        let k = if let Some(ref n) = name {
                            if count > 1 || count == u32::MAX {
                                ArrayKey::String(PhpString::from_string(format!("{}{}", n, j + 1)))
                            } else {
                                ArrayKey::String(PhpString::from_string(n.clone()))
                            }
                        } else {
                            ArrayKey::Int(field_num as i64)
                        };
                        result.set(k, Value::Long(v));
                        field_num += 1;
                        pos += 1;
                    }
                }
            }
            b'n' => {
                for j in 0..count.min((bytes.len() - pos) as u32 / 2) {
                    if pos + 2 <= bytes.len() {
                        let v = u16::from_be_bytes([bytes[pos], bytes[pos + 1]]) as i64;
                        let k = name
                            .as_ref()
                            .map(|n| {
                                ArrayKey::String(PhpString::from_string(if count > 1 {
                                    format!("{}{}", n, j + 1)
                                } else {
                                    n.clone()
                                }))
                            })
                            .unwrap_or(ArrayKey::Int(field_num as i64));
                        result.set(k, Value::Long(v));
                        field_num += 1;
                        pos += 2;
                    }
                }
            }
            b'N' => {
                for j in 0..count.min((bytes.len() - pos) as u32 / 4) {
                    if pos + 4 <= bytes.len() {
                        let v = u32::from_be_bytes([
                            bytes[pos],
                            bytes[pos + 1],
                            bytes[pos + 2],
                            bytes[pos + 3],
                        ]) as i64;
                        let k = name
                            .as_ref()
                            .map(|n| {
                                ArrayKey::String(PhpString::from_string(if count > 1 {
                                    format!("{}{}", n, j + 1)
                                } else {
                                    n.clone()
                                }))
                            })
                            .unwrap_or(ArrayKey::Int(field_num as i64));
                        result.set(k, Value::Long(v));
                        field_num += 1;
                        pos += 4;
                    }
                }
            }
            b'a' | b'A' => {
                let len = if count == u32::MAX {
                    bytes.len() - pos
                } else {
                    count as usize
                };
                let end = (pos + len).min(bytes.len());
                let mut s = bytes[pos..end].to_vec();
                if code == b'A' {
                    while s.last() == Some(&b' ') || s.last() == Some(&0) {
                        s.pop();
                    }
                }
                let k = name
                    .as_ref()
                    .map(|n| ArrayKey::String(PhpString::from_string(n.clone())))
                    .unwrap_or(ArrayKey::Int(field_num as i64));
                result.set(k, Value::String(PhpString::from_vec(s)));
                field_num += 1;
                pos = end;
            }
            b'S' | b'v' => {
                // S = unsigned 16-bit machine byte order, v = unsigned 16-bit little-endian
                let repeat = if count == u32::MAX {
                    (bytes.len() - pos) / 2
                } else {
                    count as usize
                };
                for j in 0..repeat {
                    if pos + 2 <= bytes.len() {
                        let v = if code == b'v' {
                            u16::from_le_bytes([bytes[pos], bytes[pos + 1]]) as i64
                        } else {
                            u16::from_ne_bytes([bytes[pos], bytes[pos + 1]]) as i64
                        };
                        let k = if let Some(ref n) = name {
                            if repeat > 1 { ArrayKey::String(PhpString::from_string(format!("{}{}", n, j + 1))) }
                            else { ArrayKey::String(PhpString::from_string(n.clone())) }
                        } else { ArrayKey::Int(field_num as i64) };
                        result.set(k, Value::Long(v));
                        field_num += 1;
                        pos += 2;
                    }
                }
            }
            b'l' | b'i' => {
                // l = signed 32-bit machine byte order, i = signed 32-bit machine byte order
                let repeat = if count == u32::MAX {
                    (bytes.len() - pos) / 4
                } else {
                    count as usize
                };
                for j in 0..repeat {
                    if pos + 4 <= bytes.len() {
                        let v = i32::from_ne_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]]) as i64;
                        let k = if let Some(ref n) = name {
                            if repeat > 1 { ArrayKey::String(PhpString::from_string(format!("{}{}", n, j + 1))) }
                            else { ArrayKey::String(PhpString::from_string(n.clone())) }
                        } else { ArrayKey::Int(field_num as i64) };
                        result.set(k, Value::Long(v));
                        field_num += 1;
                        pos += 4;
                    }
                }
            }
            b's' => {
                // s = signed 16-bit machine byte order
                let repeat = if count == u32::MAX {
                    (bytes.len() - pos) / 2
                } else {
                    count as usize
                };
                for j in 0..repeat {
                    if pos + 2 <= bytes.len() {
                        let v = i16::from_ne_bytes([bytes[pos], bytes[pos + 1]]) as i64;
                        let k = if let Some(ref n) = name {
                            if repeat > 1 { ArrayKey::String(PhpString::from_string(format!("{}{}", n, j + 1))) }
                            else { ArrayKey::String(PhpString::from_string(n.clone())) }
                        } else { ArrayKey::Int(field_num as i64) };
                        result.set(k, Value::Long(v));
                        field_num += 1;
                        pos += 2;
                    }
                }
            }
            b'L' | b'I' => {
                // L = unsigned 32-bit machine byte order, I = unsigned 32-bit machine byte order
                let repeat = if count == u32::MAX {
                    (bytes.len() - pos) / 4
                } else {
                    count as usize
                };
                for j in 0..repeat {
                    if pos + 4 <= bytes.len() {
                        let v = u32::from_ne_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]]) as i64;
                        let k = if let Some(ref n) = name {
                            if repeat > 1 { ArrayKey::String(PhpString::from_string(format!("{}{}", n, j + 1))) }
                            else { ArrayKey::String(PhpString::from_string(n.clone())) }
                        } else { ArrayKey::Int(field_num as i64) };
                        result.set(k, Value::Long(v));
                        field_num += 1;
                        pos += 4;
                    }
                }
            }
            b'V' => {
                // V = unsigned 32-bit little-endian
                let repeat = if count == u32::MAX {
                    (bytes.len() - pos) / 4
                } else {
                    count as usize
                };
                for j in 0..repeat {
                    if pos + 4 <= bytes.len() {
                        let v = u32::from_le_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]]) as i64;
                        let k = if let Some(ref n) = name {
                            if repeat > 1 { ArrayKey::String(PhpString::from_string(format!("{}{}", n, j + 1))) }
                            else { ArrayKey::String(PhpString::from_string(n.clone())) }
                        } else { ArrayKey::Int(field_num as i64) };
                        result.set(k, Value::Long(v));
                        field_num += 1;
                        pos += 4;
                    }
                }
            }
            b'Q' | b'q' => {
                // Q = unsigned 64-bit machine, q = signed 64-bit machine
                let repeat = if count == u32::MAX {
                    (bytes.len() - pos) / 8
                } else {
                    count as usize
                };
                for j in 0..repeat {
                    if pos + 8 <= bytes.len() {
                        let v = if code == b'q' {
                            i64::from_ne_bytes([bytes[pos], bytes[pos+1], bytes[pos+2], bytes[pos+3],
                                               bytes[pos+4], bytes[pos+5], bytes[pos+6], bytes[pos+7]])
                        } else {
                            u64::from_ne_bytes([bytes[pos], bytes[pos+1], bytes[pos+2], bytes[pos+3],
                                               bytes[pos+4], bytes[pos+5], bytes[pos+6], bytes[pos+7]]) as i64
                        };
                        let k = if let Some(ref n) = name {
                            if repeat > 1 { ArrayKey::String(PhpString::from_string(format!("{}{}", n, j + 1))) }
                            else { ArrayKey::String(PhpString::from_string(n.clone())) }
                        } else { ArrayKey::Int(field_num as i64) };
                        result.set(k, Value::Long(v));
                        field_num += 1;
                        pos += 8;
                    }
                }
            }
            b'P' => {
                // P = unsigned 64-bit little-endian
                let repeat = if count == u32::MAX {
                    (bytes.len() - pos) / 8
                } else {
                    count as usize
                };
                for j in 0..repeat {
                    if pos + 8 <= bytes.len() {
                        let v = u64::from_le_bytes([bytes[pos], bytes[pos+1], bytes[pos+2], bytes[pos+3],
                                                   bytes[pos+4], bytes[pos+5], bytes[pos+6], bytes[pos+7]]) as i64;
                        let k = if let Some(ref n) = name {
                            if repeat > 1 { ArrayKey::String(PhpString::from_string(format!("{}{}", n, j + 1))) }
                            else { ArrayKey::String(PhpString::from_string(n.clone())) }
                        } else { ArrayKey::Int(field_num as i64) };
                        result.set(k, Value::Long(v));
                        field_num += 1;
                        pos += 8;
                    }
                }
            }
            b'J' => {
                // J = unsigned 64-bit big-endian
                let repeat = if count == u32::MAX {
                    (bytes.len() - pos) / 8
                } else {
                    count as usize
                };
                for j in 0..repeat {
                    if pos + 8 <= bytes.len() {
                        let v = u64::from_be_bytes([bytes[pos], bytes[pos+1], bytes[pos+2], bytes[pos+3],
                                                   bytes[pos+4], bytes[pos+5], bytes[pos+6], bytes[pos+7]]) as i64;
                        let k = if let Some(ref n) = name {
                            if repeat > 1 { ArrayKey::String(PhpString::from_string(format!("{}{}", n, j + 1))) }
                            else { ArrayKey::String(PhpString::from_string(n.clone())) }
                        } else { ArrayKey::Int(field_num as i64) };
                        result.set(k, Value::Long(v));
                        field_num += 1;
                        pos += 8;
                    }
                }
            }
            b'f' | b'g' => {
                // f = float machine byte order, g = float little-endian
                let repeat = if count == u32::MAX {
                    (bytes.len() - pos) / 4
                } else {
                    count as usize
                };
                for j in 0..repeat {
                    if pos + 4 <= bytes.len() {
                        let v = if code == b'g' {
                            f32::from_le_bytes([bytes[pos], bytes[pos+1], bytes[pos+2], bytes[pos+3]]) as f64
                        } else {
                            f32::from_ne_bytes([bytes[pos], bytes[pos+1], bytes[pos+2], bytes[pos+3]]) as f64
                        };
                        let k = if let Some(ref n) = name {
                            if repeat > 1 { ArrayKey::String(PhpString::from_string(format!("{}{}", n, j + 1))) }
                            else { ArrayKey::String(PhpString::from_string(n.clone())) }
                        } else { ArrayKey::Int(field_num as i64) };
                        result.set(k, Value::Double(v));
                        field_num += 1;
                        pos += 4;
                    }
                }
            }
            b'd' | b'e' | b'E' | b'G' => {
                // d = double machine, e = double little-endian, E = double big-endian, G = double big-endian
                let repeat = if count == u32::MAX {
                    (bytes.len() - pos) / 8
                } else {
                    count as usize
                };
                for j in 0..repeat {
                    if pos + 8 <= bytes.len() {
                        let v = match code {
                            b'e' => f64::from_le_bytes([bytes[pos], bytes[pos+1], bytes[pos+2], bytes[pos+3],
                                                       bytes[pos+4], bytes[pos+5], bytes[pos+6], bytes[pos+7]]),
                            b'E' | b'G' => f64::from_be_bytes([bytes[pos], bytes[pos+1], bytes[pos+2], bytes[pos+3],
                                                              bytes[pos+4], bytes[pos+5], bytes[pos+6], bytes[pos+7]]),
                            _ => f64::from_ne_bytes([bytes[pos], bytes[pos+1], bytes[pos+2], bytes[pos+3],
                                                   bytes[pos+4], bytes[pos+5], bytes[pos+6], bytes[pos+7]]),
                        };
                        let k = if let Some(ref n) = name {
                            if repeat > 1 { ArrayKey::String(PhpString::from_string(format!("{}{}", n, j + 1))) }
                            else { ArrayKey::String(PhpString::from_string(n.clone())) }
                        } else { ArrayKey::Int(field_num as i64) };
                        result.set(k, Value::Double(v));
                        field_num += 1;
                        pos += 8;
                    }
                }
            }
            b'H' | b'h' => {
                // H = hex string, high nibble first; h = hex string, low nibble first
                let len = if count == u32::MAX {
                    (bytes.len() - pos) * 2
                } else {
                    count as usize
                };
                let bytes_needed = (len + 1) / 2;
                let end = (pos + bytes_needed).min(bytes.len());
                let actual_capacity = ((end - pos) * 2).min(len);
                if bytes_needed > bytes.len().saturating_sub(pos) {
                    _vm.emit_warning(&format!(
                        "unpack(): Type {}: not enough input values, need {} values but only {} were provided",
                        code as char, bytes_needed, bytes.len()
                    ));
                }
                let mut hex = String::with_capacity(actual_capacity);
                for idx in pos..end {
                    let byte = bytes[idx];
                    if code == b'H' {
                        hex.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
                        if hex.len() < len {
                            hex.push(char::from(b"0123456789abcdef"[(byte & 0xf) as usize]));
                        }
                    } else {
                        hex.push(char::from(b"0123456789abcdef"[(byte & 0xf) as usize]));
                        if hex.len() < len {
                            hex.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
                        }
                    }
                }
                let k = name
                    .as_ref()
                    .map(|n| ArrayKey::String(PhpString::from_string(n.clone())))
                    .unwrap_or(ArrayKey::Int(field_num as i64));
                result.set(k, Value::String(PhpString::from_string(hex)));
                field_num += 1;
                pos = end;
            }
            b'Z' => {
                // Z = NUL-padded string
                let len = if count == u32::MAX {
                    bytes.len() - pos
                } else {
                    count as usize
                };
                let end = (pos + len).min(bytes.len());
                let mut s = bytes[pos..end].to_vec();
                // Truncate at first NUL
                if let Some(nul_pos) = s.iter().position(|&b| b == 0) {
                    s.truncate(nul_pos);
                }
                let k = name
                    .as_ref()
                    .map(|n| ArrayKey::String(PhpString::from_string(n.clone())))
                    .unwrap_or(ArrayKey::Int(field_num as i64));
                result.set(k, Value::String(PhpString::from_vec(s)));
                field_num += 1;
                pos = end;
            }
            b'x' => {
                // x = NUL byte (skip forward)
                let repeat = if count == u32::MAX {
                    bytes.len() - pos
                } else {
                    count as usize
                };
                pos = (pos + repeat).min(bytes.len());
            }
            b'X' => {
                // X = back up one byte
                if count == u32::MAX {
                    _vm.emit_warning("unpack(): Type X: '*' ignored");
                    // Don't move position
                } else {
                    pos = pos.saturating_sub(count as usize);
                }
            }
            b'@' => {
                // @ = NUL-fill to absolute position
                if count == u32::MAX {
                    pos = bytes.len();
                } else {
                    pos = (count as usize).min(bytes.len());
                }
            }
            _ => {
                // Truly unknown format code - skip
                pos += 1;
            }
        }
    }

    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn str_decrement(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return Err(VmError {
            message: "str_decrement(): Argument #1 ($string) must not be empty".into(),
            line: 0,
        });
    }
    if bytes == b"a" || bytes == b"A" || bytes == b"0" {
        return Err(VmError {
            message: format!(
                "str_decrement(): Argument #1 ($string) \"{}\" is out of decrement range",
                s.to_string_lossy()
            ),
            line: 0,
        });
    }
    let mut result = bytes.to_vec();
    let mut i = result.len() - 1;
    loop {
        let ch = result[i];
        match ch {
            b'1'..=b'9' => { result[i] -= 1; break; }
            b'0' => {
                if i == 0 {
                    if result.len() > 1 {
                        result.remove(0);
                    }
                    break;
                }
                result[i] = b'9';
                i -= 1;
            }
            b'b'..=b'z' => { result[i] -= 1; break; }
            b'a' => {
                if i == 0 {
                    // Underflow at position 0 - remove this char
                    if result.len() > 1 {
                        result.remove(0);
                    }
                    break;
                }
                result[i] = b'z';
                i -= 1;
            }
            b'B'..=b'Z' => { result[i] -= 1; break; }
            b'A' => {
                if i == 0 {
                    if result.len() > 1 {
                        result.remove(0);
                    }
                    break;
                }
                result[i] = b'Z';
                i -= 1;
            }
            _ => break,
        }
    }
    // Remove leading char when it became the "zero" value through borrow
    // e.g. "10" -> "09" -> "9", "aa" -> "az" -> "z" (first 'a' is min for alpha)
    if result.len() > 1 && result.len() == bytes.len() {
        let first = result[0];
        let orig_first = bytes[0];
        // If the first char wrapped around to min value, remove it
        if (first == b'0' && orig_first == b'1')
            || (first == b'a' && orig_first == b'a')
            || (first == b'A' && orig_first == b'A')
        {
            result.remove(0);
        }
    }
    Ok(Value::String(PhpString::from_vec(result)))
}

fn quotemeta_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = s.as_bytes();
    let mut result = Vec::with_capacity(bytes.len() * 2);
    for &b in bytes {
        match b {
            b'.' | b'\\' | b'+' | b'*' | b'?' | b'[' | b'^' | b']' | b'(' | b')' | b'$' => {
                result.push(b'\\');
                result.push(b);
            }
            _ => result.push(b),
        }
    }
    Ok(Value::String(PhpString::from_vec(result)))
}

fn utf8_decode_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    vm.emit_deprecated_at("Function utf8_decode() is deprecated since 8.2, visit the php.net documentation for various alternatives", 0);
    // Convert UTF-8 to ISO-8859-1 (simplified: just keep bytes < 256)
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let utf8 = s.to_string_lossy();
    let mut result = Vec::new();
    for ch in utf8.chars() {
        if ch as u32 <= 255 {
            result.push(ch as u8);
        } else {
            result.push(b'?');
        }
    }
    Ok(Value::String(PhpString::from_vec(result)))
}

fn utf8_encode_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    vm.emit_deprecated_at("Function utf8_encode() is deprecated since 8.2, visit the php.net documentation for various alternatives", 0);
    // Convert ISO-8859-1 to UTF-8
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = s.as_bytes();
    let mut result = String::new();
    for &b in bytes {
        result.push(b as char);
    }
    Ok(Value::String(PhpString::from_string(result)))
}

fn get_html_translation_table_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let table_type = args.first().map(|v| v.to_long()).unwrap_or(0); // HTML_SPECIALCHARS=0, HTML_ENTITIES=1
    let flags = args.get(1).map(|v| v.to_long()).unwrap_or(11); // ENT_QUOTES|ENT_SUBSTITUTE

    // ENT_COMPAT=2 (double quotes), ENT_QUOTES=3 (both), ENT_NOQUOTES=0 (neither)
    let quote_style = flags & 3;

    let mut result = PhpArray::new();

    if table_type == 0 {
        // HTML_SPECIALCHARS
        result.set(ArrayKey::String(PhpString::from_bytes(b"&")), Value::String(PhpString::from_bytes(b"&amp;")));
        if quote_style & 2 != 0 {
            result.set(ArrayKey::String(PhpString::from_bytes(b"\"")), Value::String(PhpString::from_bytes(b"&quot;")));
        }
        result.set(ArrayKey::String(PhpString::from_bytes(b"<")), Value::String(PhpString::from_bytes(b"&lt;")));
        result.set(ArrayKey::String(PhpString::from_bytes(b">")), Value::String(PhpString::from_bytes(b"&gt;")));
        if quote_style & 1 != 0 {
            result.set(ArrayKey::String(PhpString::from_bytes(b"'")), Value::String(PhpString::from_bytes(b"&#039;")));
        }
    } else {
        // HTML_ENTITIES - include common entities
        result.set(ArrayKey::String(PhpString::from_bytes(b"&")), Value::String(PhpString::from_bytes(b"&amp;")));
        if quote_style & 2 != 0 {
            result.set(ArrayKey::String(PhpString::from_bytes(b"\"")), Value::String(PhpString::from_bytes(b"&quot;")));
        }
        result.set(ArrayKey::String(PhpString::from_bytes(b"<")), Value::String(PhpString::from_bytes(b"&lt;")));
        result.set(ArrayKey::String(PhpString::from_bytes(b">")), Value::String(PhpString::from_bytes(b"&gt;")));
        if quote_style & 1 != 0 {
            result.set(ArrayKey::String(PhpString::from_bytes(b"'")), Value::String(PhpString::from_bytes(b"&#039;")));
        }
        // Add more HTML entities for characters 160-255
        let entities = [
            (160, "&nbsp;"), (161, "&iexcl;"), (162, "&cent;"), (163, "&pound;"),
            (164, "&curren;"), (165, "&yen;"), (166, "&brvbar;"), (167, "&sect;"),
            (168, "&uml;"), (169, "&copy;"), (170, "&ordf;"), (171, "&laquo;"),
            (172, "&not;"), (173, "&shy;"), (174, "&reg;"), (175, "&macr;"),
            (176, "&deg;"), (177, "&plusmn;"), (178, "&sup2;"), (179, "&sup3;"),
            (180, "&acute;"), (181, "&micro;"), (182, "&para;"), (183, "&middot;"),
            (184, "&cedil;"), (185, "&sup1;"), (186, "&ordm;"), (187, "&raquo;"),
            (188, "&frac14;"), (189, "&frac12;"), (190, "&frac34;"), (191, "&iquest;"),
        ];
        for (code, entity) in &entities {
            let ch = [*code as u8];
            result.set(
                ArrayKey::String(PhpString::from_bytes(&ch)),
                Value::String(PhpString::from_string(entity.to_string())),
            );
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn html_entity_decode_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let flags = args.get(1).map(|v| v.to_long()).unwrap_or(3); // ENT_QUOTES | ENT_SUBSTITUTE
    let _encoding = args.get(2); // Ignored for now, assume UTF-8
    let bytes = s.as_bytes();
    let mut result = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'&' {
            // Try to find closing ;
            if let Some(semi_offset) = bytes[i..].iter().position(|&b| b == b';') {
                let entity = &bytes[i+1..i+semi_offset];
                let decoded = decode_html_entity(entity, flags);
                if let Some(decoded_bytes) = decoded {
                    result.extend_from_slice(&decoded_bytes);
                    i += semi_offset + 1;
                    continue;
                }
            }
        }
        result.push(bytes[i]);
        i += 1;
    }

    Ok(Value::String(PhpString::from_vec(result)))
}

fn decode_html_entity(entity: &[u8], flags: i64) -> Option<Vec<u8>> {
    // Numeric entities
    if entity.first() == Some(&b'#') {
        let num_str = &entity[1..];
        let codepoint = if num_str.first() == Some(&b'x') || num_str.first() == Some(&b'X') {
            // Hex
            let hex = std::str::from_utf8(&num_str[1..]).ok()?;
            u32::from_str_radix(hex, 16).ok()?
        } else {
            // Decimal
            let dec = std::str::from_utf8(num_str).ok()?;
            dec.parse::<u32>().ok()?
        };
        let ch = char::from_u32(codepoint)?;
        let mut buf = [0u8; 4];
        let encoded = ch.encode_utf8(&mut buf);
        return Some(encoded.as_bytes().to_vec());
    }

    // Named entities
    let entity_str = std::str::from_utf8(entity).ok()?;
    match entity_str {
        "amp" => Some(b"&".to_vec()),
        "lt" => Some(b"<".to_vec()),
        "gt" => Some(b">".to_vec()),
        "quot" if flags & 3 != 0 => Some(b"\"".to_vec()),
        "apos" if flags & (16 | 32) != 0 => Some(b"'".to_vec()),
        "nbsp" => Some("\u{00A0}".as_bytes().to_vec()),
        "iexcl" => Some("\u{00A1}".as_bytes().to_vec()),
        "cent" => Some("\u{00A2}".as_bytes().to_vec()),
        "pound" => Some("\u{00A3}".as_bytes().to_vec()),
        "curren" => Some("\u{00A4}".as_bytes().to_vec()),
        "yen" => Some("\u{00A5}".as_bytes().to_vec()),
        "brvbar" => Some("\u{00A6}".as_bytes().to_vec()),
        "sect" => Some("\u{00A7}".as_bytes().to_vec()),
        "uml" => Some("\u{00A8}".as_bytes().to_vec()),
        "copy" => Some("\u{00A9}".as_bytes().to_vec()),
        "ordf" => Some("\u{00AA}".as_bytes().to_vec()),
        "laquo" => Some("\u{00AB}".as_bytes().to_vec()),
        "not" => Some("\u{00AC}".as_bytes().to_vec()),
        "shy" => Some("\u{00AD}".as_bytes().to_vec()),
        "reg" => Some("\u{00AE}".as_bytes().to_vec()),
        "macr" => Some("\u{00AF}".as_bytes().to_vec()),
        "deg" => Some("\u{00B0}".as_bytes().to_vec()),
        "plusmn" => Some("\u{00B1}".as_bytes().to_vec()),
        "sup2" => Some("\u{00B2}".as_bytes().to_vec()),
        "sup3" => Some("\u{00B3}".as_bytes().to_vec()),
        "acute" => Some("\u{00B4}".as_bytes().to_vec()),
        "micro" => Some("\u{00B5}".as_bytes().to_vec()),
        "para" => Some("\u{00B6}".as_bytes().to_vec()),
        "middot" => Some("\u{00B7}".as_bytes().to_vec()),
        "cedil" => Some("\u{00B8}".as_bytes().to_vec()),
        "sup1" => Some("\u{00B9}".as_bytes().to_vec()),
        "ordm" => Some("\u{00BA}".as_bytes().to_vec()),
        "raquo" => Some("\u{00BB}".as_bytes().to_vec()),
        "frac14" => Some("\u{00BC}".as_bytes().to_vec()),
        "frac12" => Some("\u{00BD}".as_bytes().to_vec()),
        "frac34" => Some("\u{00BE}".as_bytes().to_vec()),
        "iquest" => Some("\u{00BF}".as_bytes().to_vec()),
        "times" => Some("\u{00D7}".as_bytes().to_vec()),
        "divide" => Some("\u{00F7}".as_bytes().to_vec()),
        "euro" => Some("\u{20AC}".as_bytes().to_vec()),
        "trade" => Some("\u{2122}".as_bytes().to_vec()),
        "ndash" => Some("\u{2013}".as_bytes().to_vec()),
        "mdash" => Some("\u{2014}".as_bytes().to_vec()),
        "lsquo" => Some("\u{2018}".as_bytes().to_vec()),
        "rsquo" => Some("\u{2019}".as_bytes().to_vec()),
        "ldquo" => Some("\u{201C}".as_bytes().to_vec()),
        "rdquo" => Some("\u{201D}".as_bytes().to_vec()),
        "bull" => Some("\u{2022}".as_bytes().to_vec()),
        "hellip" => Some("\u{2026}".as_bytes().to_vec()),
        "prime" => Some("\u{2032}".as_bytes().to_vec()),
        "Prime" => Some("\u{2033}".as_bytes().to_vec()),
        "lsaquo" => Some("\u{2039}".as_bytes().to_vec()),
        "rsaquo" => Some("\u{203A}".as_bytes().to_vec()),
        "oline" => Some("\u{203E}".as_bytes().to_vec()),
        "frasl" => Some("\u{2044}".as_bytes().to_vec()),
        "ensp" => Some("\u{2002}".as_bytes().to_vec()),
        "emsp" => Some("\u{2003}".as_bytes().to_vec()),
        "thinsp" => Some("\u{2009}".as_bytes().to_vec()),
        "dagger" => Some("\u{2020}".as_bytes().to_vec()),
        "Dagger" => Some("\u{2021}".as_bytes().to_vec()),
        "permil" => Some("\u{2030}".as_bytes().to_vec()),
        // Common accented chars
        "Agrave" => Some("\u{00C0}".as_bytes().to_vec()),
        "Aacute" => Some("\u{00C1}".as_bytes().to_vec()),
        "Acirc" => Some("\u{00C2}".as_bytes().to_vec()),
        "Atilde" => Some("\u{00C3}".as_bytes().to_vec()),
        "Auml" => Some("\u{00C4}".as_bytes().to_vec()),
        "Aring" => Some("\u{00C5}".as_bytes().to_vec()),
        "AElig" => Some("\u{00C6}".as_bytes().to_vec()),
        "Ccedil" => Some("\u{00C7}".as_bytes().to_vec()),
        "Egrave" => Some("\u{00C8}".as_bytes().to_vec()),
        "Eacute" => Some("\u{00C9}".as_bytes().to_vec()),
        "Ecirc" => Some("\u{00CA}".as_bytes().to_vec()),
        "Euml" => Some("\u{00CB}".as_bytes().to_vec()),
        "Igrave" => Some("\u{00CC}".as_bytes().to_vec()),
        "Iacute" => Some("\u{00CD}".as_bytes().to_vec()),
        "Icirc" => Some("\u{00CE}".as_bytes().to_vec()),
        "Iuml" => Some("\u{00CF}".as_bytes().to_vec()),
        "ETH" => Some("\u{00D0}".as_bytes().to_vec()),
        "Ntilde" => Some("\u{00D1}".as_bytes().to_vec()),
        "Ograve" => Some("\u{00D2}".as_bytes().to_vec()),
        "Oacute" => Some("\u{00D3}".as_bytes().to_vec()),
        "Ocirc" => Some("\u{00D4}".as_bytes().to_vec()),
        "Otilde" => Some("\u{00D5}".as_bytes().to_vec()),
        "Ouml" => Some("\u{00D6}".as_bytes().to_vec()),
        "Oslash" => Some("\u{00D8}".as_bytes().to_vec()),
        "Ugrave" => Some("\u{00D9}".as_bytes().to_vec()),
        "Uacute" => Some("\u{00DA}".as_bytes().to_vec()),
        "Ucirc" => Some("\u{00DB}".as_bytes().to_vec()),
        "Uuml" => Some("\u{00DC}".as_bytes().to_vec()),
        "Yacute" => Some("\u{00DD}".as_bytes().to_vec()),
        "THORN" => Some("\u{00DE}".as_bytes().to_vec()),
        "szlig" => Some("\u{00DF}".as_bytes().to_vec()),
        "agrave" => Some("\u{00E0}".as_bytes().to_vec()),
        "aacute" => Some("\u{00E1}".as_bytes().to_vec()),
        "acirc" => Some("\u{00E2}".as_bytes().to_vec()),
        "atilde" => Some("\u{00E3}".as_bytes().to_vec()),
        "auml" => Some("\u{00E4}".as_bytes().to_vec()),
        "aring" => Some("\u{00E5}".as_bytes().to_vec()),
        "aelig" => Some("\u{00E6}".as_bytes().to_vec()),
        "ccedil" => Some("\u{00E7}".as_bytes().to_vec()),
        "egrave" => Some("\u{00E8}".as_bytes().to_vec()),
        "eacute" => Some("\u{00E9}".as_bytes().to_vec()),
        "ecirc" => Some("\u{00EA}".as_bytes().to_vec()),
        "euml" => Some("\u{00EB}".as_bytes().to_vec()),
        "igrave" => Some("\u{00EC}".as_bytes().to_vec()),
        "iacute" => Some("\u{00ED}".as_bytes().to_vec()),
        "icirc" => Some("\u{00EE}".as_bytes().to_vec()),
        "iuml" => Some("\u{00EF}".as_bytes().to_vec()),
        "eth" => Some("\u{00F0}".as_bytes().to_vec()),
        "ntilde" => Some("\u{00F1}".as_bytes().to_vec()),
        "ograve" => Some("\u{00F2}".as_bytes().to_vec()),
        "oacute" => Some("\u{00F3}".as_bytes().to_vec()),
        "ocirc" => Some("\u{00F4}".as_bytes().to_vec()),
        "otilde" => Some("\u{00F5}".as_bytes().to_vec()),
        "ouml" => Some("\u{00F6}".as_bytes().to_vec()),
        "oslash" => Some("\u{00F8}".as_bytes().to_vec()),
        "ugrave" => Some("\u{00F9}".as_bytes().to_vec()),
        "uacute" => Some("\u{00FA}".as_bytes().to_vec()),
        "ucirc" => Some("\u{00FB}".as_bytes().to_vec()),
        "uuml" => Some("\u{00FC}".as_bytes().to_vec()),
        "yacute" => Some("\u{00FD}".as_bytes().to_vec()),
        "thorn" => Some("\u{00FE}".as_bytes().to_vec()),
        "yuml" => Some("\u{00FF}".as_bytes().to_vec()),
        _ => None,
    }
}

fn strip_tags_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let allowed = args.get(1);

    // Build set of allowed tag names (lowercase)
    let mut allowed_tags: Vec<Vec<u8>> = Vec::new();
    if let Some(allowed_val) = allowed {
        match allowed_val {
            Value::String(allowed_str) => {
                let ab = allowed_str.as_bytes();
                let mut j = 0;
                while j < ab.len() {
                    if ab[j] == b'<' {
                        j += 1;
                        let start = j;
                        while j < ab.len() && ab[j] != b'>' && ab[j] != b' ' {
                            j += 1;
                        }
                        if start < j {
                            allowed_tags.push(ab[start..j].to_ascii_lowercase());
                        }
                        while j < ab.len() && ab[j] != b'>' {
                            j += 1;
                        }
                        if j < ab.len() { j += 1; }
                    } else {
                        j += 1;
                    }
                }
            }
            Value::Array(arr) => {
                let arr = arr.borrow();
                for (_, v) in arr.iter() {
                    let tag = v.to_php_string().to_string_lossy().to_ascii_lowercase();
                    allowed_tags.push(tag.into_bytes());
                }
            }
            _ => {}
        }
    }

    let bytes = s.as_bytes();
    let mut result = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        // PHP strip_tags strips NUL bytes
        if bytes[i] == 0 {
            i += 1;
            continue;
        }
        if bytes[i] == b'<' {
            // Check if this could be a tag: must be followed by ?, !, /, %, or alpha
            if i + 1 < bytes.len() {
                let next = bytes[i + 1];
                if next != b'?' && next != b'!' && next != b'/' && next != b'%' && !next.is_ascii_alphabetic() {
                    // Not a tag, output as literal
                    result.push(bytes[i]);
                    i += 1;
                    continue;
                }
            } else {
                // < at end of string - output literally
                result.push(bytes[i]);
                i += 1;
                continue;
            }

            // Check for PHP/XML processing instructions and ASP-style tags
            if i + 1 < bytes.len() && (bytes[i + 1] == b'?' || bytes[i + 1] == b'%') {
                // Check if this is an XML processing instruction: <?xml (case-insensitive)
                // <?xml ends at first >, all other <? tags end at ?>
                let is_xml_pi = i + 4 < bytes.len()
                    && bytes[i + 2..i + 5].eq_ignore_ascii_case(b"xml")
                    && (i + 5 >= bytes.len() || !bytes[i + 5].is_ascii_alphanumeric());
                if is_xml_pi {
                    // XML PI - skip until first >
                    i += 2;
                    while i < bytes.len() {
                        if bytes[i] == b'>' {
                            i += 1;
                            break;
                        }
                        i += 1;
                    }
                } else {
                    // PHP/ASP tag - skip until ?> or %> respectively
                    let close_char = bytes[i + 1]; // ? or %
                    i += 2;
                    while i < bytes.len() {
                        if bytes[i] == close_char && i + 1 < bytes.len() && bytes[i + 1] == b'>' {
                            i += 2;
                            break;
                        }
                        i += 1;
                    }
                }
                continue;
            }

            // Check for HTML comments: <!-- ... -->
            if i + 3 < bytes.len() && bytes[i + 1] == b'!' && bytes[i + 2] == b'-' && bytes[i + 3] == b'-' {
                // HTML comment - skip until -->
                i += 4;
                while i < bytes.len() {
                    if bytes[i] == b'-' && i + 2 < bytes.len() && bytes[i + 1] == b'-' && bytes[i + 2] == b'>' {
                        i += 3;
                        break;
                    }
                    i += 1;
                }
                continue;
            }

            // Check for SGML/XML declarations: <! ... >
            if i + 1 < bytes.len() && bytes[i + 1] == b'!' {
                // Skip until >
                i += 2;
                while i < bytes.len() && bytes[i] != b'>' {
                    i += 1;
                }
                if i < bytes.len() { i += 1; }
                continue;
            }

            let tag_start = i;
            i += 1;
            let is_closing = i < bytes.len() && bytes[i] == b'/';
            if is_closing { i += 1; }

            let name_start = i;
            while i < bytes.len() && bytes[i] != b'>' && bytes[i] != b' ' && bytes[i] != b'\t' && bytes[i] != b'\n' && bytes[i] != b'\r' && bytes[i] != b'/' && bytes[i] != b'<' {
                i += 1;
            }
            let tag_name = &bytes[name_start..i];
            let tag_name_lower = tag_name.to_ascii_lowercase();

            // Skip attributes/rest of tag, handling quoted strings
            // PHP strip_tags tracks nested <> with a depth counter:
            // each '<' inside a tag increments depth, each '>' decrements it,
            // and the tag only ends when '>' is seen at depth 0.
            let mut in_quote: u8 = 0;
            let mut depth: u32 = 0;
            while i < bytes.len() {
                if in_quote != 0 {
                    if bytes[i] == in_quote {
                        in_quote = 0;
                    }
                } else if bytes[i] == b'"' || bytes[i] == b'\'' {
                    in_quote = bytes[i];
                } else if bytes[i] == b'<' {
                    depth += 1;
                } else if bytes[i] == b'>' {
                    if depth > 0 {
                        depth -= 1;
                    } else {
                        break;
                    }
                }
                i += 1;
            }
            if i < bytes.len() { i += 1; }

            if !allowed_tags.is_empty() && !tag_name_lower.is_empty() && allowed_tags.iter().any(|t| t == &tag_name_lower) {
                result.extend_from_slice(&bytes[tag_start..i]);
            }
        } else {
            result.push(bytes[i]);
            i += 1;
        }
    }
    Ok(Value::String(PhpString::from_vec(result)))
}

fn nl2br_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let is_xhtml = args.get(1).map(|v| v.is_truthy()).unwrap_or(true);
    let br = if is_xhtml { b"<br />" as &[u8] } else { b"<br>" as &[u8] };
    let bytes = s.as_bytes();
    let mut result = Vec::with_capacity(bytes.len() * 2);
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' {
            result.extend_from_slice(br);
            if i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
                // \r\n - emit both chars but only one <br>
                result.push(b'\r');
                result.push(b'\n');
                i += 2;
            } else {
                // standalone \r
                result.push(b'\r');
                i += 1;
            }
        } else if bytes[i] == b'\n' {
            result.extend_from_slice(br);
            if i + 1 < bytes.len() && bytes[i + 1] == b'\r' {
                // \n\r - emit both chars but only one <br>
                result.push(b'\n');
                result.push(b'\r');
                i += 2;
            } else {
                result.push(b'\n');
                i += 1;
            }
        } else {
            result.push(bytes[i]);
            i += 1;
        }
    }
    Ok(Value::String(PhpString::from_vec(result)))
}

pub fn str_getcsv_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();

    // Validate separator must be a single character
    if let Some(sep_val) = args.get(1) {
        let sep_str = sep_val.to_php_string();
        if sep_str.len() != 1 {
            let msg = "str_getcsv(): Argument #2 ($separator) must be a single character".to_string();
            let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
        }
    }
    // Validate enclosure must be a single character
    if let Some(enc_val) = args.get(2) {
        let enc_str = enc_val.to_php_string();
        if enc_str.len() != 1 {
            let msg = "str_getcsv(): Argument #3 ($enclosure) must be a single character".to_string();
            let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
        }
    }
    // Validate escape must be a single character or empty string
    if let Some(esc_val) = args.get(3) {
        let esc_str = esc_val.to_php_string();
        if esc_str.len() > 1 {
            let msg = "str_getcsv(): Argument #4 ($escape) must be empty or a single character".to_string();
            let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
        }
    }

    let separator = args.get(1).map(|v| v.to_php_string().as_bytes().first().copied().unwrap_or(b',')).unwrap_or(b',');
    let enclosure = args.get(2).map(|v| v.to_php_string().as_bytes().first().copied().unwrap_or(b'"')).unwrap_or(b'"');

    let mut result = PhpArray::new();
    let raw_bytes = s.as_bytes();
    // Strip trailing newline like PHP does
    let bytes = if raw_bytes.last() == Some(&b'\n') {
        let end = if raw_bytes.len() >= 2 && raw_bytes[raw_bytes.len()-2] == b'\r' {
            raw_bytes.len() - 2
        } else {
            raw_bytes.len() - 1
        };
        &raw_bytes[..end]
    } else {
        raw_bytes
    };
    let mut field = Vec::new();
    let mut in_enclosure = false;
    let mut i = 0;
    
    while i < bytes.len() {
        if bytes[i] == enclosure {
            if in_enclosure && i + 1 < bytes.len() && bytes[i + 1] == enclosure {
                field.push(enclosure);
                i += 2;
            } else {
                in_enclosure = !in_enclosure;
                i += 1;
            }
        } else if bytes[i] == separator && !in_enclosure {
            result.push(Value::String(PhpString::from_vec(field.clone())));
            field.clear();
            i += 1;
        } else {
            field.push(bytes[i]);
            i += 1;
        }
    }
    result.push(Value::String(PhpString::from_vec(field)));
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn str_word_count_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let format = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let input = s.to_string_lossy();
    
    let words: Vec<&str> = input.split(|c: char| !c.is_alphabetic() && c != '-' && c != '\'')
        .filter(|w| !w.is_empty())
        .collect();
    
    match format {
        0 => Ok(Value::Long(words.len() as i64)),
        1 => {
            let mut arr = PhpArray::new();
            for word in &words {
                arr.push(Value::String(PhpString::from_string(word.to_string())));
            }
            Ok(Value::Array(Rc::new(RefCell::new(arr))))
        }
        2 => {
            let mut arr = PhpArray::new();
            let mut pos = 0;
            let input_bytes = input.as_bytes();
            for word in &words {
                if let Some(idx) = input[pos..].find(word) {
                    arr.set(
                        ArrayKey::Int((pos + idx) as i64),
                        Value::String(PhpString::from_string(word.to_string())),
                    );
                    pos = pos + idx + word.len();
                }
            }
            Ok(Value::Array(Rc::new(RefCell::new(arr))))
        }
        _ => Ok(Value::False),
    }
}

fn convert_uuencode_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    convert_uuencode(vm, args)
}

fn convert_uudecode_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    convert_uudecode(vm, args)
}

fn quoted_printable_encode_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = s.as_bytes();
    let mut result = Vec::new();
    let mut line_len: usize = 0;

    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];

        // Handle CRLF line breaks - output as-is and reset line counter
        if b == b'\r' && i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
            // Encode trailing whitespace before hard line break
            while result.last() == Some(&b' ') || result.last() == Some(&b'\t') {
                let ws = result.pop().unwrap();
                result.push(b'=');
                result.push(b"0123456789ABCDEF"[(ws >> 4) as usize]);
                result.push(b"0123456789ABCDEF"[(ws & 0x0f) as usize]);
            }
            result.push(b'\r');
            result.push(b'\n');
            line_len = 0;
            i += 2;
            continue;
        }
        if b == b'\n' {
            while result.last() == Some(&b' ') || result.last() == Some(&b'\t') {
                let ws = result.pop().unwrap();
                result.push(b'=');
                result.push(b"0123456789ABCDEF"[(ws >> 4) as usize]);
                result.push(b"0123456789ABCDEF"[(ws & 0x0f) as usize]);
            }
            result.push(b'\r');
            result.push(b'\n');
            line_len = 0;
            i += 1;
            continue;
        }
        if b == b'\r' {
            while result.last() == Some(&b' ') || result.last() == Some(&b'\t') {
                let ws = result.pop().unwrap();
                result.push(b'=');
                result.push(b"0123456789ABCDEF"[(ws >> 4) as usize]);
                result.push(b"0123456789ABCDEF"[(ws & 0x0f) as usize]);
            }
            result.push(b'\r');
            result.push(b'\n');
            line_len = 0;
            i += 1;
            continue;
        }

        let is_printable = b == b'\t' || (b >= 32 && b <= 126 && b != b'=');
        let char_len: usize = if is_printable { 1 } else { 3 };

        // Check if we need a soft line break
        if line_len + char_len > 75 {
            result.extend_from_slice(b"=\r\n");
            line_len = 0;
        }

        if is_printable {
            result.push(b);
            line_len += 1;
        } else {
            result.push(b'=');
            result.push(b"0123456789ABCDEF"[(b >> 4) as usize]);
            result.push(b"0123456789ABCDEF"[(b & 0x0f) as usize]);
            line_len += 3;
        }

        i += 1;
    }

    // Encode trailing whitespace at end of string
    while result.last() == Some(&b' ') || result.last() == Some(&b'\t') {
        let ws = result.pop().unwrap();
        result.push(b'=');
        result.push(b"0123456789ABCDEF"[(ws >> 4) as usize]);
        result.push(b"0123456789ABCDEF"[(ws & 0x0f) as usize]);
    }

    Ok(Value::String(PhpString::from_vec(result)))
}

fn quoted_printable_decode_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = s.as_bytes();
    let mut result = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'=' && i + 2 < bytes.len() {
            if bytes[i + 1] == b'\r' && i + 2 < bytes.len() && bytes[i + 2] == b'\n' {
                i += 3; // soft line break
            } else if bytes[i + 1] == b'\n' {
                i += 2; // soft line break
            } else {
                let hi = hex_nibble(bytes[i + 1]);
                let lo = hex_nibble(bytes[i + 2]);
                if let (Some(h), Some(l)) = (hi, lo) {
                    result.push((h << 4) | l);
                } else {
                    result.push(b'=');
                    result.push(bytes[i + 1]);
                    result.push(bytes[i + 2]);
                }
                i += 3;
            }
        } else {
            result.push(bytes[i]);
            i += 1;
        }
    }
    Ok(Value::String(PhpString::from_vec(result)))
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'A'..=b'F' => Some(b - b'A' + 10),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    }
}

fn strcoll_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s1 = args.first().unwrap_or(&Value::Null).to_php_string();
    let s2 = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let cmp = s1.as_bytes().cmp(s2.as_bytes());
    Ok(Value::Long(match cmp {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }))
}

fn money_format_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // money_format() is removed in PHP 8
    Ok(Value::False)
}

fn settype_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.len() < 2 {
        return Ok(Value::False);
    }
    Ok(Value::True)
}

// ===== crypt() and password_* functions =====

const BCRYPT_BASE64: &[u8] = b"./ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

fn bcrypt_base64_decode(encoded: &[u8]) -> Option<[u8; 16]> {
    if encoded.len() < 22 { return None; }
    let mut result = [0u8; 16];
    let mut bits: u32 = 0;
    let mut num_bits: u32 = 0;
    let mut out_idx = 0;
    for &c in &encoded[..22] {
        let val = BCRYPT_BASE64.iter().position(|&b| b == c)? as u32;
        bits = (bits << 6) | val;
        num_bits += 6;
        while num_bits >= 8 && out_idx < 16 {
            num_bits -= 8;
            result[out_idx] = ((bits >> num_bits) & 0xff) as u8;
            out_idx += 1;
        }
    }
    Some(result)
}

fn crypt_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let password = args.first().unwrap_or(&Value::Null).to_php_string();
    let salt_val = args.get(1);
    let password_str = password.to_string_lossy();

    let salt = match salt_val {
        Some(s) => s.to_php_string(),
        None => {
            vm.emit_deprecated("crypt(): Providing a non-standard salt is deprecated");
            return Ok(Value::String(PhpString::from_bytes(b"*0")));
        }
    };
    let salt_str = salt.to_string_lossy();

    if salt_str.starts_with("$2") && salt_str.len() >= 4 {
        let variant = salt_str.as_bytes().get(2).copied().unwrap_or(0);
        if !matches!(variant, b'a' | b'b' | b'x' | b'y') {
            return Ok(Value::String(PhpString::from_bytes(b"*0")));
        }
        if salt_str.as_bytes().get(3) != Some(&b'$') {
            return Ok(Value::String(PhpString::from_bytes(b"*0")));
        }
        if salt_str.len() < 7 || salt_str.as_bytes()[6] != b'$' {
            return Ok(Value::String(PhpString::from_bytes(b"*0")));
        }
        let cost: u32 = match salt_str[4..6].parse() {
            Ok(c) => c,
            Err(_) => return Ok(Value::String(PhpString::from_bytes(b"*0"))),
        };
        if cost < 4 || cost > 31 {
            return Ok(Value::String(PhpString::from_bytes(b"*0")));
        }
        let salt_part = &salt_str[7..];
        if salt_part.len() < 22 { return Ok(Value::String(PhpString::from_bytes(b"*0"))); }
        let salt_22 = &salt_part[..22];
        if salt_22.contains('$') { return Ok(Value::String(PhpString::from_bytes(b"*0"))); }
        let raw_salt = match bcrypt_base64_decode(salt_22.as_bytes()) {
            Some(s) => s,
            None => return Ok(Value::String(PhpString::from_bytes(b"*0"))),
        };
        let version = match variant {
            b'a' => bcrypt::Version::TwoA,
            b'x' => bcrypt::Version::TwoX,
            b'y' => bcrypt::Version::TwoY,
            b'b' => bcrypt::Version::TwoB,
            _ => bcrypt::Version::TwoY,
        };
        match bcrypt::hash_with_salt(&password_str, cost, raw_salt) {
            Ok(parts) => Ok(Value::String(PhpString::from_string(parts.format_for_version(version)))),
            Err(_) => Ok(Value::String(PhpString::from_bytes(b"*0"))),
        }
    } else {
        Ok(Value::String(PhpString::from_bytes(b"*0")))
    }
}

fn password_hash_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let password = args.first().unwrap_or(&Value::Null).to_php_string();
    if password.as_bytes().contains(&0) {
        vm.emit_warning("Bcrypt password must not contain null character");
        return Ok(Value::False);
    }
    let algo = args.get(1).unwrap_or(&Value::Null);
    let algo_str = match algo {
        Value::Null | Value::Long(0) | Value::Long(1) => "2y".to_string(),
        Value::String(s) => s.to_string_lossy(),
        _ => "2y".to_string(),
    };
    if algo_str != "2y" {
        let msg = "password_hash(): Argument #2 ($algo) must be a valid password hashing algorithm";
        let exc = vm.create_exception(b"ValueError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg.to_string(), line: vm.current_line });
    }
    let mut cost: u32 = 10;
    if let Some(Value::Array(opts)) = args.get(2) {
        let opts = opts.borrow();
        if let Some(cost_val) = opts.get_str(b"cost") {
            let c = cost_val.to_long() as u32;
            if c < 4 || c > 31 {
                vm.emit_warning(&format!("Invalid bcrypt cost parameter specified: {}", c));
                return Ok(Value::False);
            }
            cost = c;
        }
    }
    match bcrypt::hash_with_result(&password.to_string_lossy(), cost) {
        Ok(parts) => Ok(Value::String(PhpString::from_string(parts.format_for_version(bcrypt::Version::TwoY)))),
        Err(_) => Ok(Value::False),
    }
}

fn password_verify_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let password = args.first().unwrap_or(&Value::Null).to_php_string();
    let hash = args.get(1).unwrap_or(&Value::Null).to_php_string();
    match bcrypt::verify(&password.to_string_lossy(), &hash.to_string_lossy()) {
        Ok(true) => Ok(Value::True),
        _ => Ok(Value::False),
    }
}

fn password_get_info_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let hash = args.first().unwrap_or(&Value::Null).to_php_string();
    let hash_str = hash.to_string_lossy();
    let mut arr = PhpArray::new();
    if hash_str.starts_with("$2y$") || hash_str.starts_with("$2a$") || hash_str.starts_with("$2b$") {
        arr.set(ArrayKey::String(PhpString::from_bytes(b"algo")), Value::String(PhpString::from_bytes(b"2y")));
        arr.set(ArrayKey::String(PhpString::from_bytes(b"algoName")), Value::String(PhpString::from_bytes(b"bcrypt")));
        let mut options = PhpArray::new();
        if hash_str.len() >= 7 {
            if let Ok(cost) = hash_str[4..6].parse::<i64>() {
                options.set(ArrayKey::String(PhpString::from_bytes(b"cost")), Value::Long(cost));
            }
        }
        arr.set(ArrayKey::String(PhpString::from_bytes(b"options")), Value::Array(Rc::new(RefCell::new(options))));
    } else {
        arr.set(ArrayKey::String(PhpString::from_bytes(b"algo")), Value::Null);
        arr.set(ArrayKey::String(PhpString::from_bytes(b"algoName")), Value::String(PhpString::from_bytes(b"unknown")));
        arr.set(ArrayKey::String(PhpString::from_bytes(b"options")), Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
    }
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

fn password_needs_rehash_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let hash = args.first().unwrap_or(&Value::Null).to_php_string();
    let hash_str = hash.to_string_lossy();
    let mut target_cost: u32 = 10;
    if let Some(Value::Array(opts)) = args.get(2) {
        let opts = opts.borrow();
        if let Some(cost_val) = opts.get_str(b"cost") {
            target_cost = cost_val.to_long() as u32;
        }
    }
    if hash_str.starts_with("$2y$") && hash_str.len() >= 7 {
        if let Ok(current_cost) = hash_str[4..6].parse::<u32>() {
            if current_cost == target_cost { return Ok(Value::False); }
        }
    }
    Ok(Value::True)
}

fn password_algos_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut arr = PhpArray::new();
    arr.push(Value::String(PhpString::from_bytes(b"2y")));
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

