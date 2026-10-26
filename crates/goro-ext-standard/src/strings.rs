use goro_core::array::{ArrayKey, PhpArray};
use goro_core::opcode::ParamType;
use goro_core::string::PhpString;
use goro_core::stubs::{StubArg, StubFunc};
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use std::cell::RefCell;
use std::rc::Rc;

pub fn register(vm: &mut Vm) {
    vm.register_function(b"strlen", strlen);
    vm.register_function(b"strtoupper", strtoupper);
    vm.register_function(b"trim", trim);
    vm.register_function(b"ltrim", ltrim);
    vm.register_function(b"substr", substr);
    vm.register_function(b"strpos", strpos);
    vm.register_function(b"str_contains", str_contains);
    vm.register_function(b"str_starts_with", str_starts_with);
    vm.register_function(b"str_ends_with", str_ends_with);
    vm.register_function(b"str_repeat", str_repeat);
    vm.register_function(b"str_replace", str_replace);
    vm.register_function(b"explode", explode);
    vm.register_function(b"implode", implode);
    vm.register_function(b"join", join_fn); // alias
    vm.register_function(b"chr", chr);
    vm.register_function(b"ord", ord);
    vm.register_function(b"sprintf", sprintf);
    vm.register_function(b"hebrev", hebrev_fn);
    vm.register_function(b"chunk_split", chunk_split);
    vm.register_function(b"str_pad", str_pad);
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
    vm.register_function(b"strrchr", strrchr);
    vm.register_function(b"strstr", strstr_fn);
    vm.register_function(b"strchr", strstr_fn);
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
    vm.register_function(b"str_decrement", str_decrement);
    vm.register_function(b"quotemeta", quotemeta_fn);
    vm.register_function(b"utf8_decode", utf8_decode_fn);
    vm.register_function(b"utf8_encode", utf8_encode_fn);
    vm.register_function(b"strip_tags", strip_tags_fn);
    vm.register_function(b"nl2br", nl2br_fn);
    vm.register_function(b"str_word_count", str_word_count_fn);
    vm.register_function(b"convert_uuencode", convert_uuencode_fn);
    vm.register_function(b"convert_uudecode", convert_uudecode_fn);
    vm.register_function(b"quoted_printable_encode", quoted_printable_encode_fn);
    vm.register_function(b"quoted_printable_decode", quoted_printable_decode_fn);

    // Register parameter types for strict_types enforcement on builtins
    let s = || Some(ParamType::Simple(b"string".to_vec()));
    let i = || Some(ParamType::Simple(b"int".to_vec()));
    let ns = || {
        Some(ParamType::Nullable(Box::new(ParamType::Simple(
            b"string".to_vec(),
        ))))
    };
    let ni = || {
        Some(ParamType::Nullable(Box::new(ParamType::Simple(
            b"int".to_vec(),
        ))))
    };
    vm.register_builtin_param_types(b"strlen", vec![s()]);
    vm.register_builtin_param_types(b"ord", vec![s()]);
    vm.register_builtin_param_types(b"chr", vec![i()]);
    vm.register_builtin_param_types(b"strtolower", vec![s()]);
    vm.register_builtin_param_types(b"strtoupper", vec![s()]);
    vm.register_builtin_param_types(b"trim", vec![s(), s()]);
    vm.register_builtin_param_types(b"ltrim", vec![s(), s()]);
    vm.register_builtin_param_types(b"rtrim", vec![s(), s()]);
    vm.register_builtin_param_types(b"chop", vec![s(), s()]);
    vm.register_builtin_param_types(b"substr", vec![s(), i(), ni()]);
    vm.register_builtin_param_types(b"strpos", vec![s(), s(), i()]);
    vm.register_builtin_param_types(b"str_contains", vec![s(), s()]);
    vm.register_builtin_param_types(b"str_starts_with", vec![s(), s()]);
    vm.register_builtin_param_types(b"str_ends_with", vec![s(), s()]);
    vm.register_builtin_param_types(b"str_repeat", vec![s(), i()]);
    vm.register_builtin_param_types(b"explode", vec![s(), s(), i()]);
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
    vm.register_builtin_param_types(b"str_word_count", vec![s(), i(), ns()]);
    vm.register_builtin_param_types(b"strcmp", vec![s(), s()]);
    vm.register_builtin_param_types(b"strncmp", vec![s(), s(), i()]);
    vm.register_builtin_param_types(b"strcasecmp", vec![s(), s()]);
    vm.register_builtin_param_types(b"strncasecmp", vec![s(), s(), i()]);
    vm.register_builtin_param_types(b"substr_count", vec![s(), s(), i(), ni()]);
    vm.register_builtin_param_types(b"substr_compare", vec![s(), s(), i(), ni(), None]);
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
    vm.register_builtin_param_types(b"get_html_translation_table", vec![i(), i(), s()]);
    vm.register_builtin_param_types(b"htmlspecialchars", vec![s(), i(), ns(), None]);
    vm.register_builtin_param_types(b"htmlspecialchars_decode", vec![s(), i()]);
    vm.register_builtin_param_types(b"htmlentities", vec![s(), i(), ns(), None]);
    vm.register_builtin_param_types(b"html_entity_decode", vec![s(), i(), s()]);
}

fn strlen(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = vm.value_to_string(args.first().unwrap_or(&Value::Null));
    Ok(Value::Long(s.len() as i64))
}

fn strtolower(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = vm.value_to_string(args.first().unwrap_or(&Value::Null));
    let lower: Vec<u8> = s
        .as_bytes()
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect();
    Ok(Value::String(PhpString::from_vec(lower)))
}

fn strtoupper(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = vm.value_to_string(args.first().unwrap_or(&Value::Null));
    let upper: Vec<u8> = s
        .as_bytes()
        .iter()
        .map(|b| b.to_ascii_uppercase())
        .collect();
    Ok(Value::String(PhpString::from_vec(upper)))
}

/// Check if a byte string is a PHP-numeric string (matches php_is_numeric_string).
/// PHP allows leading and trailing whitespace.
fn is_numeric_string(s: &[u8]) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut i = 0;
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
        i += 1;
    }
    if i >= s.len() {
        return false;
    }
    if s[i] == b'+' || s[i] == b'-' {
        i += 1;
    }
    if i >= s.len() {
        return false;
    }
    let mut has_digits = false;
    while i < s.len() && s[i].is_ascii_digit() {
        has_digits = true;
        i += 1;
    }
    if i < s.len() && s[i] == b'.' {
        i += 1;
        while i < s.len() && s[i].is_ascii_digit() {
            has_digits = true;
            i += 1;
        }
    }
    if !has_digits {
        return false;
    }
    if i < s.len() && (s[i] == b'e' || s[i] == b'E') {
        i += 1;
        if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
            i += 1;
        }
        if i >= s.len() || !s[i].is_ascii_digit() {
            return false;
        }
        while i < s.len() && s[i].is_ascii_digit() {
            i += 1;
        }
    }
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
        i += 1;
    }
    i == s.len()
}

/// php_charmask(): the bytes a trim() character list names ("a..z"
/// ranges), with its warnings for malformed ranges.
fn expand_charlist_warn(input: &[u8], vm: &mut Vm, func_name: &str) -> Vec<u8> {
    let mut mask = [false; 256];
    let end = input.len();
    let mut i = 0;
    while i < end {
        let c = input[i];
        if i + 3 < end && input[i + 1] == b'.' && input[i + 2] == b'.' && input[i + 3] >= c {
            for m in &mut mask[c as usize..=input[i + 3] as usize] {
                *m = true;
            }
            i += 4;
            continue;
        } else if i + 1 < end && input[i] == b'.' && input[i + 1] == b'.' {
            let msg = if i == 0 {
                "Invalid '..'-range, no character to the left of '..'"
            } else if i + 2 >= end {
                "Invalid '..'-range, no character to the right of '..'"
            } else if input[i - 1] > input[i + 2] {
                "Invalid '..'-range, '..'-range needs to be incrementing"
            } else {
                "Invalid '..'-range"
            };
            vm.emit_warning(&format!("{}(): {}", func_name, msg));
        } else {
            mask[c as usize] = true;
        }
        i += 1;
    }
    (0..=255u8).filter(|&b| mask[b as usize]).collect()
}

fn trim(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let first = args.first().unwrap_or(&Value::Null);
    let s = vm.value_to_string(first);
    let bytes = s.as_bytes().to_vec();
    let chars = args
        .get(1)
        .map(|v| {
            let s = vm.value_to_string(v);
            expand_charlist_warn(s.as_bytes(), vm, "trim")
        })
        .unwrap_or_else(|| b" \t\n\r\0\x0B".to_vec());
    let bytes = bytes.as_slice();

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
    let first = args.first().unwrap_or(&Value::Null);
    let s = vm.value_to_string(first);
    let bytes = s.as_bytes().to_vec();
    let chars = args
        .get(1)
        .map(|v| {
            let s = vm.value_to_string(v);
            expand_charlist_warn(s.as_bytes(), vm, "ltrim")
        })
        .unwrap_or_else(|| b" \t\n\r\0\x0B".to_vec());
    let bytes = bytes.as_slice();
    let start = bytes
        .iter()
        .position(|b| !chars.contains(b))
        .unwrap_or(bytes.len());
    Ok(Value::String(PhpString::from_vec(bytes[start..].to_vec())))
}

fn rtrim(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let first = args.first().unwrap_or(&Value::Null);
    let s = vm.value_to_string(first);
    let bytes = s.as_bytes().to_vec();
    let chars = args
        .get(1)
        .map(|v| {
            let s = vm.value_to_string(v);
            expand_charlist_warn(s.as_bytes(), vm, "rtrim")
        })
        .unwrap_or_else(|| b" \t\n\r\0\x0B".to_vec());
    let bytes = bytes.as_slice();
    let end = bytes
        .iter()
        .rposition(|b| !chars.contains(b))
        .map(|i| i + 1)
        .unwrap_or(0);
    Ok(Value::String(PhpString::from_vec(bytes[..end].to_vec())))
}

fn substr(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = vm.value_to_string(args.first().unwrap_or(&Value::Null));
    let bytes = s.as_bytes();
    let len = bytes.len() as i64;

    let mut start = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    if start < 0 {
        start = (len + start).max(0);
    }
    let start = start.min(len) as usize;

    let length = args.get(2).and_then(|v| {
        if matches!(v, Value::Null) {
            None
        } else {
            Some(v.to_long())
        }
    });
    let end = match length {
        Some(l) if l < 0 => {
            let end_pos = len + l;
            if end_pos <= start as i64 {
                start
            } else {
                end_pos as usize
            }
        }
        Some(l) if l >= 0 => {
            let end_pos = (start as i64).saturating_add(l);
            if end_pos > bytes.len() as i64 {
                bytes.len()
            } else {
                end_pos as usize
            }
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
    if let Some(offset_v) = args.get(2) {
        let derefed = offset_v.deref();
        match &derefed {
            Value::String(s) => {
                // Non-numeric string offset throws TypeError
                if !is_numeric_string(s.as_bytes()) {
                    let msg = "strpos(): Argument #3 ($offset) must be of type int, string given"
                        .to_string();
                    let exc = vm.create_exception(b"TypeError", &msg, 0);
                    vm.current_exception = Some(exc);
                    return Err(VmError {
                        message: msg,
                        line: vm.current_line,
                    });
                }
            }
            Value::Double(f) => {
                // Float offset: PHP truncates with deprecation warning (or error in some cases)
                // The test expects this to succeed.
                let _ = f;
            }
            _ => {}
        }
    }
    let offset_val = args.get(2).map(|v| v.deref().to_long()).unwrap_or(0);

    let h = haystack.as_bytes();
    let n = needle.as_bytes();

    // Handle negative offset
    let offset = if offset_val < 0 {
        let abs = (-offset_val) as usize;
        if abs > h.len() {
            let msg =
                "strpos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)";
            let exc = vm.create_exception(b"ValueError", msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg.into(),
                line: vm.current_line,
            });
        }
        h.len() - abs
    } else {
        let o = offset_val as usize;
        if o > h.len() {
            let msg =
                "strpos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)";
            let exc = vm.create_exception(b"ValueError", msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg.into(),
                line: vm.current_line,
            });
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

fn str_contains(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let haystack = vm.value_to_string(args.first().unwrap_or(&Value::Null));
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

fn str_starts_with(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let haystack = vm.value_to_string(args.first().unwrap_or(&Value::Null));
    let prefix = args.get(1).unwrap_or(&Value::Null).to_php_string();
    Ok(if haystack.as_bytes().starts_with(prefix.as_bytes()) {
        Value::True
    } else {
        Value::False
    })
}

fn str_ends_with(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let haystack = vm.value_to_string(args.first().unwrap_or(&Value::Null));
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
        let msg =
            "str_repeat(): Argument #2 ($times) must be greater than or equal to 0".to_string();
        let exc = vm.throw_type_error(msg.clone());
        if let Value::Object(obj) = &exc {
            obj.borrow_mut().class_name = b"ValueError".to_vec();
        }
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    // zend_string_safe_alloc(len, times, 0): an overflowing size is fatal.
    if s.len()
        .checked_mul(times as usize)
        .and_then(|n| n.checked_add(32))
        .is_none()
    {
        let msg = format!(
            "Possible integer overflow in memory allocation ({} * {} + 32)",
            s.len(),
            times
        );
        let line = vm.current_line;
        return Err(vm.emit_fatal_at(&msg, line));
    }
    let total_len = s.len().saturating_mul(times as usize);
    // zend_string_safe_alloc(): bail out like the Zend allocator would.
    let mem_limit = goro_core::value::memory_get_limit();
    if mem_limit > 0 {
        // (safe_emalloc(len, times, 32): a huge block's size is reported
        // as requested)
        let want = if total_len >= 2 * 1024 * 1024 {
            total_len.saturating_add(32)
        } else {
            total_len.saturating_add(25) & !7
        };
        vm.memory_check(want)?;
    } else if total_len > (1usize << 40) {
        let msg = format!(
            "Allowed memory size of {} bytes exhausted (tried to allocate {} bytes)",
            mem_limit,
            total_len.saturating_add(25) & !7,
        );
        let line = vm.current_line;
        return Err(vm.emit_fatal_at(&msg, line));
    }
    let mut repeated: Vec<u8> = Vec::new();
    if repeated.try_reserve_exact(total_len).is_err() {
        let msg = format!(
            "Out of memory (allocated {}) (tried to allocate {} bytes)",
            goro_core::value::memory_get_usage(),
            total_len
        );
        let line = vm.current_line;
        return Err(vm.emit_fatal_at(&msg, line));
    }
    if times > 0 && !s.is_empty() {
        // Fill by doubling what is already there.
        repeated.extend_from_slice(s.as_bytes());
        while repeated.len() < total_len {
            let n = repeated.len().min(total_len - repeated.len());
            repeated.extend_from_within(..n);
        }
    }
    Ok(Value::String(PhpString::from_vec(repeated)))
}

/// One php_str_to_str_ex / php_str_to_str_i_ex / php_char_to_str_ex pass.
fn str_replace_once(
    subject: &[u8],
    needle: &[u8],
    repl: &[u8],
    ci: bool,
    count: &mut i64,
) -> Vec<u8> {
    let n = needle.len();
    if n == 0 || subject.len() < n {
        return subject.to_vec();
    }
    let eq = |a: &[u8]| {
        if ci {
            a.eq_ignore_ascii_case(needle)
        } else {
            a == needle
        }
    };
    let mut out = Vec::with_capacity(subject.len());
    let mut i = 0;
    while i < subject.len() {
        if i + n <= subject.len() && eq(&subject[i..i + n]) {
            out.extend_from_slice(repl);
            *count += 1;
            i += n;
        } else {
            out.push(subject[i]);
            i += 1;
        }
    }
    out
}

enum StrOrArr {
    Str(Vec<u8>),
    Arr(Vec<Value>, Vec<ArrayKey>),
}

/// php_str_replace_common() for str_replace / str_ireplace.
fn php_str_replace_common(
    vm: &mut Vm,
    args: &[Value],
    fname: &str,
    ci: bool,
) -> Result<Value, VmError> {
    if args.len() < 3 {
        let msg = format!(
            "{}() expects at least 3 arguments, {} given",
            fname,
            args.len()
        );
        let exc = vm.create_exception(b"ArgumentCountError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    // Z_PARAM_ARRAY_HT_OR_STR
    let parse = |vm: &mut Vm, idx: usize, name: &str| -> Result<StrOrArr, VmError> {
        let v = args[idx].deref();
        match &v {
            Value::Array(a) => {
                let b = a.borrow();
                Ok(StrOrArr::Arr(
                    b.values().cloned().collect(),
                    b.keys().cloned().collect(),
                ))
            }
            Value::String(s) => Ok(StrOrArr::Str(s.as_bytes().to_vec())),
            Value::Null | Value::Undef => Ok(StrOrArr::Str(Vec::new())),
            Value::Long(_) | Value::Double(_) | Value::True | Value::False => {
                Ok(StrOrArr::Str(v.to_php_string().as_bytes().to_vec()))
            }
            Value::Object(o) => {
                let has = vm
                    .get_class_def(&o.borrow().class_name.to_ascii_lowercase())
                    .map(|c| c.get_method(b"__tostring").is_some())
                    .unwrap_or(false);
                if has {
                    Ok(StrOrArr::Str(vm.value_to_string(&v).as_bytes().to_vec()))
                } else {
                    let msg = format!(
                        "{}(): Argument #{} (${}) must be of type array|string, {} given",
                        fname,
                        idx + 1,
                        name,
                        goro_core::value::display_class_name(&o.borrow().class_name)
                    );
                    let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    Err(VmError {
                        message: msg,
                        line: vm.current_line,
                    })
                }
            }
            other => {
                let msg = format!(
                    "{}(): Argument #{} (${}) must be of type array|string, {} given",
                    fname,
                    idx + 1,
                    name,
                    Vm::value_type_name(other)
                );
                let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
                vm.current_exception = Some(exc);
                Err(VmError {
                    message: msg,
                    line: vm.current_line,
                })
            }
        }
    };
    let search = parse(vm, 0, "search")?;
    let replace = parse(vm, 1, "replace")?;
    let subject = parse(vm, 2, "subject")?;
    if let (StrOrArr::Str(_), StrOrArr::Arr(..)) = (&search, &replace) {
        let msg = format!(
            "{}(): Argument #2 ($replace) must be of type string when argument #1 ($search) is a string",
            fname
        );
        let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    // Pre-convert search/replace array entries lazily per subject, like PHP.
    let mut count: i64 = 0;
    let in_subject = |vm: &mut Vm, subj: Vec<u8>, count: &mut i64| -> Vec<u8> {
        if subj.is_empty() {
            return subj;
        }
        match &search {
            StrOrArr::Str(needle) => {
                let repl = match &replace {
                    StrOrArr::Str(r) => r.clone(),
                    _ => Vec::new(),
                };
                str_replace_once(&subj, needle, &repl, ci, count)
            }
            StrOrArr::Arr(needles, _) => {
                let mut cur = subj;
                let mut ridx = 0usize;
                for nv in needles {
                    let needle = crate::misc::sort_cmp_string(vm, nv);
                    let repl: Vec<u8> = match &replace {
                        StrOrArr::Str(r) => r.clone(),
                        StrOrArr::Arr(rs, _) => {
                            let r = match rs.get(ridx) {
                                Some(rv) => crate::misc::sort_cmp_string(vm, rv),
                                None => Vec::new(),
                            };
                            ridx += 1;
                            r
                        }
                    };
                    if needle.is_empty() {
                        continue;
                    }
                    cur = str_replace_once(&cur, &needle, &repl, ci, count);
                    if cur.is_empty() {
                        break;
                    }
                }
                cur
            }
        }
    };
    let result = match subject {
        StrOrArr::Str(subj) => Value::String(PhpString::from_vec(in_subject(vm, subj, &mut count))),
        StrOrArr::Arr(vals, keys) => {
            let mut out = PhpArray::new();
            for (k, v) in keys.into_iter().zip(vals) {
                let sb = crate::misc::sort_cmp_string(vm, &v);
                let r = in_subject(vm, sb, &mut count);
                out.set(k, Value::String(PhpString::from_vec(r)));
            }
            Value::Array(Rc::new(RefCell::new(out)))
        }
    };
    if vm.current_exception.is_some() {
        return Ok(Value::Null);
    }
    if let Some(Value::Reference(r)) = args.get(3) {
        *r.borrow_mut() = Value::Long(count);
    }
    Ok(result)
}

fn str_replace(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    php_str_replace_common(vm, args, "str_replace", false)
}

fn explode(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    use goro_core::array::PhpArray;
    use std::cell::RefCell;
    use std::rc::Rc;

    let delimiter = _vm.value_to_string(args.first().unwrap_or(&Value::Null));
    let string = _vm.value_to_string(args.get(1).unwrap_or(&Value::Null));
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
    implode_named(vm, args, "implode")
}

fn join_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    implode_named(vm, args, "join")
}

fn implode_type_error(vm: &mut Vm, msg: String) -> VmError {
    let exc = vm.create_exception(b"TypeError", &msg, 0);
    vm.current_exception = Some(exc);
    VmError {
        message: msg,
        line: vm.current_line,
    }
}

fn implode_named(vm: &mut Vm, args: &[Value], func_name: &str) -> Result<Value, VmError> {
    let null_pieces = || {
        format!(
            "{}(): If argument #1 ($separator) is of type string, argument #2 ($array) must be of type array, null given",
            func_name
        )
    };
    let (glue, arr) = if func_name == "implode" {
        // A direct implode() call is frameless in php-src (its arginfo is
        // only arity-checked here): ZEND_FRAMELESS_FUNCTION(implode, 1/2)
        // parse the separator as a plain string, then the array.
        match args {
            [] => {
                let msg = "implode() expects at least 1 argument, 0 given".to_string();
                let exc = vm.create_exception(b"ArgumentCountError", &msg, 0);
                vm.current_exception = Some(exc);
                return Err(VmError {
                    message: msg,
                    line: vm.current_line,
                });
            }
            [one] => match one.deref() {
                Value::Array(arr) => (PhpString::empty(), arr),
                _ => return Err(implode_type_error(vm, null_pieces())),
            },
            [sep, arr, ..] => {
                let sep = sep.deref();
                let strict = vm.caller_strict_types;
                let line = vm.current_line;
                let glue = if matches!(sep, Value::Null | Value::Undef) && !strict {
                    vm.emit_deprecated_at(
                        "implode(): Passing null to parameter #1 ($separator) of type array|string is deprecated",
                        line,
                    );
                    if vm.current_exception.is_some() {
                        return Err(VmError {
                            message: String::new(),
                            line,
                        });
                    }
                    PhpString::empty()
                } else {
                    match vm.zpp_coerce_scalar(goro_core::arginfo::T_STRING, &sep, strict, line) {
                        Ok(Some(Value::String(s))) => s,
                        Ok(Some(v)) => vm.value_to_string(&v),
                        Ok(None) => {
                            let msg = format!(
                                "implode(): Argument #1 ($separator) must be of type string, {} given",
                                Vm::value_type_name(&sep)
                            );
                            return Err(implode_type_error(vm, msg));
                        }
                        Err(()) => {
                            return Err(VmError {
                                message: String::new(),
                                line,
                            });
                        }
                    }
                };
                match arr.deref() {
                    Value::Array(a) => (glue, a),
                    Value::Null | Value::Undef => {
                        return Err(implode_type_error(vm, null_pieces()));
                    }
                    other => {
                        let msg = format!(
                            "implode(): Argument #2 ($array) must be of type ?array, {} given",
                            Vm::value_type_name(&other)
                        );
                        return Err(implode_type_error(vm, msg));
                    }
                }
            }
        }
    } else {
        // PHP_FUNCTION(implode) after zpp (array|string $separator,
        // ?array $array = null, checked from the arginfo).
        let a0 = args.first().map(|v| v.deref()).unwrap_or(Value::Null);
        let a1 = args.get(1).map(|v| v.deref()).unwrap_or(Value::Null);
        match (a0, a1) {
            (Value::Array(arr), Value::Null | Value::Undef) => (PhpString::empty(), arr),
            (_, Value::Null | Value::Undef) => {
                return Err(implode_type_error(vm, null_pieces()));
            }
            (Value::Array(_), _) => {
                let msg = format!(
                    "{}(): Argument #1 ($separator) must be of type string, array given",
                    func_name
                );
                return Err(implode_type_error(vm, msg));
            }
            (sep, Value::Array(arr)) => (vm.value_to_string(&sep), arr),
            (_, other) => {
                let msg = format!(
                    "{}(): Argument #2 ($array) must be of type ?array, {} given",
                    func_name,
                    Vm::value_type_name(&other)
                );
                return Err(implode_type_error(vm, msg));
            }
        }
    };

    // php_implode()
    let values: Vec<Value> = arr.borrow().values().cloned().collect();
    let mut result = Vec::new();
    for (i, v) in values.iter().enumerate() {
        if i > 0 {
            result.extend_from_slice(glue.as_bytes());
        }
        let v = v.deref();
        if matches!(v, Value::Array(_)) {
            vm.emit_warning_at("Array to string conversion", vm.current_line);
        }
        let s = vm.value_to_string_checked(&v)?;
        // If __toString raised an exception, propagate it.
        if vm.current_exception.is_some() {
            return Err(VmError {
                message: String::new(),
                line: vm.current_line,
            });
        }
        result.extend_from_slice(s.as_bytes());
    }
    Ok(Value::String(PhpString::from_vec(result)))
}

fn chr(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        let msg = "chr() expects exactly 1 argument, 0 given";
        let exc = vm.create_exception(b"TypeError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg.into(),
            line: vm.current_line,
        });
    }
    if args.len() > 1 {
        let msg = format!("chr() expects exactly 1 argument, {} given", args.len());
        let exc = vm.create_exception(b"TypeError", &msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    let n = args[0].to_long();
    if !(0..=255).contains(&n) {
        vm.emit_deprecated_at(
            "chr(): Providing a value not in-between 0 and 255 is deprecated, this is because a byte value must be in the [0, 255] interval. The value used will be constrained using % 256",
            vm.current_line,
        );
    }
    let code = ((n % 256 + 256) % 256) as u8;
    Ok(Value::String(PhpString::one_char(code)))
}

fn ord(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = s.as_bytes();
    // PHP 8.5: deprecation warnings for non-single-byte input.
    if bytes.is_empty() {
        let line = vm.current_line;
        vm.emit_deprecated_at("ord(): Providing an empty string is deprecated", line);
    } else if bytes.len() > 1 {
        let line = vm.current_line;
        vm.emit_deprecated_at(
            "ord(): Providing a string that is not one byte long is deprecated. Use ord($str[0]) instead",
            line,
        );
    }
    let code = bytes.first().copied().unwrap_or(0);
    Ok(Value::Long(code as i64))
}

/// The "Passing null to parameter #N ($format)" deprecation of the
/// formatted-print functions the generic zpp layer does not handle.
pub(crate) fn deprecate_null_format(vm: &mut Vm, func_name: &str, arg_num: u32, v: Option<&Value>) {
    if matches!(v.map(|v| v.deref()), Some(Value::Null)) {
        let line = vm.current_line;
        vm.emit_deprecated_at(
            &format!(
                "{}(): Passing null to parameter #{} ($format) of type string is deprecated",
                func_name, arg_num
            ),
            line,
        );
    }
}

/// Validate that the format argument to sprintf/printf is a string (not array/resource/non-Stringable object)
fn validate_sprintf_format_arg(
    vm: &mut Vm,
    func_name: &str,
    format_arg: Option<&Value>,
) -> Result<(), VmError> {
    if let Some(arg) = format_arg {
        let derefed = arg.deref();
        match &derefed {
            Value::Array(_) => {
                let msg = format!(
                    "{}(): Argument #1 ($format) must be of type string, array given",
                    func_name
                );
                let exc = vm.create_exception(b"TypeError", &msg, 0);
                vm.current_exception = Some(exc);
                return Err(VmError {
                    message: msg,
                    line: vm.current_line,
                });
            }
            Value::Resource(_) => {
                let msg = format!(
                    "{}(): Argument #1 ($format) must be of type string, resource given",
                    func_name
                );
                let exc = vm.create_exception(b"TypeError", &msg, 0);
                vm.current_exception = Some(exc);
                return Err(VmError {
                    message: msg,
                    line: vm.current_line,
                });
            }
            Value::Object(obj) => {
                // Check if the object implements __toString (Stringable)
                let class_name = obj.borrow().class_name.clone();
                let class_lower: Vec<u8> =
                    class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                let has_tostring = vm
                    .classes
                    .get(&class_lower)
                    .map(|c| c.get_method(b"__tostring").is_some())
                    .unwrap_or(false);
                if !has_tostring {
                    let class_display = goro_core::string::msg_str(&class_name).to_string();
                    let msg = format!(
                        "{}(): Argument #1 ($format) must be of type string, {} given",
                        func_name, class_display
                    );
                    let exc = vm.create_exception(b"TypeError", &msg, 0);
                    vm.current_exception = Some(exc);
                    return Err(VmError {
                        message: msg,
                        line: vm.current_line,
                    });
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Validate that the second argument of vsprintf/vprintf/vfprintf is an array.
fn validate_sprintf_values_arg(
    vm: &mut Vm,
    func_name: &str,
    values_arg: &Value,
) -> Result<(), VmError> {
    let derefed = values_arg.deref();
    let type_name = match &derefed {
        Value::Array(_) => return Ok(()),
        Value::Long(_) => "int",
        Value::Double(_) => "float",
        Value::Null | Value::Undef => "null",
        Value::True => "true",
        Value::False => "false",
        Value::String(_) => "string",
        Value::Resource(_) => "resource",
        Value::Object(obj) => {
            let name = String::from_utf8_lossy(&obj.borrow().class_name).to_string();
            let msg = format!(
                "{}(): Argument #2 ($values) must be of type array, {} given",
                func_name, name
            );
            let exc = vm.create_exception(b"TypeError", &msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
        _ => "unknown",
    };
    let msg = format!(
        "{}(): Argument #2 ($values) must be of type array, {} given",
        func_name, type_name
    );
    let exc = vm.create_exception(b"TypeError", &msg, 0);
    vm.current_exception = Some(exc);
    Err(VmError {
        message: msg,
        line: vm.current_line,
    })
}

fn sprintf(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        let msg = "sprintf() expects at least 1 argument, 0 given";
        let exc = vm.create_exception(b"ArgumentCountError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg.into(),
            line: vm.current_line,
        });
    }
    // TypeError for non-string format argument (array, resource)
    validate_sprintf_format_arg(vm, "sprintf", args.first())?;
    // Check argument count
    let result = do_sprintf_vm(vm, args)?;
    Ok(Value::String(PhpString::from_vec(result)))
}

/// Shared sprintf implementation used by both sprintf() and printf().
/// No-VM variant: error paths return the empty string.
pub fn do_sprintf(args: &[Value]) -> Vec<u8> {
    do_sprintf_inner(None, args, &mut None).unwrap_or_default()
}

/// Sprintf implementation with VM reference (for __toString invocation and
/// for raising errors). Errors set the VM's current_exception.
pub fn do_sprintf_vm(vm: &mut Vm, args: &[Value]) -> Result<Vec<u8>, VmError> {
    let mut pending = None;
    let out = do_sprintf_inner(Some(vm), args, &mut pending)?;
    pending.map_or(Ok(out), Err)
}

thread_local! {
    /// The formatted-print function being run, for its diagnostics.
    static SPRINTF_FNAME: std::cell::Cell<&'static str> = const { std::cell::Cell::new("sprintf") };
}

/// do_sprintf_vm() for `fname` (printf, vsprintf, fprintf, ...).
pub fn do_sprintf_vm_named(
    vm: &mut Vm,
    fname: &'static str,
    args: &[Value],
) -> Result<Vec<u8>, VmError> {
    let (out, pending) = do_sprintf_vm_named_partial(vm, fname, args)?;
    pending.map_or(Ok(out), Err)
}

/// do_sprintf_vm_named() for the printing functions: a `%s` argument that
/// cannot be converted throws, but php_formatted_print() goes on with an
/// empty string, and the result is still written before the exception
/// propagates. Returns the output and that pending exception.
pub fn do_sprintf_vm_named_partial(
    vm: &mut Vm,
    fname: &'static str,
    args: &[Value],
) -> Result<(Vec<u8>, Option<VmError>), VmError> {
    let prev = SPRINTF_FNAME.with(|f| f.replace(fname));
    let mut pending = None;
    let r = do_sprintf_inner(Some(vm), args, &mut pending);
    SPRINTF_FNAME.with(|f| f.set(prev));
    Ok((r?, pending))
}

/// The `nb_additional_parameters` of php_formatted_print() for the running
/// function: -1 for the v*printf() family (ValueError on missing items),
/// otherwise the number of parameters before the variadic values.
fn sprintf_additional_params(fname: &str) -> i64 {
    match fname {
        "vsprintf" | "vprintf" | "vfprintf" => -1,
        "fprintf" => 2,
        _ => 1,
    }
}

/// php_conv_fp(): fixed ('F') or exponential ('e'/'E') conversion of a
/// finite number. Returns (digits without sign, is_negative).
fn php_conv_fp(format: u8, num: f64, precision: usize, dec_point: u8) -> (Vec<u8>, bool) {
    // __cvt() reports zero (and -0.0) as positive.
    let is_negative = num != 0.0 && num.is_sign_negative();
    let a = num.abs();
    let mut s = if format == b'F' {
        format!("{:.*}", precision, a).into_bytes()
    } else {
        let t = format!("{:.*e}", precision, a);
        let (mant, exp) = t.split_once('e').unwrap_or((&t, "0"));
        let exp: i32 = exp.parse().unwrap_or(0);
        let mut v = mant.as_bytes().to_vec();
        v.push(format);
        v.push(if exp < 0 { b'-' } else { b'+' });
        v.extend_from_slice(exp.unsigned_abs().to_string().as_bytes());
        v
    };
    if format == b'F' && dec_point != b'.' {
        if let Some(p) = s.iter().position(|&c| c == b'.') {
            s[p] = dec_point;
        }
    }
    (s, is_negative)
}

/// php_sprintf_appendstring().
#[allow(clippy::too_many_arguments)]
fn sprintf_append_string(
    out: &mut Vec<u8>,
    add: &[u8],
    min_width: usize,
    max_width: usize,
    padding: u8,
    align_left: bool,
    neg: bool,
    expprec: bool,
    always_sign: bool,
) {
    let mut add = add;
    let mut copy_len = if expprec {
        max_width.min(add.len())
    } else {
        add.len()
    };
    let npad = min_width.saturating_sub(copy_len);
    out.reserve(min_width.max(copy_len));
    if !align_left {
        if (neg || always_sign) && padding == b'0' && !add.is_empty() {
            out.push(if neg { b'-' } else { b'+' });
            add = &add[1..];
            copy_len = copy_len.saturating_sub(1);
        }
        out.extend(std::iter::repeat_n(padding, npad));
    }
    out.extend_from_slice(&add[..copy_len.min(add.len())]);
    if align_left {
        out.extend(std::iter::repeat_n(padding, npad));
    }
}

fn do_sprintf_inner(
    mut vm: Option<&mut Vm>,
    args: &[Value],
    pending: &mut Option<VmError>,
) -> Result<Vec<u8>, VmError> {
    if args.is_empty() {
        return Ok(Vec::new());
    }
    let format = if let Some(vm_ref) = vm.as_deref_mut() {
        vm_ref.value_to_string(&args[0])
    } else {
        args[0].to_php_string()
    };
    let fmt = format.as_bytes();
    let values = &args[1..];
    let argc = values.len();
    let fname = SPRINTF_FNAME.with(|f| f.get());

    // Helper: raise an exception of `class` (or no-op if no VM).
    fn raise_class(vm: &mut Option<&mut Vm>, class: &[u8], msg: String) -> VmError {
        let msg = cut_at_nul(msg);
        if let Some(vm_ref) = vm.as_deref_mut() {
            let exc = vm_ref.create_exception(class, &msg, 0);
            vm_ref.current_exception = Some(exc);
            return VmError {
                message: msg,
                line: vm_ref.current_line,
            };
        }
        VmError {
            message: msg,
            line: 0,
        }
    }
    fn raise(vm: &mut Option<&mut Vm>, msg: String) -> VmError {
        raise_class(vm, b"ValueError", msg)
    }
    // The format is NUL-terminated in C: reading past the end sees '\0'.
    let at = |i: usize| fmt.get(i).copied().unwrap_or(0);
    // php_sprintf_getnumber(): ZEND_STRTOL, -1 when >= INT_MAX.
    fn getnumber(fmt: &[u8], i: &mut usize) -> i64 {
        let mut num: u64 = 0;
        while *i < fmt.len() && fmt[*i].is_ascii_digit() {
            num = num
                .saturating_mul(10)
                .saturating_add((fmt[*i] - b'0') as u64);
            *i += 1;
        }
        if num >= i32::MAX as u64 {
            -1
        } else {
            num as i64
        }
    }
    // php_sprintf_get_argnum(): Ok(None) for "next", Ok(Some(0-based)).
    fn get_argnum(
        fmt: &[u8],
        i: &mut usize,
        vm: &mut Option<&mut Vm>,
    ) -> Result<Option<usize>, VmError> {
        let mut t = *i;
        while t < fmt.len() && fmt[t].is_ascii_digit() {
            t += 1;
        }
        if fmt.get(t) != Some(&b'$') {
            return Ok(None);
        }
        let n = getnumber(fmt, i);
        if n <= 0 {
            return Err(raise(
                vm,
                format!(
                    "Argument number specifier must be greater than zero and less than {}",
                    i32::MAX
                ),
            ));
        }
        *i += 1; // skip '$'
        Ok(Some(n as usize - 1))
    }

    let mut result: Vec<u8> = Vec::new();
    let mut currarg: usize = 0;
    let mut max_missing: Option<usize> = None;
    let mut i = 0usize;

    'outer: while i < fmt.len() {
        let Some(off) = fmt[i..].iter().position(|&c| c == b'%') else {
            result.extend_from_slice(&fmt[i..]);
            break;
        };
        result.extend_from_slice(&fmt[i..i + off]);
        i += off + 1; // skip the '%'

        if at(i) == b'%' {
            result.push(b'%');
            i += 1;
            continue;
        }
        let mut align_left = false;
        let mut padding = b' ';
        let mut always_sign = false;
        let mut expprec = false;
        let mut adj_precision = false;
        let mut width: usize = 0;
        let mut precision: i64 = 0;
        let argnum: Option<usize>;

        if at(i).is_ascii_alphabetic() {
            argnum = None;
        } else {
            argnum = get_argnum(fmt, &mut i, &mut vm)?;
            loop {
                match at(i) {
                    b' ' | b'0' => padding = at(i),
                    b'-' => align_left = true,
                    b'+' => always_sign = true,
                    b'\'' => {
                        if i + 1 < fmt.len() {
                            i += 1;
                            padding = fmt[i];
                        } else {
                            return Err(raise(&mut vm, "Missing padding character".to_string()));
                        }
                    }
                    _ => break,
                }
                i += 1;
            }
            if at(i) == b'*' {
                i += 1;
                let w = match get_argnum(fmt, &mut i, &mut vm)? {
                    Some(n) => n,
                    None => {
                        currarg += 1;
                        currarg - 1
                    }
                };
                if w >= argc {
                    max_missing = Some(max_missing.map_or(w, |m| m.max(w)));
                    continue 'outer;
                }
                let Value::Long(wv) = values[w].deref() else {
                    return Err(raise(&mut vm, "Width must be an integer".to_string()));
                };
                if wv < 0 || wv > i32::MAX as i64 {
                    return Err(raise(
                        &mut vm,
                        format!("Width must be between 0 and {}", i32::MAX),
                    ));
                }
                width = wv as usize;
            } else if at(i).is_ascii_digit() {
                let w = getnumber(fmt, &mut i);
                if w < 0 {
                    return Err(raise(
                        &mut vm,
                        format!("Width must be between 0 and {}", i32::MAX),
                    ));
                }
                width = w as usize;
            }
            if at(i) == b'.' {
                i += 1;
                adj_precision = true;
                if at(i) == b'*' {
                    i += 1;
                    let p = match get_argnum(fmt, &mut i, &mut vm)? {
                        Some(n) => n,
                        None => {
                            currarg += 1;
                            currarg - 1
                        }
                    };
                    if p >= argc {
                        max_missing = Some(max_missing.map_or(p, |m| m.max(p)));
                        continue 'outer;
                    }
                    let Value::Long(pv) = values[p].deref() else {
                        return Err(raise(&mut vm, "Precision must be an integer".to_string()));
                    };
                    if pv < -1 || pv > i32::MAX as i64 {
                        return Err(raise(
                            &mut vm,
                            format!("Precision must be between -1 and {}", i32::MAX),
                        ));
                    }
                    precision = pv;
                    expprec = true;
                } else if at(i).is_ascii_digit() {
                    precision = getnumber(fmt, &mut i);
                    if precision < 0 {
                        return Err(raise(
                            &mut vm,
                            format!("Precision must be between 0 and {}", i32::MAX),
                        ));
                    }
                    expprec = true;
                }
            }
        }

        if at(i) == b'l' {
            i += 1;
        }
        let argnum = match argnum {
            Some(n) => n,
            None => {
                currarg += 1;
                currarg - 1
            }
        };
        if argnum >= argc {
            max_missing = Some(max_missing.map_or(argnum, |m| m.max(argnum)));
            continue;
        }
        let spec = at(i);
        if expprec && precision == -1 && !matches!(spec, b'g' | b'G' | b'h' | b'H') {
            return Err(raise(
                &mut vm,
                "Precision -1 is only supported for %g, %G, %h and %H".to_string(),
            ));
        }
        let arg = &values[argnum];

        // zval_get_long() of an object: GMP-like objects cast, others warn
        // ("could not be converted to int") and give 1.
        fn object_warn(vm: &mut Option<&mut Vm>, arg: &Value, kind: u8, tname: &str) {
            if let Value::Object(obj) = arg.deref() {
                if goro_core::value::native_cast(&obj, kind).is_some() {
                    return;
                }
                if let Some(vm_ref) = vm.as_deref_mut() {
                    let class_name = goro_core::value::display_class_name(&obj.borrow().class_name);
                    vm_ref.emit_warning(&format!(
                        "Object of class {} could not be converted to {}",
                        class_name, tname
                    ));
                }
            }
        }
        /// zval_get_long(): out-of-range floats warn and wrap like
        /// zend_dval_to_lval().
        fn int_arg(vm: &mut Option<&mut Vm>, arg: &Value) -> i64 {
            let v = arg.deref();
            match &v {
                Value::Object(_) => {
                    if let Some(vm_ref) = vm.as_deref_mut() {
                        if vm_ref
                            .object_is_operator_type
                            .map(|f| f(&v))
                            .unwrap_or(false)
                        {
                            if let Some(n) = vm_ref.object_to_long.and_then(|f| f(&v)) {
                                return n;
                            }
                        }
                    }
                    object_warn(vm, arg, 1, "int");
                    1
                }
                Value::Double(f) => {
                    let f = *f;
                    if !f.is_finite() || !goro_core::value::php_double_fits_long(f) {
                        if let Some(vm_ref) = vm.as_deref_mut() {
                            vm_ref.emit_warning(&format!(
                                "The float {} is not representable as an int, cast occurred",
                                goro_core::value::format_php_float_repr(f)
                            ));
                        }
                        // (zend_dval_to_lval(): modular, 2**63 wraps to PHP_INT_MIN)
                        return goro_core::value::php_dval_to_lval(f);
                    }
                    f as i64
                }
                _ => v.to_long(),
            }
        }

        match spec {
            b's' => {
                if matches!(arg.deref(), Value::Array(_)) {
                    if let Some(vm_ref) = vm.as_deref_mut() {
                        vm_ref.emit_warning("Array to string conversion");
                    }
                }
                let s: Vec<u8> = if let Some(vm_ref) = vm.as_deref_mut() {
                    // zval_get_tmp_string(): on an exception the formatting
                    // goes on with an empty string.
                    match vm_ref.value_to_string_checked(arg) {
                        Ok(s) => s.as_bytes().to_vec(),
                        Err(e) => {
                            *pending = Some(e);
                            Vec::new()
                        }
                    }
                } else {
                    arg.to_php_string().as_bytes().to_vec()
                };
                sprintf_append_string(
                    &mut result,
                    &s,
                    width,
                    precision.max(0) as usize,
                    padding,
                    align_left,
                    false,
                    expprec,
                    false,
                );
            }
            b'd' => {
                let n = int_arg(&mut vm, arg);
                let mut s = Vec::new();
                if n >= 0 && always_sign {
                    s.push(b'+');
                }
                s.extend_from_slice(n.to_string().as_bytes());
                // Can't right-pad 0's on integers.
                let pad = if align_left && padding == b'0' {
                    b' '
                } else {
                    padding
                };
                sprintf_append_string(
                    &mut result,
                    &s,
                    width,
                    0,
                    pad,
                    align_left,
                    n < 0,
                    false,
                    always_sign,
                );
            }
            b'u' => {
                let n = int_arg(&mut vm, arg) as u64;
                let pad = if align_left && padding == b'0' {
                    b' '
                } else {
                    padding
                };
                sprintf_append_string(
                    &mut result,
                    n.to_string().as_bytes(),
                    width,
                    0,
                    pad,
                    align_left,
                    false,
                    false,
                    false,
                );
            }
            b'e' | b'E' | b'f' | b'F' | b'g' | b'G' | b'h' | b'H' => {
                object_warn(&mut vm, arg, 2, "float");
                let number = arg.to_double();
                // php_sprintf_appenddouble()
                const MAX_FLOAT_PRECISION: i64 = 53;
                let mut precision = precision;
                if !adj_precision {
                    precision = 6;
                } else if precision > MAX_FLOAT_PRECISION {
                    if let Some(vm_ref) = vm.as_deref_mut() {
                        let line = vm_ref.current_line;
                        vm_ref.emit_notice_at(
                            &format!(
                                "{}(): Requested precision of {} digits was truncated to PHP maximum of {} digits",
                                fname, precision, MAX_FLOAT_PRECISION
                            ),
                            line,
                        );
                    }
                    precision = MAX_FLOAT_PRECISION;
                }
                if number.is_nan() {
                    sprintf_append_string(
                        &mut result,
                        b"NaN",
                        3,
                        0,
                        padding,
                        align_left,
                        false,
                        false,
                        always_sign,
                    );
                    i += 1;
                    continue;
                }
                if number.is_infinite() {
                    let neg = number < 0.0;
                    let s: &[u8] = if neg { b"-INF" } else { b"INF" };
                    sprintf_append_string(
                        &mut result,
                        s,
                        0,
                        0,
                        padding,
                        align_left,
                        neg,
                        false,
                        always_sign,
                    );
                    i += 1;
                    continue;
                }
                let mut s = Vec::new();
                let is_negative;
                match spec {
                    b'e' | b'E' | b'f' | b'F' => {
                        let conv = if spec == b'f' { b'F' } else { spec };
                        let dp = if spec == b'f' {
                            crate::locale::decimal_point()
                        } else {
                            b'.'
                        };
                        let (digits, neg) = php_conv_fp(conv, number, precision as usize, dp);
                        is_negative = neg;
                        if neg {
                            s.push(b'-');
                        } else if always_sign {
                            s.push(b'+');
                        }
                        s.extend_from_slice(&digits);
                    }
                    _ => {
                        if precision == 0 {
                            precision = 1;
                        }
                        let dp = if matches!(spec, b'g' | b'G') {
                            crate::locale::decimal_point()
                        } else {
                            b'.'
                        };
                        let exp_char = if matches!(spec, b'G' | b'H') {
                            b'E'
                        } else {
                            b'e'
                        };
                        let g = goro_core::value::zend_gcvt(number, precision as i32, dp, exp_char);
                        is_negative = g.first() == Some(&b'-');
                        if !is_negative && always_sign {
                            s.push(b'+');
                        }
                        s.extend_from_slice(&g);
                    }
                }
                sprintf_append_string(
                    &mut result,
                    &s,
                    width,
                    0,
                    padding,
                    align_left,
                    is_negative,
                    false,
                    always_sign,
                );
            }
            b'c' => result.push(int_arg(&mut vm, arg) as u8),
            b'o' | b'x' | b'X' | b'b' => {
                let n = int_arg(&mut vm, arg) as u64;
                let s = match spec {
                    b'o' => format!("{:o}", n),
                    b'x' => format!("{:x}", n),
                    b'X' => format!("{:X}", n),
                    _ => format!("{:b}", n),
                };
                sprintf_append_string(
                    &mut result,
                    s.as_bytes(),
                    width,
                    0,
                    padding,
                    align_left,
                    false,
                    expprec,
                    false,
                );
            }
            b'%' => result.push(b'%'),
            0 if i >= fmt.len() => {
                return Err(raise(
                    &mut vm,
                    "Missing format specifier at end of string".to_string(),
                ));
            }
            _ => {
                return Err(raise(
                    &mut vm,
                    format!(
                        "Unknown format specifier \"{}\"",
                        goro_core::string::msg_char(spec)
                    ),
                ));
            }
        }
        i += 1;
    }

    if let Some(m) = max_missing {
        let nb = sprintf_additional_params(fname);
        if nb == -1 {
            return Err(raise(
                &mut vm,
                format!(
                    "The arguments array must contain {} items, {} given",
                    m + 1,
                    argc
                ),
            ));
        }
        return Err(raise_class(
            &mut vm,
            b"ArgumentCountError",
            format!(
                "{} arguments are required, {} given",
                m as i64 + nb + 1,
                argc as i64 + nb
            ),
        ));
    }
    Ok(result)
}

/// hebrev(): convert logical Hebrew text to visual text (string.c).
fn hebrev_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let input = zval_get_string(vm, args.first().unwrap_or(&Value::Null))?;
    let max_chars = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    Ok(Value::String(PhpString::from_vec(php_hebrev(
        input.as_bytes(),
        max_chars,
    ))))
}

fn php_hebrev(str: &[u8], max_chars: i64) -> Vec<u8> {
    let str_len = str.len();
    if str_len == 0 {
        return Vec::new();
    }
    let isheb = |c: u8| (224..=250).contains(&c);
    let isblank = |c: u8| c == b' ' || c == b'\t';
    let isnewline = |c: u8| c == b'\n' || c == b'\r';
    let ispunct = |c: u8| c.is_ascii_punctuation();
    // The C code reads the terminating NUL past the end.
    let at = |i: usize| str.get(i).copied().unwrap_or(0);

    let mut heb_str = vec![0u8; str_len];
    // `target` walks backwards from the end of heb_str.
    let mut target = str_len as isize - 1;
    let mut tmp = 0usize;
    let (mut block_start, mut block_end) = (0usize, 0usize);
    let mut heb_block = isheb(at(0));
    loop {
        if heb_block {
            while (isheb(at(tmp + 1))
                || isblank(at(tmp + 1))
                || ispunct(at(tmp + 1))
                || at(tmp + 1) == b'\n')
                && block_end < str_len - 1
            {
                tmp += 1;
                block_end += 1;
            }
            for i in block_start + 1..=block_end + 1 {
                let c = match str[i - 1] {
                    b'(' => b')',
                    b')' => b'(',
                    b'[' => b']',
                    b']' => b'[',
                    b'{' => b'}',
                    b'}' => b'{',
                    b'<' => b'>',
                    b'>' => b'<',
                    b'\\' => b'/',
                    b'/' => b'\\',
                    c => c,
                };
                if target >= 0 {
                    heb_str[target as usize] = c;
                }
                target -= 1;
            }
            heb_block = false;
        } else {
            while !isheb(at(tmp + 1)) && at(tmp + 1) != b'\n' && block_end < str_len - 1 {
                tmp += 1;
                block_end += 1;
            }
            while (isblank(at(tmp)) || ispunct(at(tmp)))
                && at(tmp) != b'/'
                && at(tmp) != b'-'
                && block_end > block_start
            {
                tmp -= 1;
                block_end -= 1;
            }
            let mut i = block_end + 1;
            while i > block_start {
                if target >= 0 {
                    heb_str[target as usize] = str[i - 1];
                }
                target -= 1;
                i -= 1;
            }
            heb_block = true;
        }
        block_start = block_end + 1;
        if block_end >= str_len - 1 {
            break;
        }
    }

    let mut broken = Vec::with_capacity(str_len);
    let mut begin = str_len - 1;
    let mut end = str_len - 1;
    loop {
        let mut char_count: i64 = 0;
        while (max_chars == 0 || (max_chars > 0 && char_count < max_chars)) && begin > 0 {
            char_count += 1;
            begin -= 1;
            if isnewline(heb_str[begin]) {
                while begin > 0 && isnewline(heb_str[begin - 1]) {
                    begin -= 1;
                    char_count += 1;
                }
                break;
            }
        }
        if max_chars >= 0 && char_count == max_chars {
            // try to avoid breaking words
            let mut new_char_count = char_count;
            let mut new_begin = begin;
            while new_char_count > 0 {
                if isblank(heb_str[new_begin]) || isnewline(heb_str[new_begin]) {
                    break;
                }
                new_begin += 1;
                new_char_count -= 1;
            }
            if new_char_count > 0 {
                begin = new_begin;
            }
        }
        let orig_begin = begin;
        if isblank(heb_str[begin]) {
            heb_str[begin] = b'\n';
        }
        while begin <= end && isnewline(heb_str[begin]) {
            begin += 1;
        }
        if begin <= end {
            broken.extend_from_slice(&heb_str[begin..=end]);
        }
        let mut i = orig_begin;
        while i <= end && isnewline(heb_str[i]) {
            broken.push(heb_str[i]);
            i += 1;
        }
        begin = orig_begin;
        if begin == 0 {
            break;
        }
        begin -= 1;
        end = begin;
    }
    broken.truncate(str_len);
    broken
}

fn chunk_split(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let body = args.first().unwrap_or(&Value::Null).to_php_string();
    // PHP 8: floats with a fractional component or values outside int range
    // are rejected with TypeError. Integer-valued floats (e.g. 6E+0) coerce.
    let chunklen = match args.get(1) {
        None => 76,
        Some(Value::Double(f)) => {
            let f = *f;
            if !f.is_finite() || f.fract() != 0.0 || f > i64::MAX as f64 || f < i64::MIN as f64 {
                let msg = "chunk_split(): Argument #2 ($length) must be of type int, float given"
                    .to_string();
                let exc = vm.throw_type_error(msg.clone());
                vm.current_exception = Some(exc);
                return Err(VmError {
                    message: msg,
                    line: vm.current_line,
                });
            }
            f as i64
        }
        Some(v) => v.to_long(),
    };
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
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
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
    let length = args.get(1).map(|v| v.to_long()).unwrap_or(0).max(0) as usize;
    let pad_string = match args.get(2) {
        Some(Value::Null) | None => PhpString::from_bytes(b" "),
        Some(v) => v.to_php_string(),
    };
    let pad_type = args.get(3).map(|v| v.to_long()).unwrap_or(1); // STR_PAD_RIGHT=1

    // PHP returns the input unchanged before validating the pad arguments.
    if input.len() >= length {
        return Ok(Value::String(input));
    }
    let pad_bytes = pad_string.as_bytes();
    if pad_bytes.is_empty() {
        let msg = "str_pad(): Argument #3 ($pad_string) must not be empty";
        let exc = vm.create_exception(b"ValueError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg.to_string(),
            line: 0,
        });
    }

    // Validate pad_type
    if pad_type != 0 && pad_type != 1 && pad_type != 2 {
        let msg = "str_pad(): Argument #4 ($pad_type) must be STR_PAD_LEFT, STR_PAD_RIGHT, or STR_PAD_BOTH";
        let exc = vm.create_exception(b"ValueError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg.to_string(),
            line: 0,
        });
    }

    let bytes = input.as_bytes();
    // zend_string_safe_alloc(1, len, num_pad_chars, 0)
    vm.memory_check(
        bytes
            .len()
            .saturating_add((length - bytes.len()).saturating_add(25 + 7) & !7),
    )?;

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

fn ucfirst(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = vm.value_to_string(args.first().unwrap_or(&Value::Null));
    let mut bytes = s.as_bytes().to_vec();
    if let Some(first) = bytes.first_mut() {
        *first = first.to_ascii_uppercase();
    }
    Ok(Value::String(PhpString::from_vec(bytes)))
}

fn lcfirst(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = vm.value_to_string(args.first().unwrap_or(&Value::Null));
    let mut bytes = s.as_bytes().to_vec();
    if let Some(first) = bytes.first_mut() {
        *first = first.to_ascii_lowercase();
    }
    Ok(Value::String(PhpString::from_vec(bytes)))
}

fn ucwords(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = vm.value_to_string(args.first().unwrap_or(&Value::Null));
    let delimiters = args.get(1).map(|v| v.to_php_string());
    let mut bytes = s.as_bytes().to_vec();
    let delim_bytes: Vec<u8> = if let Some(ref d) = delimiters {
        expand_charlist_warn(d.as_bytes(), vm, "ucwords")
    } else {
        b" \t\r\n\x0B\x0C".to_vec()
    };
    // PHP_FUNCTION(ucwords): the first byte is uppercased, then every byte
    // following a delimiter (tested after its own uppercasing).
    if let Some(first) = bytes.first_mut() {
        *first = first.to_ascii_uppercase();
    }
    for r in 1..bytes.len() {
        if delim_bytes.contains(&bytes[r - 1]) {
            bytes[r] = bytes[r].to_ascii_uppercase();
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
        } else if bytes[i] == b'\\' {
            // A trailing lone backslash is dropped.
            i += 1;
        } else {
            result.push(bytes[i]);
            i += 1;
        }
    }
    Ok(Value::String(PhpString::from_vec(result)))
}

fn addcslashes(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = vm.value_to_string(args.first().unwrap_or(&Value::Null));
    let charlist_val = args.get(1).unwrap_or(&Value::Null);
    // TypeError if charlist is an array
    if matches!(charlist_val, Value::Array(_)) {
        let msg = "addcslashes(): Argument #2 ($characters) must be of type string, array given"
            .to_string();
        let exc = vm.throw_type_error(msg.clone());
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: 0,
        });
    }
    let charlist = charlist_val.to_php_string();
    // php_charmask()
    let mut char_set = [false; 256];
    for c in expand_charlist_warn(charlist.as_bytes(), vm, "addcslashes") {
        char_set[c as usize] = true;
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
                0 => result.extend_from_slice(b"\\000"),
                _ if !(0x20..=0x7E).contains(&b) => {
                    // Other control chars and bytes > 126: use octal
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
                    // (char)strtol(numtmp, NULL, 8): \777 wraps to 0xFF
                    let v = oct.iter().fold(0u32, |v, &d| v * 8 + u32::from(d - b'0'));
                    result.push(v as u8);
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
                    } else {
                        // no hex digit: falls through to the literal 'x'
                        result.push(b'x');
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

/// php_quot_print_encode() (ext/standard/quot_print.c)
pub(crate) fn php_quot_print_encode(src: &[u8]) -> Vec<u8> {
    const MAXL: u64 = 75;
    let hex = b"0123456789ABCDEF";
    let at = |i: usize| -> u8 { src.get(i).copied().unwrap_or(0) };
    let mut d: Vec<u8> = Vec::with_capacity(src.len() * 3);
    let mut lp: u64 = 0;
    let mut i = 0usize;
    let mut length = src.len();
    while length > 0 {
        length -= 1;
        let c = src[i];
        i += 1;
        if c == b'\r' && at(i) == b'\n' && length > 0 {
            d.push(b'\r');
            d.push(src[i]);
            i += 1;
            length -= 1;
            lp = 0;
        } else if c < 0x20
            || c == 0x7f
            || c & 0x80 != 0
            || c == b'='
            || (c == b' ' && at(i) == b'\r')
        {
            lp += 3;
            if (lp > MAXL && c <= 0x7f)
                || (c > 0x7f && c <= 0xdf && lp + 3 > MAXL)
                || (c > 0xdf && c <= 0xef && lp + 6 > MAXL)
                || (c > 0xef && c <= 0xf4 && lp + 9 > MAXL)
            {
                d.extend_from_slice(b"=\r\n");
                lp = 3;
            }
            d.push(b'=');
            d.push(hex[(c >> 4) as usize]);
            d.push(hex[(c & 0xf) as usize]);
        } else {
            lp += 1;
            if lp > MAXL {
                d.extend_from_slice(b"=\r\n");
                lp = 1;
            }
            d.push(c);
        }
    }
    d
}

/// php_uuencode()
fn convert_uuencode(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let src = zpp_str(vm, args, 0, "convert_uuencode", "data")?;
    let b = src.as_bytes();
    let at = |i: usize| -> u8 { b.get(i).copied().unwrap_or(0) };
    let enc = |c: u8| -> u8 { if c != 0 { (c & 0o77) + b' ' } else { b'`' } };
    let enc_c2 = |i: usize| -> u8 { enc(((at(i) << 4) & 0o60) | ((at(i + 1) >> 4) & 0o17)) };
    let enc_c3 = |i: usize| -> u8 { enc(((at(i + 1) << 2) & 0o74) | ((at(i + 2) >> 6) & 0o3)) };
    let e = b.len();
    let mut out: Vec<u8> = Vec::with_capacity(e * 3 / 2 + 46);
    let mut s = 0usize;
    let mut len = 45usize;
    while s + 3 < e {
        let mut ee = s + len;
        if ee > e {
            ee = e;
            len = ee - s;
            if !len.is_multiple_of(3) {
                ee = s + (len / 3) * 3;
            }
        }
        out.push(enc(len as u8));
        while s < ee {
            out.push(enc(at(s) >> 2));
            out.push(enc_c2(s));
            out.push(enc_c3(s));
            out.push(enc(at(s + 2) & 0o77));
            s += 3;
        }
        if len == 45 {
            out.push(b'\n');
        }
    }
    if s < e {
        if len == 45 {
            out.push(enc((e - s) as u8));
            len = 0;
        }
        out.push(enc(at(s) >> 2));
        out.push(enc_c2(s));
        out.push(if e - s > 1 { enc_c3(s) } else { enc(0) });
        out.push(if e - s > 2 {
            enc(at(s + 2) & 0o77)
        } else {
            enc(0)
        });
    }
    if len < 45 {
        out.push(b'\n');
    }
    out.push(enc(0));
    out.push(b'\n');
    Ok(Value::String(PhpString::from_vec(out)))
}

/// php_uudecode()
fn convert_uudecode(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let src = zpp_str(vm, args, 0, "convert_uudecode", "data")?;
    let b = src.as_bytes();
    let dec = |c: u8| -> u8 { c.wrapping_sub(b' ') & 0o77 };
    let at = |i: usize| -> u8 { b.get(i).copied().unwrap_or(0) };
    let fail = |vm: &mut Vm| -> Result<Value, VmError> {
        vm.emit_warning("convert_uudecode(): Argument #1 ($data) is not a valid uuencoded string");
        Ok(Value::False)
    };
    if b.is_empty() {
        return fail(vm);
    }
    let e = b.len();
    let mut out: Vec<u8> = Vec::new();
    let mut total = 0usize;
    let mut s = 0usize;
    while s < e {
        let len = dec(b[s]) as usize;
        s += 1;
        if len == 0 {
            break;
        }
        if len > e {
            return fail(vm);
        }
        total += len;
        let ee = s + if len == 45 {
            60
        } else {
            (len as f64 * 1.33).floor() as usize
        };
        if ee > e {
            return fail(vm);
        }
        while s < ee {
            if s + 4 > e {
                return fail(vm);
            }
            out.push(dec(b[s]) << 2 | dec(b[s + 1]) >> 4);
            out.push(dec(b[s + 1]) << 4 | dec(b[s + 2]) >> 2);
            out.push(dec(b[s + 2]) << 6 | dec(b[s + 3]));
            s += 4;
        }
        if len < 45 {
            break;
        }
        s += 1;
    }
    if total > out.len() {
        out.push(dec(at(s)) << 2 | dec(at(s + 1)) >> 4);
        if total > 1 {
            out.push(dec(at(s + 1)) << 4 | dec(at(s + 2)) >> 2);
            if total > 2 {
                out.push(dec(at(s + 2)) << 6 | dec(at(s + 3)));
            }
        }
    }
    out.resize(total, 0);
    Ok(Value::String(PhpString::from_vec(out)))
}

/// strtr()'s zpp: ZEND_PARSE_PARAMETERS_START(2, 2) (string, array) or
/// (3, 3) (string, string, string) by argument count.
static STRTR_ARRAY: StubFunc = StubFunc {
    name: "strtr",
    flags: 0,
    args: &[
        StubArg {
            name: "string",
            ty: "string",
            default: None,
            flags: 0,
        },
        StubArg {
            name: "from",
            ty: "array",
            default: None,
            flags: 0,
        },
    ],
    ret: "string",
    ext: "standard",
};
static STRTR_STRINGS: StubFunc = StubFunc {
    name: "strtr",
    flags: 0,
    args: &[
        StubArg {
            name: "string",
            ty: "string",
            default: None,
            flags: 0,
        },
        StubArg {
            name: "from",
            ty: "string",
            default: None,
            flags: 0,
        },
        StubArg {
            name: "to",
            ty: "string",
            default: None,
            flags: 0,
        },
    ],
    ret: "string",
    ext: "standard",
};

fn strtr(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let sig = if args.len() <= 2 {
        &STRTR_ARRAY
    } else {
        &STRTR_STRINGS
    };
    let checked = vm.zpp_stub_check(
        sig,
        goro_core::arginfo::CHECK_ARITY | goro_core::arginfo::CHECK_TYPES,
        args,
    )?;
    let args = &checked[..];
    let subject = args[0].to_php_string();

    // PHP shortcuts an empty subject before doing any work — no warnings
    // are emitted even when the replacement set contains empty keys.
    if subject.as_bytes().is_empty() {
        return Ok(Value::String(subject));
    }

    // Two forms: strtr($str, $from, $to) or strtr($str, $replacements_array).
    // The 3-arg form requires $from to be a string (scalars coerce; null emits
    // a deprecation); array and resource raise TypeError. The 2-arg form
    // requires $from to be an array; everything else raises TypeError.
    if args.len() >= 3 {
        let from = args[1].to_php_string();
        let to = args[2].to_php_string();
        let from_bytes = from.as_bytes();
        let to_bytes = to.as_bytes();
        let min_len = from_bytes.len().min(to_bytes.len());
        // PHP keeps the LAST mapping when a byte appears twice in $from, so
        // iterate forward and overwrite rather than break on first match.
        let mut map: [Option<u8>; 256] = [None; 256];
        for i in 0..min_len {
            map[from_bytes[i] as usize] = Some(to_bytes[i]);
        }
        let mut result: Vec<u8> = subject.as_bytes().to_vec();
        for byte in &mut result {
            if let Some(r) = map[*byte as usize] {
                *byte = r;
            }
        }
        Ok(Value::String(PhpString::from_vec(result)))
    } else if let Some(Value::Array(replacements)) = args.get(1).map(|v| v.deref()) {
        // php_strtr_array()
        let entries: Vec<(Vec<u8>, Value)> = replacements
            .borrow()
            .iter()
            .map(|(k, v)| {
                let key = match k {
                    goro_core::array::ArrayKey::String(s) => s.as_bytes().to_vec(),
                    goro_core::array::ArrayKey::Int(n) => n.to_string().into_bytes(),
                };
                (key, v.clone())
            })
            .collect();
        let src = subject.as_bytes();
        if entries.is_empty() {
            return Ok(Value::String(subject));
        }
        if entries.len() == 1 {
            // The replacement is converted up front.
            let (key, value) = &entries[0];
            let replace = strtr_zval_get_string(vm, value)?;
            if key.is_empty() {
                vm.emit_warning_at(
                    "strtr(): Ignoring replacement of empty string",
                    vm.current_line,
                );
                return Ok(Value::String(subject));
            }
            let mut result = Vec::with_capacity(src.len());
            let mut i = 0;
            while i < src.len() {
                if src[i..].starts_with(key) {
                    result.extend_from_slice(replace.as_bytes());
                    i += key.len();
                } else {
                    result.push(src[i]);
                    i += 1;
                }
            }
            return Ok(Value::String(PhpString::from_vec(result)));
        }
        // php_strtr_array_ex(): longest match first at each position; a
        // replacement is converted to a string each time it is used.
        let mut pairs: Vec<(Vec<u8>, Value)> = Vec::with_capacity(entries.len());
        for (key, value) in entries {
            if key.is_empty() {
                vm.emit_warning_at(
                    "strtr(): Ignoring replacement of empty string",
                    vm.current_line,
                );
                continue;
            }
            if key.len() <= src.len() {
                pairs.push((key, value));
            }
        }
        pairs.sort_by_key(|p| std::cmp::Reverse(p.0.len()));

        let mut result = Vec::with_capacity(src.len());
        let mut i = 0;
        while i < src.len() {
            let found = pairs
                .iter()
                .find(|(from, _)| src[i..].starts_with(from))
                .map(|(from, to)| (from.len(), to.clone()));
            match found {
                Some((len, to)) => {
                    let to = strtr_zval_get_string(vm, &to)?;
                    result.extend_from_slice(to.as_bytes());
                    i += len;
                }
                None => {
                    result.push(src[i]);
                    i += 1;
                }
            }
        }
        Ok(Value::String(PhpString::from_vec(result)))
    } else if args.len() == 2 {
        // strtr($str, $replace_pairs) - second arg must be an array
        let val = &args[1];
        let type_name = goro_core::vm::Vm::value_type_name(val);
        let msg = format!(
            "strtr(): Argument #2 ($from) must be of type array, {} given",
            type_name
        );
        let exc = vm.throw_type_error(msg.clone());
        vm.current_exception = Some(exc);
        Err(VmError {
            message: msg,
            line: vm.current_line,
        })
    } else {
        Ok(Value::String(subject))
    }
}

/// zval_get_string() of a strtr() replacement.
fn strtr_zval_get_string(vm: &mut Vm, v: &Value) -> Result<PhpString, VmError> {
    let v = v.deref();
    if matches!(v, Value::Array(_)) {
        vm.emit_warning_at("Array to string conversion", vm.current_line);
        return Ok(PhpString::from_bytes(b"Array"));
    }
    vm.value_to_string_checked(&v)
}

fn str_shuffle(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let mut bytes = s.as_bytes().to_vec();
    if bytes.len() > 1 {
        let engine = goro_core::random::default_engine();
        goro_core::random::shuffle_items(vm, &engine, &mut bytes);
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
            let msg = "substr_compare(): Argument #4 ($length) must be greater than or equal to 0"
                .to_string();
            let exc = vm.throw_type_error(msg.clone());
            if let Value::Object(obj) = &exc {
                obj.borrow_mut().class_name = b"ValueError".to_vec();
            }
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
    }

    // An explicit length of 0 is always 0, even for an out-of-range offset.
    if let Some(0) = length {
        return Ok(Value::Long(0));
    }

    // Resolve negative offset (PHP clamps to 0 if it would go before start).
    let resolved_offset = if offset < 0 {
        (main_len + offset).max(0)
    } else {
        offset
    };
    if resolved_offset > main_len {
        let msg =
            "substr_compare(): Argument #3 ($offset) must be contained in argument #1 ($haystack)"
                .to_string();
        let exc = vm.throw_type_error(msg.clone());
        if let Value::Object(obj) = &exc {
            obj.borrow_mut().class_name = b"ValueError".to_vec();
        }
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    let start = resolved_offset as usize;

    let sub = &main_bytes[start..]; // s1 + offset
    let s2_bytes = str2.as_bytes();
    let len1 = sub.len();
    let len2 = s2_bytes.len();
    // PHP: cmp_len = len ? len : MAX(len2, len1)
    let cmp_len = match length {
        Some(l) if l > 0 => l as usize,
        _ => len1.max(len2),
    };

    // PHP's zend_binary_strncmp: memcmp prefix of MIN(cmp_len, len1, len2),
    // then if equal, three-way-compare MIN(cmp_len, len1) vs MIN(cmp_len, len2).
    let prefix_len = cmp_len.min(len1).min(len2);
    let (a_prefix, b_prefix) = if case_insensitive {
        let a: Vec<u8> = sub[..prefix_len]
            .iter()
            .map(|c| c.to_ascii_lowercase())
            .collect();
        let b: Vec<u8> = s2_bytes[..prefix_len]
            .iter()
            .map(|c| c.to_ascii_lowercase())
            .collect();
        (a, b)
    } else {
        (sub[..prefix_len].to_vec(), s2_bytes[..prefix_len].to_vec())
    };
    // (the first differing bytes' difference, as memcmp()/tolower() give it)
    if let Some((x, y)) = a_prefix.iter().zip(&b_prefix).find(|(x, y)| x != y) {
        return Ok(Value::Long(*x as i64 - *y as i64));
    }
    let l1 = cmp_len.min(len1) as i64;
    let l2 = cmp_len.min(len2) as i64;
    Ok(Value::Long((l1 - l2).signum()))
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
        let pct = if total > 0.0 {
            (sim as f64 * 200.0) / total
        } else {
            0.0
        };
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

fn metaphone(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Faithful port of PHP's ext/standard/metaphone.c (traditional=1).
    let word = args.first().unwrap_or(&Value::Null).to_php_string();
    let max_phonemes = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    if max_phonemes < 0 {
        let msg = "metaphone(): Argument #2 ($max_phonemes) must be greater than or equal to 0"
            .to_string();
        let exc = vm.create_exception(b"ValueError", &msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    // metaphone.c walks a NUL-terminated C string.
    let word_bytes = word.as_bytes();
    let word_bytes = &word_bytes[..word_bytes
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(word_bytes.len())];
    // zend_string_alloc(max_phonemes + 1) of the phoned word
    if max_phonemes > 0 {
        vm.memory_check(((max_phonemes as usize).saturating_add(26 + 7)) & !7)?;
    }
    let max = if max_phonemes > 0 {
        max_phonemes as usize
    } else {
        usize::MAX
    };

    // _codes table from metachar.h, indexed by letter (A-Z, uppercase).
    // bit 0: vowel (AEIOU)
    // bit 1: passthrough (FJMNR)
    // bit 2: AFFECTH (CGPST) — diphthong-former before H
    // bit 3: MAKESOFT (EIY) — makes C/G soft
    // bit 4: NOGHTOF (BDH) — prevents GH→F
    const CODES: [u8; 26] = [
        1, 16, 4, 16, 9, 2, 4, 16, 9, 2, 0, 2, 2, 2, 1, 4, 0, 2, 4, 4, 1, 0, 0, 0, 8, 0,
    ];
    let encode = |c: u8| -> u8 {
        if c.is_ascii_alphabetic() {
            CODES[(c.to_ascii_uppercase() - b'A') as usize]
        } else {
            0
        }
    };
    let isvowel = |c: u8| encode(c) & 1 != 0;
    let affecth = |c: u8| encode(c) & 4 != 0;
    let makesoft = |c: u8| encode(c) & 8 != 0;
    let noghtof = |c: u8| encode(c) & 16 != 0;
    let isbreak = |c: u8| !c.is_ascii_alphabetic();

    let convert_raw = |c: u8| c.to_ascii_uppercase();

    let read_at = |idx: usize| -> u8 {
        if idx < word_bytes.len() {
            word_bytes[idx]
        } else {
            0
        }
    };
    let read_letter_at = |idx: usize| -> u8 { convert_raw(read_at(idx)) };

    let mut result: Vec<u8> = Vec::new();
    // Find first letter.
    let mut w_idx: usize = 0;
    while w_idx < word_bytes.len() && !word_bytes[w_idx].is_ascii_alphabetic() {
        w_idx += 1;
    }
    if w_idx >= word_bytes.len() {
        return Ok(Value::String(PhpString::empty()));
    }

    // Phonize(): max_phonemes is only checked before each letter, so one
    // letter's code (X -> KS) may exceed it.
    let phonize = |result: &mut Vec<u8>, c: u8| result.push(c);

    // First letter handling
    let first = read_letter_at(w_idx);
    match first {
        b'A' => {
            // AE becomes E, else A (preserve initial vowel)
            if read_letter_at(w_idx + 1) == b'E' {
                phonize(&mut result, b'E');
                w_idx += 2;
            } else {
                phonize(&mut result, b'A');
                w_idx += 1;
            }
        }
        b'G' | b'K' | b'P' => {
            // [GKP]N becomes N
            if read_letter_at(w_idx + 1) == b'N' {
                phonize(&mut result, b'N');
                w_idx += 2;
            }
            // Else: do nothing, w_idx stays — main loop will process it
        }
        b'W' => {
            let next = read_letter_at(w_idx + 1);
            if next == b'R' {
                phonize(&mut result, b'R');
                w_idx += 2;
            } else if next == b'H' || isvowel(next) {
                phonize(&mut result, b'W');
                w_idx += 2;
            }
            // else: ignore (drop W), w_idx stays
        }
        b'X' => {
            // X becomes S at start
            phonize(&mut result, b'S');
            w_idx += 1;
        }
        b'E' | b'I' | b'O' | b'U' => {
            // Vowels are preserved at the start
            phonize(&mut result, first);
            w_idx += 1;
        }
        _ => {
            // No first-letter encoding; main loop will handle this letter
        }
    }

    // Main loop
    while w_idx < word_bytes.len() && result.len() < max {
        let raw = word_bytes[w_idx];
        if !raw.is_ascii_alphabetic() {
            w_idx += 1;
            continue;
        }
        let curr = convert_raw(raw);
        // Look at previous letter (in the raw input)
        let prev = if w_idx >= 1 {
            convert_raw(word_bytes[w_idx - 1])
        } else {
            0
        };

        // Drop duplicates, except CC
        if curr == prev && curr != b'C' {
            w_idx += 1;
            continue;
        }

        let mut skip = 0usize;
        let next = read_letter_at(w_idx + 1);
        let after_next = read_letter_at(w_idx + 2);

        match curr {
            b'B' => {
                if prev != b'M' {
                    phonize(&mut result, b'B');
                }
            }
            b'C' => {
                if makesoft(next) {
                    // C[EIY]
                    if next == b'I' && after_next == b'A' {
                        phonize(&mut result, b'X');
                    } else if prev == b'S' {
                        // SC[IEY] - dropped
                    } else {
                        phonize(&mut result, b'S');
                    }
                } else if next == b'H' {
                    // CH - SH (traditional=1 in PHP's metaphone)
                    phonize(&mut result, b'X');
                    skip += 1;
                } else {
                    phonize(&mut result, b'K');
                }
            }
            b'D' => {
                if next == b'G' && makesoft(after_next) {
                    phonize(&mut result, b'J');
                    skip += 1;
                } else {
                    phonize(&mut result, b'T');
                }
            }
            b'G' => {
                if next == b'H' {
                    let look3 = if w_idx >= 3 {
                        convert_raw(word_bytes[w_idx - 3])
                    } else {
                        0
                    };
                    let look4 = if w_idx >= 4 {
                        convert_raw(word_bytes[w_idx - 4])
                    } else {
                        0
                    };
                    if !(noghtof(look3) || look4 == b'H') {
                        phonize(&mut result, b'F');
                        skip += 1;
                    }
                    // else silent
                } else if next == b'N' {
                    // -GN dropped if at break or -GNED
                    let after_next_raw = read_at(w_idx + 2);
                    let look_ahead_3 = read_letter_at(w_idx + 3);
                    if isbreak(after_next_raw) || (after_next == b'E' && look_ahead_3 == b'D') {
                        // dropped
                    } else {
                        phonize(&mut result, b'K');
                    }
                } else if makesoft(next) && prev != b'G' {
                    phonize(&mut result, b'J');
                } else {
                    phonize(&mut result, b'K');
                }
            }
            b'H' => {
                if isvowel(next) && !affecth(prev) {
                    phonize(&mut result, b'H');
                }
            }
            b'K' => {
                if prev != b'C' {
                    phonize(&mut result, b'K');
                }
            }
            b'P' => {
                if next == b'H' {
                    phonize(&mut result, b'F');
                } else {
                    phonize(&mut result, b'P');
                }
            }
            b'Q' => {
                phonize(&mut result, b'K');
            }
            b'S' => {
                if next == b'I' && (after_next == b'O' || after_next == b'A') {
                    phonize(&mut result, b'X');
                } else if next == b'H' {
                    phonize(&mut result, b'X');
                    skip += 1;
                } else {
                    phonize(&mut result, b'S');
                }
            }
            b'T' => {
                if next == b'I' && (after_next == b'O' || after_next == b'A') {
                    phonize(&mut result, b'X');
                } else if next == b'H' {
                    phonize(&mut result, b'0');
                    skip += 1;
                } else if !(next == b'C' && after_next == b'H') {
                    // TCH is silent (skip T, let CH handle on next iteration)
                    phonize(&mut result, b'T');
                }
            }
            b'V' => {
                phonize(&mut result, b'F');
            }
            b'W' => {
                if isvowel(next) {
                    phonize(&mut result, b'W');
                }
            }
            b'X' => {
                phonize(&mut result, b'K');
                phonize(&mut result, b'S');
            }
            b'Y' => {
                if isvowel(next) {
                    phonize(&mut result, b'Y');
                }
            }
            b'Z' => {
                phonize(&mut result, b'S');
            }
            b'F' | b'J' | b'L' | b'M' | b'N' | b'R' => {
                phonize(&mut result, curr);
            }
            _ => {}
        }
        w_idx += 1 + skip;
    }

    Ok(Value::String(PhpString::from_vec(result)))
}

#[allow(clippy::needless_range_loop)] // index loop mirrors the C implementation
/// reference_levdist() (ext/standard/levenshtein.c): bytes, signed costs.
fn levenshtein(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s1 = args.first().unwrap_or(&Value::Null).to_php_string();
    let s2 = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let (s1, s2) = (s1.as_bytes(), s2.as_bytes());
    let cost = |i: usize| args.get(i).map(|v| v.to_long()).unwrap_or(1);
    let (ins, rep, del) = (cost(2), cost(3), cost(4));
    if s1.is_empty() {
        return Ok(Value::Long((s2.len() as i64).wrapping_mul(ins)));
    }
    if s2.is_empty() {
        return Ok(Value::Long((s1.len() as i64).wrapping_mul(del)));
    }
    let mut p1: Vec<i64> = (0..=s2.len() as i64).map(|i| i.wrapping_mul(ins)).collect();
    let mut p2 = vec![0i64; s2.len() + 1];
    for &c1 in s1 {
        p2[0] = p1[0].wrapping_add(del);
        for (i2, &c2) in s2.iter().enumerate() {
            let mut c0 = p1[i2].wrapping_add(if c1 == c2 { 0 } else { rep });
            let d = p1[i2 + 1].wrapping_add(del);
            if d < c0 {
                c0 = d;
            }
            let n = p2[i2].wrapping_add(ins);
            if n < c0 {
                c0 = n;
            }
            p2[i2 + 1] = c0;
        }
        std::mem::swap(&mut p1, &mut p2);
    }
    Ok(Value::Long(p1[s2.len()]))
}

#[allow(clippy::needless_range_loop)] // index loop mirrors the C implementation
fn count_chars(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let mode = args.get(1).map(|v| v.to_long()).unwrap_or(0);

    if !(0..=4).contains(&mode) {
        let msg =
            "count_chars(): Argument #2 ($mode) must be between 0 and 4 (inclusive)".to_string();
        let exc = vm.throw_type_error(msg.clone());
        if let Value::Object(obj) = &exc {
            obj.borrow_mut().class_name = b"ValueError".to_vec();
        }
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
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

/// Z_PARAM_STR: scalars convert, Stringable objects use __toString; arrays,
/// resources and other objects throw a TypeError.
pub(crate) fn zpp_str(
    vm: &mut Vm,
    args: &[Value],
    idx: usize,
    fname: &str,
    pname: &str,
) -> Result<PhpString, VmError> {
    let v = args.get(idx).cloned().unwrap_or(Value::Null).deref();
    let given = match &v {
        Value::Array(_) => "array".to_string(),
        Value::Resource(_) => "resource".to_string(),
        Value::Object(o) => {
            let has = vm
                .get_class_def(&o.borrow().class_name.to_ascii_lowercase())
                .map(|c| c.get_method(b"__tostring").is_some())
                .unwrap_or(false);
            if has {
                return Ok(vm.value_to_string(&v));
            }
            goro_core::value::display_class_name(&o.borrow().class_name)
        }
        _ => return Ok(v.to_php_string()),
    };
    let msg = format!(
        "{}(): Argument #{} (${}) must be of type string, {} given",
        fname,
        idx + 1,
        pname,
        given
    );
    let exc = vm.throw_type_error(msg.clone());
    vm.current_exception = Some(exc);
    Err(VmError {
        message: msg,
        line: vm.current_line,
    })
}

fn strstr_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let haystack = vm.value_to_string(args.first().unwrap_or(&Value::Null));
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
        return Err(VmError {
            message: msg.into(),
            line: vm.current_line,
        });
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
    crate::zsort::strnatcmp_ex(a, b, case_insensitive) as i64
}

fn strnatcmp_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = vm.value_to_string(args.first().unwrap_or(&Value::Null));
    let b = vm.value_to_string(args.get(1).unwrap_or(&Value::Null));
    Ok(Value::Long(strnatcmp_impl(
        a.as_bytes(),
        b.as_bytes(),
        false,
    )))
}

fn strnatcasecmp_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = vm.value_to_string(args.first().unwrap_or(&Value::Null));
    let b = vm.value_to_string(args.get(1).unwrap_or(&Value::Null));
    Ok(Value::Long(strnatcmp_impl(
        a.as_bytes(),
        b.as_bytes(),
        true,
    )))
}

/// zend_binary_strncmp() (zend_binary_strcmp() with `length` = usize::MAX):
/// memcmp()'s difference of the first differing bytes (as glibc returns it),
/// else the three-way comparison of the compared lengths.
fn zend_binary_strncmp(a: &[u8], b: &[u8], length: usize) -> i64 {
    let (la, lb) = (a.len().min(length), b.len().min(length));
    let n = la.min(lb);
    match a[..n].iter().zip(&b[..n]).find(|(x, y)| x != y) {
        Some((x, y)) => *x as i64 - *y as i64,
        None => (la as i64 - lb as i64).signum(),
    }
}

fn strcmp_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = vm.value_to_string(args.first().unwrap_or(&Value::Null));
    let b = vm.value_to_string(args.get(1).unwrap_or(&Value::Null));
    Ok(Value::Long(zend_binary_strncmp(
        a.as_bytes(),
        b.as_bytes(),
        usize::MAX,
    )))
}

fn strncmp_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = vm.value_to_string(args.first().unwrap_or(&Value::Null));
    let b = vm.value_to_string(args.get(1).unwrap_or(&Value::Null));
    let len_raw = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    if len_raw < 0 {
        let msg = "strncmp(): Argument #3 ($length) must be greater than or equal to 0".to_string();
        let exc = vm.throw_type_error(msg.clone());
        if let Value::Object(obj) = &exc {
            obj.borrow_mut().class_name = b"ValueError".to_vec();
        }
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    Ok(Value::Long(zend_binary_strncmp(
        a.as_bytes(),
        b.as_bytes(),
        len_raw as usize,
    )))
}

fn strcasecmp_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = vm.value_to_string(args.first().unwrap_or(&Value::Null));
    let b = vm.value_to_string(args.get(1).unwrap_or(&Value::Null));
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    let min_len = a_bytes.len().min(b_bytes.len());
    for i in 0..min_len {
        let ca = a_bytes[i].to_ascii_lowercase();
        let cb = b_bytes[i].to_ascii_lowercase();
        if ca != cb {
            // zend_binary_strcasecmp: difference of the first mismatching bytes
            return Ok(Value::Long(ca as i64 - cb as i64));
        }
    }
    let d = a_bytes.len() as i64 - b_bytes.len() as i64;
    Ok(Value::Long(if d < 0 {
        -1
    } else if d > 0 {
        1
    } else {
        0
    }))
}

fn strncasecmp_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = vm.value_to_string(args.first().unwrap_or(&Value::Null));
    let b = vm.value_to_string(args.get(1).unwrap_or(&Value::Null));
    let len_raw = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    if len_raw < 0 {
        let msg =
            "strncasecmp(): Argument #3 ($length) must be greater than or equal to 0".to_string();
        let exc = vm.throw_type_error(msg.clone());
        if let Value::Object(obj) = &exc {
            obj.borrow_mut().class_name = b"ValueError".to_vec();
        }
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    let len = len_raw as usize;
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    let a_sub_len = len.min(a_bytes.len());
    let b_sub_len = len.min(b_bytes.len());
    // strncasecmp returns the raw byte difference (c1 - c2) when bytes differ,
    // and the length difference (normalized to -1/0/1) when bytes match.
    let compare_len = a_sub_len.min(b_sub_len);
    for i in 0..compare_len {
        let ca = a_bytes[i].to_ascii_lowercase();
        let cb = b_bytes[i].to_ascii_lowercase();
        if ca != cb {
            return Ok(Value::Long(ca as i64 - cb as i64));
        }
    }
    let len_diff = a_sub_len as i64 - b_sub_len as i64;
    Ok(Value::Long(if len_diff < 0 {
        -1
    } else if len_diff > 0 {
        1
    } else {
        0
    }))
}

fn vprintf_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        let msg = "vprintf() expects at least 1 argument, 0 given";
        let exc = vm.create_exception(b"ArgumentCountError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg.into(),
            line: vm.current_line,
        });
    }
    // Check format arg type
    validate_sprintf_format_arg(vm, "vprintf", args.first())?;
    deprecate_null_format(vm, "vprintf", 1, args.first());
    let format = vm.value_to_string(args.first().unwrap_or(&Value::Null));
    let arr = args.get(1).unwrap_or(&Value::Null);
    validate_sprintf_values_arg(vm, "vprintf", arr)?;
    let fmt_args: Vec<Value> = if let Value::Array(a) = arr.deref() {
        a.borrow().values().cloned().collect()
    } else {
        vec![]
    };
    let mut all_args = vec![Value::String(format)];
    all_args.extend(fmt_args);
    let (result, pending) = do_sprintf_vm_named_partial(vm, "vprintf", &all_args)?;
    let len = result.len();
    vm.write_output(&result);
    pending.map_or(Ok(Value::Long(len as i64)), Err)
}

fn printf_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        let msg = "printf() expects at least 1 argument, 0 given";
        let exc = vm.create_exception(b"ArgumentCountError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg.into(),
            line: vm.current_line,
        });
    }
    validate_sprintf_format_arg(vm, "printf", args.first())?;
    deprecate_null_format(vm, "printf", 1, args.first());
    let (result, pending) = do_sprintf_vm_named_partial(vm, "printf", args)?;
    let len = result.len();
    vm.write_output(&result);
    pending.map_or(Ok(Value::Long(len as i64)), Err)
}

fn strrchr(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let haystack = vm.value_to_string(args.first().unwrap_or(&Value::Null));
    let needle = args.get(1).unwrap_or(&Value::Null);
    let before_needle = args.get(2).map(|v| v.is_truthy()).unwrap_or(false);
    // PHP 8: needle is coerced to string (Z_PARAM_STR), then strrchr uses first byte.
    // This means int 42 -> "42" -> first byte '4' (not ASCII char '*').
    let needle_str = vm.value_to_string(needle);
    let needle_byte = if needle_str.is_empty() {
        0u8
    } else {
        needle_str.as_bytes()[0]
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

fn stristr(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let haystack = zpp_str(vm, args, 0, "stristr", "haystack")?;
    let needle = zpp_str(vm, args, 1, "stristr", "needle")?;
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
    static STRTOK_STATE: RefCell<(Vec<u8>, usize, bool)> = const { RefCell::new((Vec::new(), 0, false)) };
}

fn strtok_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // strtok has two forms:
    // strtok(string, delimiters) - initializes with new string
    // strtok(delimiters) - continues tokenizing the current string
    let delim = if args.len() >= 2 {
        // strtok(string, delimiters) - set new string
        let s = args[0].to_php_string();
        let d = args[1].to_php_string();
        STRTOK_STATE.with(|state| {
            let mut st = state.borrow_mut();
            st.0 = s.as_bytes().to_vec();
            st.1 = 0;
            st.2 = true;
        });
        d
    } else {
        // strtok(delimiters) - continue with saved string. If never initialized,
        // PHP emits a warning and returns false.
        let initialized = STRTOK_STATE.with(|state| state.borrow().2);
        if !initialized {
            vm.emit_warning_at(
                "strtok(): Both arguments must be provided when starting tokenization",
                vm.current_line,
            );
            return Ok(Value::False);
        }
        args.first().unwrap_or(&Value::Null).to_php_string()
    };
    let delim_bytes = delim.as_bytes();

    STRTOK_STATE.with(|state| {
        let mut st = state.borrow_mut();
        let initial_pos = st.1;
        let s_len = st.0.len();

        // PHP source (ext/standard/string.c, PHP_FUNCTION(strtok)):
        //   p = BG(strtok_last);
        //   pe = strtok_string + strtok_len;
        //   if (p >= pe) RETURN_FALSE;  // path A: silent end, keeps state initialized
        //   ... scan ...
        //   while leading delims: if (++p >= pe) goto return_false;
        //   return_false: release strtok_string; BG(strtok_string) = NULL;  // path B: clears state
        //
        // Path A (already at end) keeps state alive so subsequent continuations
        // return false silently. Path B (scan-consumed-all-as-delims) clears
        // state so the *next* continuation emits the "Both arguments must be
        // provided when starting tokenization" warning.

        // Path A: starting at end -> silent false.
        if initial_pos >= s_len {
            return Ok(Value::False);
        }

        // Skip leading delimiter characters
        let mut pos = initial_pos;
        while pos < s_len && delim_bytes.contains(&st.0[pos]) {
            pos += 1;
        }

        if pos >= s_len {
            // Path B: scan consumed everything as delimiters -> clear state.
            st.0.clear();
            st.1 = 0;
            st.2 = false;
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
        s_len.saturating_sub(offset.unsigned_abs() as usize)
    };
    let end = match length {
        Some(l) if l < 0 => {
            let end_pos = (s_len as i64).saturating_add(l);
            if end_pos <= start as i64 {
                start
            } else {
                end_pos as usize
            }
        }
        Some(l) => {
            let end_pos = (start as i64).saturating_add(l);
            if end_pos > s_len as i64 {
                s_len
            } else if end_pos < start as i64 {
                start
            } else {
                end_pos as usize
            }
        }
        None => s_len,
    };
    (start, end)
}

fn strspn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let chars = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let offset = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    // ?int $length = null: null is no limit.
    let length = args
        .get(3)
        .filter(|v| !matches!(v.deref(), Value::Null | Value::Undef))
        .map(|v| v.to_long());
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
    // ?int $length = null: null is no limit.
    let length = args
        .get(3)
        .filter(|v| !matches!(v.deref(), Value::Null | Value::Undef))
        .map(|v| v.to_long());
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
        return Err(VmError {
            message: msg.into(),
            line: vm.current_line,
        });
    }
    validate_sprintf_format_arg(vm, "vsprintf", args.first())?;
    let format = vm.value_to_string(args.first().unwrap_or(&Value::Null));
    let arr = args.get(1).unwrap_or(&Value::Null);
    validate_sprintf_values_arg(vm, "vsprintf", arr)?;
    let fmt_args: Vec<Value> = if let Value::Array(a) = arr.deref() {
        a.borrow().values().cloned().collect()
    } else {
        vec![]
    };
    let mut all_args = vec![Value::String(format)];
    all_args.extend(fmt_args);
    let result = do_sprintf_vm_named(vm, "vsprintf", &all_args)?;
    Ok(Value::String(PhpString::from_vec(result)))
}

fn substr_count(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let haystack = args.first().unwrap_or(&Value::Null).to_php_string();
    let needle = args.get(1).unwrap_or(&Value::Null).to_php_string();
    if needle.is_empty() {
        // PHP 8: throw ValueError for empty needle
        let msg = "substr_count(): Argument #2 ($needle) must not be empty";
        let exc = vm.create_exception(b"ValueError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg.into(),
            line: vm.current_line,
        });
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
            return Err(VmError {
                message: msg.into(),
                line: vm.current_line,
            });
        }
        s as usize
    } else {
        if raw_offset > h_len {
            let msg = "substr_count(): Argument #3 ($offset) must be contained in argument #1 ($haystack)";
            let exc = vm.create_exception(b"ValueError", msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg.into(),
                line: vm.current_line,
            });
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
                    return Err(VmError {
                        message: msg.into(),
                        line: vm.current_line,
                    });
                }
                e as usize
            } else {
                let e = start + len as usize;
                if e > h_bytes.len() {
                    let msg = "substr_count(): Argument #4 ($length) must be contained in argument #1 ($haystack)";
                    let exc = vm.create_exception(b"ValueError", msg, 0);
                    vm.current_exception = Some(exc);
                    return Err(VmError {
                        message: msg.into(),
                        line: vm.current_line,
                    });
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
    let n = needle.as_bytes();
    let mut count = 0i64;
    let mut i = 0;
    while i + n.len() <= search_bytes.len() {
        if &search_bytes[i..i + n.len()] == n {
            count += 1;
            i += n.len();
        } else {
            i += 1;
        }
    }
    Ok(Value::Long(count))
}

fn str_ireplace(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    php_str_replace_common(vm, args, "str_ireplace", true)
}

fn wordwrap(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    // Keep width as signed; negative widths trigger break-on-every-iteration
    // (matches PHP semantics).
    let width: i64 = args.get(1).map(|v| v.to_long()).unwrap_or(75);
    let brk = args
        .get(2)
        .map(|v| v.to_php_string())
        .unwrap_or_else(|| PhpString::from_bytes(b"\n"));
    let cut_long = args.get(3).map(|v| v.is_truthy()).unwrap_or(false);

    let bytes = s.as_bytes();
    let brk_bytes = brk.as_bytes();

    if bytes.is_empty() {
        return Ok(Value::String(PhpString::empty()));
    }

    if brk_bytes.is_empty() {
        let msg = "wordwrap(): Argument #3 ($break) must not be empty".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }

    if width == 0 && cut_long {
        let msg = "wordwrap(): Argument #4 ($cut_long_words) cannot be true when argument #2 ($width) is 0";
        let exc = vm.create_exception(b"ValueError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg.into(),
            line: vm.current_line,
        });
    }

    // Pre-flight memory check: PHP's zend_string_safe_alloc bails out with a
    // single Fatal error when the estimated allocation would exceed the
    // memory_limit. Mirror that here so we don't try to allocate multi-GB
    // Vecs (which would panic in Rust).
    let est_breaks = if width > 0 {
        bytes.len() / width as usize + 1
    } else {
        bytes.len() + 1
    };
    let alloc_size = bytes
        .len()
        .saturating_add(est_breaks.saturating_mul(brk_bytes.len()))
        .saturating_add(1);
    if goro_core::value::memory_get_limit() > 0 {
        vm.memory_check(alloc_size)?;
    }

    let bytes_len = bytes.len() as i64;
    let brk_len = brk_bytes.len() as i64;
    let mut laststart: i64 = 0;
    let mut lastspace: i64 = 0;

    // A single-character break without cutting: breaks replace spaces in
    // place (PHP_FUNCTION(wordwrap)'s special case).
    if brk_len == 1 && !cut_long {
        let mut out = bytes.to_vec();
        for current in 0..bytes_len {
            let ch = bytes[current as usize];
            if ch == brk_bytes[0] {
                laststart = current + 1;
                lastspace = current + 1;
            } else if ch == b' ' {
                if current - laststart >= width {
                    out[current as usize] = brk_bytes[0];
                    laststart = current + 1;
                }
                lastspace = current;
            } else if current - laststart >= width && laststart != lastspace {
                out[lastspace as usize] = brk_bytes[0];
                laststart = lastspace + 1;
            }
        }
        return Ok(Value::String(PhpString::from_vec(out)));
    }

    // Multiple character line break or forced cut.
    let cap = bytes
        .len()
        .saturating_add(est_breaks.saturating_mul(brk_bytes.len()));
    let mut result = Vec::with_capacity(cap);
    let mut current: i64 = 0;
    while current < bytes_len {
        let ch = bytes[current as usize];
        if ch == brk_bytes[0]
            && current + brk_len < bytes_len
            && &bytes[current as usize..(current + brk_len) as usize] == brk_bytes
        {
            // An existing break: copy through it.
            result.extend_from_slice(&bytes[laststart as usize..(current + brk_len) as usize]);
            current += brk_len - 1;
            laststart = current + 1;
            lastspace = current + 1;
        } else if ch == b' ' {
            if current - laststart >= width {
                result.extend_from_slice(&bytes[laststart as usize..current as usize]);
                result.extend_from_slice(brk_bytes);
                laststart = current + 1;
            }
            lastspace = current;
        } else if current - laststart >= width && cut_long && laststart >= lastspace {
            result.extend_from_slice(&bytes[laststart as usize..current as usize]);
            result.extend_from_slice(brk_bytes);
            laststart = current;
            lastspace = current;
        } else if current - laststart >= width && laststart < lastspace {
            result.extend_from_slice(&bytes[laststart as usize..lastspace as usize]);
            result.extend_from_slice(brk_bytes);
            laststart = lastspace + 1;
            lastspace = laststart;
        }
        current += 1;
    }
    // Copy over any stragglers.
    if laststart != current {
        result.extend_from_slice(&bytes[laststart as usize..]);
    }

    Ok(Value::String(PhpString::from_vec(result)))
}

fn strrpos(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let haystack = args.first().unwrap_or(&Value::Null).to_php_string();
    let needle = args.get(1).unwrap_or(&Value::Null).to_php_string();
    // TypeError: offset must be int (not float)
    if let Some(offset_v) = args.get(2) {
        if matches!(offset_v, Value::Double(_)) {
            let msg =
                "strrpos(): Argument #3 ($offset) must be of type int, float given".to_string();
            let exc = vm.create_exception(b"TypeError", &msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
    }
    let offset = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() {
        // Empty needle: return position based on offset
        if offset >= 0 {
            let start = offset as usize;
            if start > h.len() {
                let msg =
                    "strrpos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)";
                let exc = vm.create_exception(b"ValueError", msg, 0);
                vm.current_exception = Some(exc);
                return Err(VmError {
                    message: msg.into(),
                    line: vm.current_line,
                });
            }
            return Ok(Value::Long(h.len() as i64));
        } else {
            let abs = (-offset) as usize;
            if abs > h.len() {
                let msg =
                    "strrpos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)";
                let exc = vm.create_exception(b"ValueError", msg, 0);
                vm.current_exception = Some(exc);
                return Err(VmError {
                    message: msg.into(),
                    line: vm.current_line,
                });
            }
            return Ok(Value::Long((h.len() - abs) as i64));
        }
    }
    if offset >= 0 {
        let start = offset as usize;
        if start > h.len() {
            let msg =
                "strrpos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)";
            let exc = vm.create_exception(b"ValueError", msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg.into(),
                line: vm.current_line,
            });
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
        // Negative offset for strrpos: limit how far from the end the match
        // *start* may be. Match start must be <= (h.len() + offset).
        let abs = (-offset) as usize;
        if abs > h.len() {
            let msg =
                "strrpos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)";
            let exc = vm.create_exception(b"ValueError", msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg.into(),
                line: vm.current_line,
            });
        }
        if h.len() < n.len() {
            return Ok(Value::False);
        }
        let limit = h.len() - abs;
        // Find rightmost match whose start is <= limit.
        for (i, w) in h.windows(n.len()).enumerate().rev() {
            if i > limit {
                continue;
            }
            if w == n {
                return Ok(Value::Long(i as i64));
            }
        }
        Ok(Value::False)
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
            let msg =
                "stripos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)";
            let exc = vm.create_exception(b"ValueError", msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg.into(),
                line: vm.current_line,
            });
        }
        h.len() - abs
    } else {
        let o = offset_val as usize;
        if o > h.len() {
            let msg =
                "stripos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)";
            let exc = vm.create_exception(b"ValueError", msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg.into(),
                line: vm.current_line,
            });
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
    if let Some(offset_v) = args.get(2) {
        if matches!(offset_v, Value::Double(_)) {
            let msg =
                "strripos(): Argument #3 ($offset) must be of type int, float given".to_string();
            let exc = vm.create_exception(b"TypeError", &msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
    }
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
                return Err(VmError {
                    message: msg.into(),
                    line: vm.current_line,
                });
            }
            return Ok(Value::Long(h.len() as i64));
        } else {
            let abs = (-offset) as usize;
            if abs > h.len() {
                let msg = "strripos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)";
                let exc = vm.create_exception(b"ValueError", msg, 0);
                vm.current_exception = Some(exc);
                return Err(VmError {
                    message: msg.into(),
                    line: vm.current_line,
                });
            }
            return Ok(Value::Long((h.len() - abs) as i64));
        }
    }
    if offset >= 0 {
        let start = offset as usize;
        if start > h.len() {
            let msg =
                "strripos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)";
            let exc = vm.create_exception(b"ValueError", msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg.into(),
                line: vm.current_line,
            });
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
        // Negative offset for strripos: match start must be <= (h.len() + offset).
        let abs = (-offset) as usize;
        if abs > h.len() {
            let msg =
                "strripos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)";
            let exc = vm.create_exception(b"ValueError", msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg.into(),
                line: vm.current_line,
            });
        }
        if h.len() < n.len() {
            return Ok(Value::False);
        }
        let limit = h.len() - abs;
        for (i, w) in h.windows(n.len()).enumerate().rev() {
            if i > limit {
                continue;
            }
            if w == n.as_slice() {
                return Ok(Value::Long(i as i64));
            }
        }
        Ok(Value::False)
    }
}

/// zval_get_string(): "Array" with a warning for arrays, __toString() (or
/// an Error) for objects.
pub(crate) fn zval_get_string(vm: &mut Vm, v: &Value) -> Result<PhpString, VmError> {
    match v.deref() {
        Value::String(s) => Ok(s),
        Value::Array(_) => {
            vm.emit_warning("Array to string conversion");
            Ok(PhpString::from_bytes(b"Array"))
        }
        other => vm.value_to_string_checked(&other),
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

    // TypeError: When string is scalar but offset/length is array, error
    if !matches!(string_arg, Value::Array(_)) {
        if matches!(start_arg, Value::Array(_)) {
            let msg = "substr_replace(): Argument #3 ($offset) cannot be an array when working on a single string".to_string();
            let exc = _vm.create_exception(b"TypeError", &msg, 0);
            _vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: _vm.current_line,
            });
        }
        if let Some(la) = length_arg {
            if matches!(la, Value::Array(_)) {
                let msg = "substr_replace(): Argument #4 ($length) cannot be an array when working on a single string".to_string();
                let exc = _vm.create_exception(b"TypeError", &msg, 0);
                _vm.current_exception = Some(exc);
                return Err(VmError {
                    message: msg,
                    line: _vm.current_line,
                });
            }
        }
    }

    // If first argument is an array, process each element
    if let Value::Array(arr) = string_arg {
        let entries: Vec<(ArrayKey, Value)> = arr
            .borrow()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let repl_vals: Option<Vec<Value>> = match replacement_arg {
            Value::Array(a) => Some(a.borrow().values().cloned().collect()),
            _ => None,
        };
        let repl_scalar = match replacement_arg {
            Value::Array(_) => PhpString::empty(),
            v => zval_get_string(_vm, v)?,
        };
        let mut result = PhpArray::new();

        for (i, (key, val)) in entries.iter().enumerate() {
            let s = zval_get_string(_vm, val)?;

            // Get replacement for this element
            let repl = match &repl_vals {
                Some(rv) => match rv.get(i) {
                    Some(v) => zval_get_string(_vm, v)?,
                    None => PhpString::empty(),
                },
                None => repl_scalar.clone(),
            };

            // Get start for this element
            let start = if let Value::Array(start_arr) = start_arg {
                let start_arr = start_arr.borrow();
                start_arr
                    .iter()
                    .nth(i)
                    .map(|(_, v)| v.to_long())
                    .unwrap_or(0)
            } else {
                start_arg.to_long()
            };

            // Get length for this element
            let length = length_arg.and_then(|la| {
                if let Value::Array(len_arr) = la {
                    let len_arr = len_arr.borrow();
                    len_arr.iter().nth(i).map(|(_, v)| v.to_long())
                } else if matches!(la.deref(), Value::Null) {
                    None // (?int $length = null: to the end)
                } else {
                    Some(la.to_long())
                }
            });

            let replaced = substr_replace_single(s.as_bytes(), repl.as_bytes(), start, length);
            // zend_symtable_update(): numeric string keys become integers.
            let key = match key {
                ArrayKey::String(k) => Vm::value_to_array_key(Value::String(k.clone())),
                k => k.clone(),
            };
            result.set(key, Value::String(PhpString::from_vec(replaced)));
        }

        Ok(Value::Array(std::rc::Rc::new(std::cell::RefCell::new(
            result,
        ))))
    } else {
        let s = string_arg.to_php_string();
        // When replacement is array, use array[0] as the replacement
        let replacement = if let Value::Array(repl_arr) = replacement_arg {
            // zval_get_tmp_string() of its first element
            let first = repl_arr.borrow().values().next().cloned();
            match first {
                Some(v) => zval_get_string(_vm, &v)?,
                None => PhpString::empty(),
            }
        } else {
            replacement_arg.to_php_string()
        };
        // When start is array, use array[0]
        let start = if let Value::Array(start_arr) = start_arg {
            let start_arr = start_arr.borrow();
            start_arr
                .iter()
                .next()
                .map(|(_, v)| v.to_long())
                .unwrap_or(0)
        } else {
            start_arg.to_long()
        };
        // When length is array, use array[0]
        let length = length_arg.and_then(|la| {
            if let Value::Array(len_arr) = la {
                let len_arr = len_arr.borrow();
                len_arr.iter().next().map(|(_, v)| v.to_long())
            } else if matches!(la.deref(), Value::Null) {
                None // (?int $length = null: to the end)
            } else {
                Some(la.to_long())
            }
        });
        let result = substr_replace_single(s.as_bytes(), replacement.as_bytes(), start, length);
        Ok(Value::String(PhpString::from_vec(result)))
    }
}

fn hex2bin(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let hex = s.as_bytes();
    if !hex.len().is_multiple_of(2) {
        vm.emit_warning("hex2bin(): Hexadecimal input string must have an even length");
        return Ok(Value::False);
    }
    let mut result = Vec::with_capacity(hex.len() / 2);
    for chunk in hex.chunks(2) {
        let hi = match chunk[0] {
            b'0'..=b'9' => chunk[0] - b'0',
            b'a'..=b'f' => chunk[0] - b'a' + 10,
            b'A'..=b'F' => chunk[0] - b'A' + 10,
            _ => {
                vm.emit_warning("hex2bin(): Input string must be hexadecimal string");
                return Ok(Value::False);
            }
        };
        let lo = match chunk[1] {
            b'0'..=b'9' => chunk[1] - b'0',
            b'a'..=b'f' => chunk[1] - b'a' + 10,
            b'A'..=b'F' => chunk[1] - b'A' + 10,
            _ => {
                vm.emit_warning("hex2bin(): Input string must be hexadecimal string");
                return Ok(Value::False);
            }
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

/// zval_get_long() of a pack() argument: an out-of-range float warns.
/// zend_value_error() and friends format the message, then pass it on as a
/// C string: a NUL (from a `%c` of the format) ends it.
fn cut_at_nul(mut msg: String) -> String {
    if let Some(p) = msg.find('\0') {
        msg.truncate(p);
    }
    msg
}

/// zval_get_long() / zval_get_double() of an object without a cast
/// handler warns "could not be converted to int/float".
fn pack_object_warning(vm: &mut Vm, v: &Value, kind: u8, tname: &str) {
    if let Value::Object(obj) = v {
        if goro_core::value::native_cast(obj, kind).is_none() {
            let class_name = goro_core::value::display_class_name(&obj.borrow().class_name);
            vm.emit_warning(&format!(
                "Object of class {} could not be converted to {}",
                class_name, tname
            ));
        }
    }
}

/// zval_get_long() for php_pack()
fn pack_long(vm: &mut Vm, v: Option<&Value>) -> i64 {
    let Some(v) = v else { return 0 };
    let v = v.deref();
    pack_object_warning(vm, &v, 1, "int");
    let line = vm.current_line;
    vm.warn_oob_float_to_int(&v, line);
    v.to_long()
}

/// zval_get_double() for pack()
fn pack_double(vm: &mut Vm, v: Option<&Value>) -> f64 {
    let Some(v) = v else { return 0.0 };
    let v = v.deref();
    pack_object_warning(vm, &v, 2, "float");
    v.to_double()
}

fn pack_value_error(vm: &mut Vm, msg: String) -> VmError {
    let msg = cut_at_nul(msg);
    let exc = vm.create_exception(b"ValueError", &msg, 0);
    vm.current_exception = Some(exc);
    VmError {
        message: msg,
        line: vm.current_line,
    }
}

/// pack(): a port of ext/standard/pack.c.
/// zval_get_string() of a pack() argument: an array warns.
fn pack_arg_string(vm: &mut Vm, v: &Value) -> Result<goro_core::string::PhpString, VmError> {
    let v = v.deref();
    if matches!(v, Value::Array(_)) {
        let line = vm.current_line;
        vm.emit_warning_at("Array to string conversion", line);
    }
    vm.value_to_string_checked(&v)
}

fn pack_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let format = args.first().unwrap_or(&Value::Null).to_php_string();
    let format = format.as_bytes();
    let argv = args.get(1..).unwrap_or(&[]);
    let num_args = argv.len() as i64;
    let int_size = std::mem::size_of::<std::ffi::c_int>() as i64;

    // Preprocess the format into (code, arg) pairs, validating the codes and
    // argument counts; only the string arguments of '*' codes are converted
    // here (try_convert_to_string()), the rest when packing.
    let mut codes: Vec<(u8, i64)> = Vec::new();
    let mut strs: Vec<Option<PhpString>> = vec![None; argv.len()];
    let mut currentarg: i64 = 0;
    let mut i = 0usize;
    while i < format.len() {
        let code = format[i];
        i += 1;
        let mut arg: i64 = 1;
        if i < format.len() {
            let c = format[i];
            if c == b'*' {
                arg = -1;
                i += 1;
            } else if c.is_ascii_digit() {
                // atoi(): (int)strtol(), which saturates at LONG_MAX
                let mut n: i64 = 0;
                while i < format.len() && format[i].is_ascii_digit() {
                    n = n
                        .saturating_mul(10)
                        .saturating_add((format[i] - b'0') as i64);
                    i += 1;
                }
                arg = n as i32 as i64;
            }
        }
        match code {
            b'x' | b'X' | b'@' => {
                if arg < 0 {
                    let line = vm.current_line;
                    vm.emit_warning_at(
                        &format!(
                            "pack(): Type {}: '*' ignored",
                            goro_core::string::msg_char(code)
                        ),
                        line,
                    );
                    arg = 1;
                }
            }
            b'a' | b'A' | b'Z' | b'h' | b'H' => {
                if currentarg >= num_args {
                    return Err(pack_value_error(
                        vm,
                        format!(
                            "Type {}: not enough arguments",
                            goro_core::string::msg_char(code)
                        ),
                    ));
                }
                let idx = currentarg as usize;
                if arg < 0 {
                    let s = pack_arg_string(vm, &argv[idx])?;
                    arg = s.len() as i64;
                    if code == b'Z' {
                        arg += 1;
                    }
                    strs[idx] = Some(s);
                }
                currentarg += 1;
            }
            b'q' | b'Q' | b'J' | b'P' | b'c' | b'C' | b's' | b'S' | b'i' | b'I' | b'l' | b'L'
            | b'n' | b'N' | b'v' | b'V' | b'f' | b'g' | b'G' | b'd' | b'e' | b'E' => {
                if arg < 0 {
                    arg = num_args - currentarg;
                }
                if currentarg > i32::MAX as i64 - arg || currentarg + arg > num_args {
                    return Err(pack_value_error(
                        vm,
                        format!(
                            "Type {}: too few arguments",
                            goro_core::string::msg_char(code)
                        ),
                    ));
                }
                currentarg += arg;
            }
            _ => {
                return Err(pack_value_error(
                    vm,
                    format!(
                        "Type {}: unknown format code",
                        goro_core::string::msg_char(code)
                    ),
                ));
            }
        }
        codes.push((code, arg));
    }
    if currentarg < num_args {
        let line = vm.current_line;
        vm.emit_warning_at(
            &format!("pack(): {} arguments unused", num_args - currentarg),
            line,
        );
    }

    // Size the output (and emit the 'X' warnings) like the C sizing pass.
    let mut outputpos: i64 = 0;
    let mut outputsize: i64 = 0;
    for &(code, arg) in &codes {
        let mut cnt = arg;
        let unit = match code {
            b'h' | b'H' => {
                cnt = arg / 2 + arg % 2;
                1
            }
            b'a' | b'A' | b'Z' | b'c' | b'C' | b'x' => 1,
            b's' | b'S' | b'n' | b'v' => 2,
            b'i' | b'I' => int_size,
            b'l' | b'L' | b'N' | b'V' | b'f' | b'g' | b'G' => 4,
            b'q' | b'Q' | b'J' | b'P' | b'd' | b'e' | b'E' => 8,
            b'X' => {
                outputpos -= arg;
                if outputpos < 0 {
                    let line = vm.current_line;
                    vm.emit_warning_at(
                        &format!(
                            "pack(): Type {}: outside of string",
                            goro_core::string::msg_char(code)
                        ),
                        line,
                    );
                    outputpos = 0;
                }
                0
            }
            b'@' => {
                outputpos = arg;
                0
            }
            _ => 0,
        };
        if unit > 0 {
            if cnt < 0 || (i32::MAX as i64 - outputpos) / unit < cnt {
                return Err(pack_value_error(
                    vm,
                    format!(
                        "Type {}: integer overflow in format string",
                        goro_core::string::msg_char(code)
                    ),
                ));
            }
            outputpos += cnt * unit;
        }
        outputsize = outputsize.max(outputpos);
    }

    let mut out = vec![0u8; outputsize.max(0) as usize + 1];
    let mut pos: usize = 0;
    let mut currentarg = 0usize;
    for &(code, arg) in &codes {
        let n = arg.max(0) as usize;
        match code {
            b'a' | b'A' | b'Z' => {
                let arg_cp = if code != b'Z' { n } else { n.saturating_sub(1) };
                let s = match strs[currentarg].take() {
                    Some(s) => s,
                    None => pack_arg_string(vm, &argv[currentarg])?,
                };
                currentarg += 1;
                let fill = if code == b'A' { b' ' } else { 0 };
                out[pos..pos + n].fill(fill);
                let cp = s.len().min(arg_cp);
                out[pos..pos + cp].copy_from_slice(&s.as_bytes()[..cp]);
                pos += n;
            }
            b'h' | b'H' => {
                let mut nibbleshift = if code == b'h' { 0 } else { 4 };
                let mut first = true;
                let s = match strs[currentarg].take() {
                    Some(s) => s,
                    None => pack_arg_string(vm, &argv[currentarg])?,
                };
                currentarg += 1;
                let mut cnt = n;
                if cnt > s.len() {
                    let line = vm.current_line;
                    vm.emit_warning_at(
                        &format!(
                            "pack(): Type {}: not enough characters in string",
                            goro_core::string::msg_char(code)
                        ),
                        line,
                    );
                    cnt = s.len();
                }
                // `outputpos--` then pre-increment on each first nibble.
                let mut opos = pos as isize - 1;
                for &c in &s.as_bytes()[..cnt] {
                    let nib = match c {
                        b'0'..=b'9' => c - b'0',
                        b'A'..=b'F' => c - b'A' + 10,
                        b'a'..=b'f' => c - b'a' + 10,
                        _ => {
                            let line = vm.current_line;
                            vm.emit_warning_at(
                                &format!(
                                    "pack(): Type {}: illegal hex digit {}",
                                    goro_core::string::msg_char(code),
                                    goro_core::string::msg_char(c)
                                ),
                                line,
                            );
                            0
                        }
                    };
                    if first {
                        opos += 1;
                        out[opos as usize] = 0;
                        first = false;
                    } else {
                        first = true;
                    }
                    out[opos as usize] |= nib << nibbleshift;
                    nibbleshift = (nibbleshift + 4) & 7;
                }
                pos = (opos + 1) as usize;
            }
            b'c' | b'C' | b's' | b'S' | b'n' | b'v' | b'i' | b'I' | b'l' | b'L' | b'N' | b'V'
            | b'q' | b'Q' | b'J' | b'P' => {
                let (size, big) = match code {
                    b'c' | b'C' => (1, false),
                    b'n' => (2, true),
                    b's' | b'S' | b'v' => (2, false),
                    b'i' | b'I' => (int_size as usize, false),
                    b'N' => (4, true),
                    b'l' | b'L' | b'V' => (4, false),
                    b'J' => (8, true),
                    _ => (8, false),
                };
                for _ in 0..n {
                    let v = pack_long(vm, argv.get(currentarg)) as u64;
                    currentarg += 1;
                    let le = v.to_le_bytes();
                    for k in 0..size {
                        out[pos + k] = if big { le[size - 1 - k] } else { le[k] };
                    }
                    pos += size;
                }
            }
            b'f' | b'g' | b'G' => {
                for _ in 0..n {
                    let v = pack_double(vm, argv.get(currentarg)) as f32;
                    currentarg += 1;
                    let b = if code == b'G' {
                        v.to_be_bytes()
                    } else {
                        v.to_le_bytes()
                    };
                    out[pos..pos + 4].copy_from_slice(&b);
                    pos += 4;
                }
            }
            b'd' | b'e' | b'E' => {
                for _ in 0..n {
                    let v = pack_double(vm, argv.get(currentarg));
                    currentarg += 1;
                    let b = if code == b'E' {
                        v.to_be_bytes()
                    } else {
                        v.to_le_bytes()
                    };
                    out[pos..pos + 8].copy_from_slice(&b);
                    pos += 8;
                }
            }
            b'x' => {
                out[pos..pos + n].fill(0);
                pos += n;
            }
            b'X' => {
                pos = pos.saturating_sub(n);
            }
            b'@' => {
                if n > pos {
                    out[pos..n].fill(0);
                }
                pos = n;
            }
            _ => {}
        }
    }
    out.truncate(pos);
    Ok(Value::String(PhpString::from_vec(out)))
}

/// unpack(): a port of ext/standard/pack.c.
fn unpack_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    use goro_core::array::{ArrayKey, PhpArray};
    use std::cell::RefCell;
    use std::rc::Rc;

    let format = args.first().unwrap_or(&Value::Null).to_php_string();
    let data = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let raw_offset = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    if raw_offset < 0 || raw_offset as usize > data.len() {
        return Err(pack_value_error(
            vm,
            "unpack(): Argument #3 ($offset) must be contained in argument #2 ($data)".into(),
        ));
    }
    let input = &data.as_bytes()[raw_offset as usize..];
    let inputlen = input.len() as i64;
    let fmt = format.as_bytes();
    let int_size = std::mem::size_of::<std::ffi::c_int>() as i64;
    let mut result = PhpArray::new();
    let mut inputpos: i64 = 0;
    let mut f = 0usize;

    while f < fmt.len() {
        let typ = fmt[f];
        f += 1;
        let mut repetitions: i64 = 1;
        if f < fmt.len() {
            let c = fmt[f];
            if c.is_ascii_digit() {
                let mut n: i64 = 0;
                let mut overflow = false;
                while f < fmt.len() && fmt[f].is_ascii_digit() {
                    n = n.saturating_mul(10).saturating_add((fmt[f] - b'0') as i64);
                    if n > i32::MAX as i64 {
                        overflow = true;
                    }
                    f += 1;
                }
                if overflow {
                    let line = vm.current_line;
                    vm.emit_warning_at(
                        &format!(
                            "unpack(): Type {}: integer overflow",
                            goro_core::string::msg_char(typ)
                        ),
                        line,
                    );
                    return Ok(Value::False);
                }
                repetitions = n;
            } else if c == b'*' {
                repetitions = -1;
                f += 1;
            }
        }
        let name_start = f;
        let argb = repetitions;
        while f < fmt.len() && fmt[f] != b'/' {
            f += 1;
        }
        let name = &fmt[name_start..f.min(name_start + 200)];

        let mut size: i64 = match typ {
            b'X' => {
                if repetitions < 0 {
                    let line = vm.current_line;
                    vm.emit_warning_at(
                        &format!(
                            "unpack(): Type {}: '*' ignored",
                            goro_core::string::msg_char(typ)
                        ),
                        line,
                    );
                    repetitions = 1;
                }
                -1
            }
            b'@' => 0,
            b'a' | b'A' | b'Z' => {
                let s = repetitions;
                repetitions = 1;
                s
            }
            b'h' | b'H' => {
                let s = if repetitions > 0 {
                    (repetitions + 1) / 2
                } else {
                    repetitions
                };
                repetitions = 1;
                s
            }
            b'c' | b'C' | b'x' => 1,
            b's' | b'S' | b'n' | b'v' => 2,
            b'i' | b'I' => int_size,
            b'l' | b'L' | b'N' | b'V' | b'f' | b'g' | b'G' => 4,
            b'q' | b'Q' | b'J' | b'P' | b'd' | b'e' | b'E' => 8,
            _ => {
                return Err(pack_value_error(
                    vm,
                    format!("Invalid format type {}", goro_core::string::msg_char(typ)),
                ));
            }
        };

        let mut i: i64 = 0;
        while i != repetitions {
            if size != 0 && size != -1 && i32::MAX as i64 - size + 1 < inputpos {
                let line = vm.current_line;
                vm.emit_warning_at(
                    &format!(
                        "unpack(): Type {}: integer overflow",
                        goro_core::string::msg_char(typ)
                    ),
                    line,
                );
                return Ok(Value::False);
            }
            if inputpos + size <= inputlen {
                let key = if name.is_empty() {
                    ArrayKey::Int(i + 1)
                } else if repetitions == 1 {
                    Vm::value_to_array_key(Value::String(PhpString::from_vec(name.to_vec())))
                } else {
                    let mut k = name.to_vec();
                    k.extend_from_slice((i + 1).to_string().as_bytes());
                    Vm::value_to_array_key(Value::String(PhpString::from_vec(k)))
                };
                let p = inputpos as usize;
                let rd = |n: usize| -> [u8; 8] {
                    let mut b = [0u8; 8];
                    b[..n].copy_from_slice(&input[p..p + n]);
                    b
                };
                let val: Option<Value> = match typ {
                    b'a' | b'A' | b'Z' => {
                        let mut len = inputlen - inputpos;
                        if size >= 0 && len > size {
                            len = size;
                        }
                        size = len;
                        let mut s = &input[p..p + len as usize];
                        if typ == b'A' {
                            while let Some(&c) = s.last() {
                                if !matches!(c, 0 | b' ' | b'\t' | b'\r' | b'\n') {
                                    break;
                                }
                                s = &s[..s.len() - 1];
                            }
                        } else if typ == b'Z' {
                            if let Some(z) = s.iter().position(|&c| c == 0) {
                                s = &s[..z];
                            }
                        }
                        Some(Value::String(PhpString::from_vec(s.to_vec())))
                    }
                    b'h' | b'H' => {
                        let mut len = (inputlen - inputpos) * 2;
                        if size > i32::MAX as i64 / 2 {
                            return Err(pack_value_error(
                                vm,
                                format!(
                                    "unpack(): Argument #1 ($format) repeater must be less than or equal to {}",
                                    i32::MAX / 2
                                ),
                            ));
                        }
                        if size >= 0 && len > size * 2 {
                            len = size * 2;
                        }
                        if len > 0 && argb > 0 {
                            len -= argb % 2;
                        }
                        let mut nibbleshift = if typ == b'h' { 0 } else { 4 };
                        let mut first = true;
                        let mut ipos = 0usize;
                        let mut buf = Vec::with_capacity(len.max(0) as usize);
                        for _ in 0..len.max(0) {
                            let cc = (input[p + ipos] >> nibbleshift) & 0xf;
                            buf.push(if cc < 10 { cc + b'0' } else { cc - 10 + b'a' });
                            nibbleshift = (nibbleshift + 4) & 7;
                            if first {
                                first = false;
                            } else {
                                ipos += 1;
                                first = true;
                            }
                        }
                        Some(Value::String(PhpString::from_vec(buf)))
                    }
                    b'c' => Some(Value::Long(input[p] as i8 as i64)),
                    b'C' => Some(Value::Long(input[p] as i64)),
                    b's' => Some(Value::Long(
                        i16::from_le_bytes([input[p], input[p + 1]]) as i64
                    )),
                    b'S' | b'v' => Some(Value::Long(
                        u16::from_le_bytes([input[p], input[p + 1]]) as i64
                    )),
                    b'n' => Some(Value::Long(
                        u16::from_be_bytes([input[p], input[p + 1]]) as i64
                    )),
                    b'i' | b'l' => {
                        let b = rd(4);
                        Some(Value::Long(
                            i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as i64
                        ))
                    }
                    b'I' | b'L' | b'V' => {
                        let b = rd(4);
                        Some(Value::Long(
                            u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as i64
                        ))
                    }
                    b'N' => {
                        let b = rd(4);
                        Some(Value::Long(
                            u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as i64
                        ))
                    }
                    b'q' | b'Q' | b'P' => Some(Value::Long(i64::from_le_bytes(rd(8)))),
                    b'J' => Some(Value::Long(i64::from_be_bytes(rd(8)))),
                    b'f' | b'g' | b'G' => {
                        let b = rd(4);
                        let b = [b[0], b[1], b[2], b[3]];
                        let v = if typ == b'G' {
                            f32::from_be_bytes(b)
                        } else {
                            f32::from_le_bytes(b)
                        };
                        Some(Value::Double(v as f64))
                    }
                    b'd' | b'e' | b'E' => {
                        let b = rd(8);
                        let v = if typ == b'E' {
                            f64::from_be_bytes(b)
                        } else {
                            f64::from_le_bytes(b)
                        };
                        Some(Value::Double(v))
                    }
                    b'x' => None,
                    b'X' => {
                        if inputpos < size {
                            inputpos = -size;
                            i = repetitions - 1;
                            if repetitions >= 0 {
                                let line = vm.current_line;
                                vm.emit_warning_at(
                                    &format!(
                                        "unpack(): Type {}: outside of string",
                                        goro_core::string::msg_char(typ)
                                    ),
                                    line,
                                );
                            }
                        }
                        None
                    }
                    b'@' => {
                        if repetitions <= inputlen {
                            inputpos = repetitions;
                        } else {
                            let line = vm.current_line;
                            vm.emit_warning_at(
                                &format!(
                                    "unpack(): Type {}: outside of string",
                                    goro_core::string::msg_char(typ)
                                ),
                                line,
                            );
                        }
                        i = repetitions - 1;
                        None
                    }
                    _ => None,
                };
                if let Some(v) = val {
                    result.set(key, v);
                }
                inputpos += size;
                if inputpos < 0 {
                    if size != -1 {
                        let line = vm.current_line;
                        vm.emit_warning_at(
                            &format!(
                                "unpack(): Type {}: outside of string",
                                goro_core::string::msg_char(typ)
                            ),
                            line,
                        );
                    }
                    inputpos = 0;
                }
            } else if repetitions < 0 {
                break;
            } else {
                let avail = inputlen - inputpos;
                let line = vm.current_line;
                vm.emit_warning_at(
                    &format!(
                        "unpack(): Type {}: not enough input values, need {} values but only {} {} provided",
                        goro_core::string::msg_char(typ),
                        size,
                        avail,
                        if avail == 1 { "was" } else { "were" }
                    ),
                    line,
                );
                return Ok(Value::False);
            }
            i += 1;
        }
        if f < fmt.len() {
            f += 1; // skip the '/' separator
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

/// PHP_FUNCTION(str_decrement) (ext/standard/string.c).
fn str_decrement(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = s.as_bytes();
    let fail = |vm: &mut Vm, msg: String| -> Result<Value, VmError> {
        let msg = format!("str_decrement(): Argument #1 ($string) {}", msg);
        let exc = vm.create_exception(b"ValueError", &msg, 0);
        vm.current_exception = Some(exc);
        Err(VmError {
            message: msg,
            line: vm.current_line,
        })
    };
    if bytes.is_empty() {
        return fail(vm, "must not be empty".into());
    }
    if !bytes.iter().all(|c| c.is_ascii_alphanumeric()) {
        return fail(
            vm,
            "must be composed only of alphanumeric ASCII characters".into(),
        );
    }
    let range_err = |vm: &mut Vm| {
        fail(
            vm,
            format!("\"{}\" is out of decrement range", s.to_string_lossy()),
        )
    };
    if bytes[0] == b'0' {
        return range_err(vm);
    }
    let mut dec = bytes.to_vec();
    let mut position = dec.len() - 1;
    let mut carry;
    loop {
        let c = dec[position];
        if c != b'a' && c != b'A' && c != b'0' {
            carry = false;
            dec[position] -= 1;
        } else {
            carry = true;
            dec[position] = if c == b'0' { b'9' } else { c + 25 };
        }
        if !carry || position == 0 {
            break;
        }
        position -= 1;
    }
    if carry || (dec[0] == b'0' && dec.len() > 1) {
        if dec.len() == 1 {
            return range_err(vm);
        }
        dec.remove(0);
    }
    Ok(Value::String(PhpString::from_vec(dec)))
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
    vm.emit_function_deprecation("Function utf8_decode() is deprecated since 8.2, visit the php.net documentation for various alternatives")?;
    let args = &vm.zpp_self_check(b"utf8_decode", args)?[..];
    // UTF-8 to ISO-8859-1: '?' for an illegal sequence or a code point
    // above U+00FF.
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = s.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    let mut pos = 0;
    while pos < bytes.len() {
        match crate::html::php_next_utf8_char(bytes, &mut pos) {
            Some(c) if c <= 0xFF => result.push(c as u8),
            _ => result.push(b'?'),
        }
    }
    Ok(Value::String(PhpString::from_vec(result)))
}

fn utf8_encode_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    vm.emit_function_deprecation("Function utf8_encode() is deprecated since 8.2, visit the php.net documentation for various alternatives")?;
    let args = &vm.zpp_self_check(b"utf8_encode", args)?[..];
    // Convert ISO-8859-1 to UTF-8
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = s.as_bytes();
    let mut result = String::new();
    for &b in bytes {
        result.push(b as char);
    }
    Ok(Value::String(PhpString::from_string(result)))
}

fn strip_tags_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // PHP 8 declares the second arg as `array|string|null` but standard
    // scalar→string coercion still applies in non-strict mode: bool/int/float
    // are accepted, while resource and object-without-__toString are not.
    if let Some(v) = args.get(1) {
        let d = v.deref();
        let allowed = match &d {
            Value::String(_)
            | Value::Array(_)
            | Value::Null
            | Value::Long(_)
            | Value::Double(_)
            | Value::True
            | Value::False => true,
            Value::Object(o) => {
                let class_lower: Vec<u8> = o
                    .borrow()
                    .class_name
                    .iter()
                    .map(|b| b.to_ascii_lowercase())
                    .collect();
                _vm.classes
                    .get(&class_lower)
                    .map(|c| c.methods.contains_key(&b"__tostring".to_vec()))
                    .unwrap_or(false)
            }
            _ => false,
        };
        if !allowed {
            let type_name = Vm::value_type_name(&d);
            let msg = format!(
                "strip_tags(): Argument #2 ($allowed_tags) must be of type array|string|null, {} given",
                type_name
            );
            let exc = _vm.create_exception(b"TypeError", &msg, 0);
            _vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: _vm.current_line,
            });
        }
    }

    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    // The allow string: given as is, or "<tag>" per element of the array
    // form (an empty array means none).
    let allow: Option<Vec<u8>> = match args.get(1).map(|v| v.deref()) {
        Some(Value::Array(arr)) => {
            let values: Vec<Value> = arr.borrow().values().cloned().collect();
            let mut buf = Vec::new();
            for v in values {
                // zval_get_string()
                let v = v.deref();
                if matches!(v, Value::Array(_)) {
                    _vm.emit_warning("Array to string conversion");
                }
                let tag = _vm.value_to_string_checked(&v)?;
                buf.push(b'<');
                buf.extend_from_slice(tag.as_bytes());
                buf.push(b'>');
            }
            (!buf.is_empty()).then_some(buf)
        }
        Some(Value::Null) | None => None,
        Some(other) => Some(_vm.value_to_string(&other).as_bytes().to_vec()),
    };
    let result = php_strip_tags_ex(s.as_bytes(), allow.as_deref(), false);
    Ok(Value::String(PhpString::from_vec(result)))
}

/// C isspace() (C locale)
fn c_isspace(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C)
}

/// php_tag_find(): whether the tag in `tag` ("<...>"), normalized to
/// "<name>", occurs in the (lowercased) allow string.
fn php_tag_find(tag: &[u8], set: &[u8]) -> bool {
    let mut norm = Vec::with_capacity(tag.len() + 1);
    let mut state = 0;
    let mut t = 0;
    while t < tag.len() {
        let c = tag[t].to_ascii_lowercase();
        match c {
            b'<' => norm.push(c),
            b'>' => break,
            _ => {
                if !c_isspace(c) {
                    state = 1;
                    let prev = if t > 0 { tag[t - 1] } else { 0 };
                    let next = tag.get(t + 1).copied().unwrap_or(0);
                    if c != b'/' || (prev != b'<' && next != b'>') {
                        norm.push(c);
                    }
                } else if state == 1 {
                    break;
                }
            }
        }
        t += 1;
    }
    norm.push(b'>');
    // strstr(): the allow string ends at its first NUL
    let set = &set[..set.iter().position(|&b| b == 0).unwrap_or(set.len())];
    set.windows(norm.len()).any(|w| w == norm.as_slice())
}

/// php_strip_tags_ex(): the state machine stripping HTML and PHP tags
/// (0: output, 1: inside a tag, 2: inside a PHP tag, 3: inside a `<!`
/// construct, 4: inside a comment). Tags named in `allow` are kept.
fn php_strip_tags_ex(buf: &[u8], allow: Option<&[u8]>, allow_tag_spaces: bool) -> Vec<u8> {
    let allow = allow.map(|a| a.to_ascii_lowercase());
    // (the C buffer is NUL-terminated)
    let at = |i: usize| buf.get(i).copied().unwrap_or(0);
    let mut out = Vec::with_capacity(buf.len());
    let mut tbuf: Vec<u8> = Vec::new();
    let (mut lc, mut br, mut depth, mut in_q) = (0u8, 0i64, 0i64, 0u8);
    let mut state = 0u8;
    let mut is_xml = false;
    let toggle_q = |in_q: &mut u8, c: u8| *in_q = if *in_q != 0 { 0 } else { c };
    let mut p = 0;
    while p < buf.len() {
        let c = buf[p];
        match state {
            0 => match c {
                0 => {}
                b'<' => {
                    if in_q != 0 {
                    } else if c_isspace(at(p + 1)) && !allow_tag_spaces {
                        out.push(c);
                    } else {
                        lc = b'<';
                        state = 1;
                        if allow.is_some() {
                            tbuf.push(b'<');
                        }
                    }
                }
                b'>' => {
                    if depth > 0 {
                        depth -= 1;
                    } else if in_q == 0 {
                        out.push(c);
                    }
                }
                _ => out.push(c),
            },
            1 => {
                let mut reg_char = false;
                match c {
                    0 => {}
                    b'<' => {
                        if in_q != 0 {
                        } else if c_isspace(at(p + 1)) && !allow_tag_spaces {
                            reg_char = true;
                        } else {
                            depth += 1;
                        }
                    }
                    b'>' => {
                        if depth > 0 {
                            depth -= 1;
                        } else if in_q == 0 {
                            lc = b'>';
                            if !(is_xml && p >= 1 && buf[p - 1] == b'-') {
                                in_q = 0;
                                state = 0;
                                is_xml = false;
                                if let Some(set) = &allow {
                                    tbuf.push(b'>');
                                    if php_tag_find(&tbuf, set) {
                                        out.extend_from_slice(&tbuf);
                                    }
                                    tbuf.clear();
                                }
                            }
                        }
                    }
                    b'"' | b'\'' => {
                        if p != 0 && (in_q == 0 || c == in_q) {
                            toggle_q(&mut in_q, c);
                        }
                        reg_char = true;
                    }
                    b'!' => {
                        // JavaScript & other HTML scripting languages
                        if p >= 1 && buf[p - 1] == b'<' {
                            state = 3;
                            lc = c;
                        } else {
                            reg_char = true;
                        }
                    }
                    b'?' => {
                        if p >= 1 && buf[p - 1] == b'<' {
                            br = 0;
                            state = 2;
                        } else {
                            reg_char = true;
                        }
                    }
                    _ => reg_char = true,
                }
                if reg_char && allow.is_some() {
                    tbuf.push(c);
                }
            }
            2 => match c {
                b'(' => {
                    if lc != b'"' && lc != b'\'' {
                        lc = b'(';
                        br += 1;
                    }
                }
                b')' => {
                    if lc != b'"' && lc != b'\'' {
                        lc = b')';
                        br -= 1;
                    }
                }
                b'>' => {
                    if depth > 0 {
                        depth -= 1;
                    } else if in_q == 0 && br == 0 && p >= 1 && lc != b'"' && buf[p - 1] == b'?' {
                        in_q = 0;
                        state = 0;
                        tbuf.clear();
                    }
                }
                b'"' | b'\'' => {
                    if p >= 1 && buf[p - 1] != b'\\' {
                        if lc == c {
                            lc = 0;
                        } else if lc != b'\\' {
                            lc = c;
                        }
                        if in_q == 0 || c == in_q {
                            toggle_q(&mut in_q, c);
                        }
                    }
                }
                b'l' | b'L' => {
                    // `<?xml` is not PHP: back to HTML
                    if p > 4
                        && matches!(buf[p - 1], b'm' | b'M')
                        && matches!(buf[p - 2], b'x' | b'X')
                        && buf[p - 3] == b'?'
                        && buf[p - 4] == b'<'
                    {
                        state = 1;
                        is_xml = true;
                    }
                }
                _ => {}
            },
            3 => match c {
                b'>' => {
                    if depth > 0 {
                        depth -= 1;
                    } else if in_q == 0 {
                        in_q = 0;
                        state = 0;
                        tbuf.clear();
                    }
                }
                b'"' | b'\'' => {
                    if p != 0 && buf[p - 1] != b'\\' && (in_q == 0 || c == in_q) {
                        toggle_q(&mut in_q, c);
                    }
                }
                b'-' => {
                    if p >= 2 && buf[p - 1] == b'-' && buf[p - 2] == b'!' {
                        state = 4;
                    }
                }
                b'E' | b'e' => {
                    // !DOCTYPE exception
                    if p > 6 && buf[p - 6..p].eq_ignore_ascii_case(b"doctyp") {
                        state = 1;
                    }
                }
                _ => {}
            },
            _ => {
                if c == b'>' && in_q == 0 && p >= 2 && buf[p - 1] == b'-' && buf[p - 2] == b'-' {
                    in_q = 0;
                    state = 0;
                    tbuf.clear();
                }
            }
        }
        p += 1;
    }
    out
}

fn nl2br_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let is_xhtml = args.get(1).map(|v| v.is_truthy()).unwrap_or(true);
    let br = if is_xhtml {
        b"<br />" as &[u8]
    } else {
        b"<br>" as &[u8]
    };
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

fn str_word_count_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Mirrors PHP's str_word_count. A "word" character is any ASCII letter,
    // an apostrophe (`'`), a hyphen (`-`), or any byte listed in $characters.
    // PHP's range syntax `a..z` in $characters expands to a..b..c..z. The
    // leading `'`/`-` is dropped (unless $characters explicitly contains it),
    // and a trailing `-` is dropped (again unless allowed by $characters).
    // Trailing `'` is kept. Format must be 0/1/2 — anything else is a
    // ValueError.
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let format = args.get(1).map(|v| v.to_long()).unwrap_or(0);

    if format != 0 && format != 1 && format != 2 {
        let msg =
            "str_word_count(): Argument #2 ($format) must be a valid format value".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }

    // Build the 256-byte char mask from optional $characters (with `..` ranges).
    let mut mask = [false; 256];
    let mut has_char_list = false;
    if let Some(cl) = args.get(2) {
        if !matches!(cl.deref(), Value::Null) {
            has_char_list = true;
            let cl_str = cl.to_php_string();
            for c in expand_charlist_warn(cl_str.as_bytes(), vm, "str_word_count") {
                mask[c as usize] = true;
            }
        }
    }

    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return Ok(match format {
            0 => Value::Long(0),
            _ => Value::Array(Rc::new(RefCell::new(PhpArray::new()))),
        });
    }

    let is_word_char = |c: u8| -> bool {
        c.is_ascii_alphabetic() || c == b'\'' || c == b'-' || (has_char_list && mask[c as usize])
    };

    // PHP: skip leading `'` or `-` unless allowed; skip trailing `-` only.
    let mut start = 0usize;
    let mut end = bytes.len();
    let first = bytes[0];
    if (first == b'\'' && !(has_char_list && mask[b'\'' as usize]))
        || (first == b'-' && !(has_char_list && mask[b'-' as usize]))
    {
        start = 1;
    }
    let last = bytes[end - 1];
    if last == b'-' && !(has_char_list && mask[b'-' as usize]) {
        end -= 1;
    }

    let mut words: Vec<(usize, Vec<u8>)> = Vec::new();
    let mut p = start;
    while p < end {
        let s_pos = p;
        while p < end && is_word_char(bytes[p]) {
            p += 1;
        }
        if p > s_pos {
            words.push((s_pos, bytes[s_pos..p].to_vec()));
        }
        p += 1;
    }

    match format {
        0 => Ok(Value::Long(words.len() as i64)),
        1 => {
            let mut arr = PhpArray::new();
            for (_, word) in &words {
                arr.push(Value::String(PhpString::from_vec(word.clone())));
            }
            Ok(Value::Array(Rc::new(RefCell::new(arr))))
        }
        2 => {
            let mut arr = PhpArray::new();
            for (pos, word) in &words {
                arr.set(
                    ArrayKey::Int(*pos as i64),
                    Value::String(PhpString::from_vec(word.clone())),
                );
            }
            Ok(Value::Array(Rc::new(RefCell::new(arr))))
        }
        _ => unreachable!(),
    }
}

fn convert_uuencode_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    convert_uuencode(vm, args)
}

fn convert_uudecode_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    convert_uudecode(vm, args)
}

fn quoted_printable_encode_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = zpp_str(vm, args, 0, "quoted_printable_encode", "string")?;
    Ok(Value::String(PhpString::from_vec(php_quot_print_encode(
        s.as_bytes(),
    ))))
}

/// PHP_FUNCTION(quoted_printable_decode): walks the string as a C string
/// (up to its first NUL); "=XX" decodes, "=" + spaces/tabs + CR|LF|CRLF or
/// end of string is a soft line break, any other "=" is kept.
fn quoted_printable_decode_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = s.as_bytes();
    let at = |i: usize| bytes.get(i).copied().unwrap_or(0);
    let mut result = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while at(i) != 0 {
        if at(i) != b'=' {
            result.push(at(i));
            i += 1;
            continue;
        }
        if let (Some(h), Some(l)) = (hex_nibble(at(i + 1)), hex_nibble(at(i + 2))) {
            result.push((h << 4) | l);
            i += 3;
            continue;
        }
        let mut k = 1;
        while matches!(at(i + k), b' ' | b'\t') {
            k += 1;
        }
        match (at(i + k), at(i + k + 1)) {
            (0, _) => i += k,
            (b'\r', b'\n') => i += k + 2,
            (b'\r' | b'\n', _) => i += k + 1,
            _ => {
                result.push(b'=');
                i += 1;
            }
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
