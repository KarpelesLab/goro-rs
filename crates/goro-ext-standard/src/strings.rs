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
}

fn strlen(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    Ok(Value::Long(s.len() as i64))
}

fn strtolower(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let lower: Vec<u8> = s.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
    Ok(Value::String(PhpString::from_vec(lower)))
}

fn strtoupper(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let upper: Vec<u8> = s.as_bytes().iter().map(|b| b.to_ascii_uppercase()).collect();
    Ok(Value::String(PhpString::from_vec(upper)))
}

fn trim(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = s.as_bytes();
    let chars = args
        .get(1)
        .map(|v| v.to_php_string())
        .map(|s| s.as_bytes().to_vec())
        .unwrap_or_else(|| b" \t\n\r\0\x0B".to_vec());

    let start = bytes.iter().position(|b| !chars.contains(b)).unwrap_or(bytes.len());
    let end = bytes.iter().rposition(|b| !chars.contains(b)).map(|i| i + 1).unwrap_or(start);
    Ok(Value::String(PhpString::from_vec(bytes[start..end].to_vec())))
}

fn ltrim(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = s.as_bytes();
    let chars = args
        .get(1)
        .map(|v| v.to_php_string())
        .map(|s| s.as_bytes().to_vec())
        .unwrap_or_else(|| b" \t\n\r\0\x0B".to_vec());
    let start = bytes.iter().position(|b| !chars.contains(b)).unwrap_or(bytes.len());
    Ok(Value::String(PhpString::from_vec(bytes[start..].to_vec())))
}

fn rtrim(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = s.as_bytes();
    let chars = args
        .get(1)
        .map(|v| v.to_php_string())
        .map(|s| s.as_bytes().to_vec())
        .unwrap_or_else(|| b" \t\n\r\0\x0B".to_vec());
    let end = bytes.iter().rposition(|b| !chars.contains(b)).map(|i| i + 1).unwrap_or(0);
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

    let length = args.get(2).map(|v| v.to_long());
    let end = match length {
        Some(l) if l < 0 => ((len + l) as usize).max(start),
        Some(l) => (start + l as usize).min(bytes.len()),
        None => bytes.len(),
    };

    if start >= bytes.len() || start >= end {
        return Ok(Value::String(PhpString::empty()));
    }

    Ok(Value::String(PhpString::from_vec(bytes[start..end].to_vec())))
}

fn strpos(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let haystack = args.first().unwrap_or(&Value::Null).to_php_string();
    let needle = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let offset = args.get(2).map(|v| v.to_long()).unwrap_or(0) as usize;

    let h = haystack.as_bytes();
    let n = needle.as_bytes();

    if n.is_empty() || offset >= h.len() {
        return Ok(Value::False);
    }

    for i in offset..=(h.len().saturating_sub(n.len())) {
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

    for i in 0..=(h.len().saturating_sub(n.len())) {
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

fn str_repeat(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let times = args.get(1).map(|v| v.to_long()).unwrap_or(0).max(0) as usize;
    let repeated = s.as_bytes().repeat(times);
    Ok(Value::String(PhpString::from_vec(repeated)))
}

fn str_replace(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let search = args.first().unwrap_or(&Value::Null).to_php_string();
    let replace = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let subject = args.get(2).unwrap_or(&Value::Null).to_php_string();

    let s = subject.as_bytes();
    let find = search.as_bytes();
    let rep = replace.as_bytes();

    if find.is_empty() {
        return Ok(Value::String(subject));
    }

    let mut result = Vec::new();
    let mut i = 0;
    while i < s.len() {
        if i + find.len() <= s.len() && &s[i..i + find.len()] == find {
            result.extend_from_slice(rep);
            i += find.len();
        } else {
            result.push(s[i]);
            i += 1;
        }
    }
    Ok(Value::String(PhpString::from_vec(result)))
}

fn explode(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    use goro_core::array::PhpArray;
    use std::cell::RefCell;
    use std::rc::Rc;

    let delimiter = args.first().unwrap_or(&Value::Null).to_php_string();
    let string = args.get(1).unwrap_or(&Value::Null).to_php_string();

    let d = delimiter.as_bytes();
    let s = string.as_bytes();

    if d.is_empty() {
        return Err(VmError {
            message: "explode(): Argument #1 ($separator) must not be empty".into(),
            line: 0,
        });
    }

    let mut arr = PhpArray::new();
    let mut start = 0;
    let mut i = 0;
    while i + d.len() <= s.len() {
        if &s[i..i + d.len()] == d {
            arr.push(Value::String(PhpString::from_vec(s[start..i].to_vec())));
            i += d.len();
            start = i;
        } else {
            i += 1;
        }
    }
    arr.push(Value::String(PhpString::from_vec(s[start..].to_vec())));

    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

fn implode(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let (glue, pieces) = if args.len() >= 2 {
        (
            args[0].to_php_string(),
            &args[1],
        )
    } else {
        (PhpString::empty(), args.first().unwrap_or(&Value::Null))
    };

    if let Value::Array(arr) = pieces {
        let arr = arr.borrow();
        let parts: Vec<Vec<u8>> = arr
            .values()
            .map(|v| v.to_php_string().as_bytes().to_vec())
            .collect();
        let mut result = Vec::new();
        for (i, part) in parts.iter().enumerate() {
            if i > 0 {
                result.extend_from_slice(glue.as_bytes());
            }
            result.extend_from_slice(part);
        }
        Ok(Value::String(PhpString::from_vec(result)))
    } else {
        Ok(Value::String(PhpString::empty()))
    }
}

fn chr(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let code = args.first().map(|v| v.to_long()).unwrap_or(0) as u8;
    Ok(Value::String(PhpString::from_bytes(&[code])))
}

fn ord(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let code = s.as_bytes().first().copied().unwrap_or(0);
    Ok(Value::Long(code as i64))
}

fn sprintf(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        return Err(VmError {
            message: "sprintf() expects at least 1 argument".into(),
            line: 0,
        });
    }
    let result = do_sprintf(args);
    Ok(Value::String(PhpString::from_vec(result.into_bytes())))
}

/// Shared sprintf implementation used by both sprintf() and printf()
pub fn do_sprintf(args: &[Value]) -> String {
    if args.is_empty() { return String::new(); }
    let format = args[0].to_php_string();
    let format_bytes = format.as_bytes();

    let mut result = String::new();
    let mut arg_idx = 1;
    let mut i = 0;

    while i < format_bytes.len() {
        if format_bytes[i] == b'%' {
            i += 1;
            if i >= format_bytes.len() { break; }
            if format_bytes[i] == b'%' {
                result.push('%');
                i += 1;
                continue;
            }

            // Parse format specifier: %[flags][width][.precision]type
            // Flags: -, +, space, 0, '
            let mut pad_char = b' ';
            let mut left_align = false;
            let mut show_sign = false;
            let mut pad_zero = false;

            // Flags
            loop {
                if i >= format_bytes.len() { break; }
                match format_bytes[i] {
                    b'-' => { left_align = true; i += 1; }
                    b'+' => { show_sign = true; i += 1; }
                    b'0' => { pad_zero = true; pad_char = b'0'; i += 1; }
                    b' ' => { i += 1; } // space flag
                    b'\'' => {
                        i += 1;
                        if i < format_bytes.len() { pad_char = format_bytes[i]; i += 1; }
                    }
                    _ => break,
                }
            }

            // Width
            let mut width: usize = 0;
            while i < format_bytes.len() && format_bytes[i].is_ascii_digit() {
                width = width * 10 + (format_bytes[i] - b'0') as usize;
                i += 1;
            }

            // Precision
            let mut precision: Option<usize> = None;
            if i < format_bytes.len() && format_bytes[i] == b'.' {
                i += 1;
                let mut prec = 0;
                while i < format_bytes.len() && format_bytes[i].is_ascii_digit() {
                    prec = prec * 10 + (format_bytes[i] - b'0') as usize;
                    i += 1;
                }
                precision = Some(prec);
            }

            if i >= format_bytes.len() { break; }
            let spec = format_bytes[i];
            i += 1;

            let arg = args.get(arg_idx).unwrap_or(&Value::Null);
            arg_idx += 1;

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
                    if show_sign && n >= 0 { format!("+{}", n) } else { n.to_string() }
                }
                b'f' | b'F' => {
                    let f = arg.to_double();
                    let prec = precision.unwrap_or(6);
                    if show_sign && f >= 0.0 { format!("+{:.prec$}", f) } else { format!("{:.prec$}", f) }
                }
                b'e' => {
                    let f = arg.to_double();
                    let prec = precision.unwrap_or(6);
                    format!("{:.prec$e}", f)
                }
                b'E' => {
                    let f = arg.to_double();
                    let prec = precision.unwrap_or(6);
                    format!("{:.prec$E}", f)
                }
                b'g' | b'G' => {
                    let f = arg.to_double();
                    let prec = precision.unwrap_or(6);
                    // Use shorter of %e and %f
                    let ef = format!("{:.prec$e}", f);
                    let ff = format!("{:.prec$}", f);
                    if ef.len() < ff.len() { ef } else { ff }
                }
                b'x' => format!("{:x}", arg.to_long()),
                b'X' => format!("{:X}", arg.to_long()),
                b'o' => format!("{:o}", arg.to_long()),
                b'b' => format!("{:b}", arg.to_long()),
                b'c' => String::from(arg.to_long() as u8 as char),
                b'u' => format!("{}", arg.to_long() as u64),
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
                    for _ in 0..padding { result.push(pad_char as char); }
                } else {
                    // For zero-padding with sign, put sign before zeros
                    if pad_zero && (formatted.starts_with('-') || formatted.starts_with('+')) {
                        result.push(formatted.chars().next().unwrap());
                        for _ in 0..padding { result.push('0'); }
                        result.push_str(&formatted[1..]);
                    } else {
                        for _ in 0..padding { result.push(pad_char as char); }
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

fn nl2br(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = s.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    for &b in bytes {
        if b == b'\n' {
            result.extend_from_slice(b"<br />\n");
        } else {
            result.push(b);
        }
    }
    Ok(Value::String(PhpString::from_vec(result)))
}

fn chunk_split(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let body = args.first().unwrap_or(&Value::Null).to_php_string();
    let chunklen = args.get(1).map(|v| v.to_long()).unwrap_or(76) as usize;
    let end = args.get(2).map(|v| v.to_php_string()).unwrap_or_else(|| PhpString::from_bytes(b"\r\n"));

    let bytes = body.as_bytes();
    let mut result = Vec::new();
    for chunk in bytes.chunks(chunklen) {
        result.extend_from_slice(chunk);
        result.extend_from_slice(end.as_bytes());
    }
    Ok(Value::String(PhpString::from_vec(result)))
}

fn str_pad(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let input = args.first().unwrap_or(&Value::Null).to_php_string();
    let length = args.get(1).map(|v| v.to_long()).unwrap_or(0) as usize;
    let pad_string = args.get(2).map(|v| v.to_php_string()).unwrap_or_else(|| PhpString::from_bytes(b" "));
    let pad_type = args.get(3).map(|v| v.to_long()).unwrap_or(1); // STR_PAD_RIGHT=1

    let bytes = input.as_bytes();
    if bytes.len() >= length {
        return Ok(Value::String(input));
    }

    let pad_needed = length - bytes.len();
    let pad_bytes = pad_string.as_bytes();
    if pad_bytes.is_empty() {
        return Ok(Value::String(input));
    }

    let mut padding = Vec::new();
    while padding.len() < pad_needed {
        padding.push(pad_bytes[padding.len() % pad_bytes.len()]);
    }

    let mut result = Vec::with_capacity(length);
    match pad_type {
        0 => {
            // STR_PAD_RIGHT (actually RIGHT is 1 in PHP, but let's handle both)
            result.extend_from_slice(bytes);
            result.extend_from_slice(&padding);
        }
        2 => {
            // STR_PAD_BOTH
            let left = pad_needed / 2;
            result.extend_from_slice(&padding[..left]);
            result.extend_from_slice(bytes);
            result.extend_from_slice(&padding[..pad_needed - left]);
        }
        _ => {
            // STR_PAD_RIGHT (default)
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
    let mut bytes = s.as_bytes().to_vec();
    let mut capitalize_next = true;
    for b in &mut bytes {
        if *b == b' ' || *b == b'\t' || *b == b'\r' || *b == b'\n' || *b == b'\x0B' || *b == b'\x0C' {
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
            b'\'' | b'"' | b'\\' => { result.push(b'\\'); result.push(b); }
            0 => { result.push(b'\\'); result.push(b'0'); }
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
                b'0' => { result.push(0); i += 2; }
                ch => { result.push(ch); i += 2; }
            }
        } else {
            result.push(bytes[i]);
            i += 1;
        }
    }
    Ok(Value::String(PhpString::from_vec(result)))
}

fn addcslashes(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let charlist = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let chars = charlist.as_bytes();
    let mut result = Vec::new();
    for &b in s.as_bytes() {
        if chars.contains(&b) {
            match b {
                b'\n' => result.extend_from_slice(b"\\n"),
                b'\r' => result.extend_from_slice(b"\\r"),
                b'\t' => result.extend_from_slice(b"\\t"),
                0 => result.extend_from_slice(b"\\000"),
                _ => { result.push(b'\\'); result.push(b); }
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
                b'n' => { result.push(b'\n'); i += 2; }
                b'r' => { result.push(b'\r'); i += 2; }
                b't' => { result.push(b'\t'); i += 2; }
                b'v' => { result.push(0x0B); i += 2; }
                b'a' => { result.push(0x07); i += 2; }
                b'f' => { result.push(0x0C); i += 2; }
                b'\\' => { result.push(b'\\'); i += 2; }
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
                ch => { result.push(ch); i += 2; }
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
    let result: Vec<u8> = s.as_bytes().iter().map(|&b| {
        match b {
            b'a'..=b'm' | b'A'..=b'M' => b + 13,
            b'n'..=b'z' | b'N'..=b'Z' => b - 13,
            _ => b,
        }
    }).collect();
    Ok(Value::String(PhpString::from_vec(result)))
}

fn strip_tags(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = s.as_bytes();
    let mut result = Vec::new();
    let mut in_tag = false;
    for &b in bytes {
        match b {
            b'<' => in_tag = true,
            b'>' => in_tag = false,
            _ if !in_tag => result.push(b),
            _ => {}
        }
    }
    Ok(Value::String(PhpString::from_vec(result)))
}

fn quoted_printable_encode(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let mut result = Vec::new();
    for &b in s.as_bytes() {
        if (b >= 33 && b <= 126 && b != b'=') || b == b'\t' || b == b' ' {
            result.push(b);
        } else {
            result.push(b'=');
            result.extend_from_slice(format!("{:02X}", b).as_bytes());
        }
    }
    Ok(Value::String(PhpString::from_vec(result)))
}

fn quoted_printable_decode(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = s.as_bytes();
    let mut result = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'=' && i + 2 < bytes.len() && bytes[i+1].is_ascii_hexdigit() && bytes[i+2].is_ascii_hexdigit() {
            let hex: String = [bytes[i+1] as char, bytes[i+2] as char].iter().collect();
            result.push(u8::from_str_radix(&hex, 16).unwrap_or(0));
            i += 3;
        } else {
            result.push(bytes[i]);
            i += 1;
        }
    }
    Ok(Value::String(PhpString::from_vec(result)))
}

fn convert_uuencode(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(PhpString::empty()))
}

fn convert_uudecode(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(PhpString::empty()))
}

fn str_getcsv(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    use goro_core::array::PhpArray;
    use std::cell::RefCell;
    use std::rc::Rc;

    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let sep = args.get(1).map(|v| v.to_php_string()).unwrap_or_else(|| PhpString::from_bytes(b","));
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
