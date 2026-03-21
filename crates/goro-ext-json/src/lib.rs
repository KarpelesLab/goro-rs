use goro_core::array::PhpArray;
use goro_core::object::PhpObject;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use std::cell::RefCell;
use std::rc::Rc;

/// Register all JSON extension functions
pub fn register(vm: &mut Vm) {
    vm.register_function(b"json_encode", json_encode);
    vm.register_function(b"json_decode", json_decode);
    vm.register_function(b"json_last_error", json_last_error);
    vm.register_function(b"json_last_error_msg", json_last_error_msg);
    vm.register_function(b"json_validate", json_validate);
}

// JSON encode option flags
const JSON_HEX_TAG: i64 = 1;
const JSON_HEX_AMP: i64 = 2;
const JSON_HEX_APOS: i64 = 4;
const JSON_HEX_QUOT: i64 = 8;
const JSON_FORCE_OBJECT: i64 = 16;
const JSON_NUMERIC_CHECK: i64 = 32;
const JSON_UNESCAPED_SLASHES: i64 = 64;
const JSON_PRETTY_PRINT: i64 = 128;
const JSON_UNESCAPED_UNICODE: i64 = 256;
const JSON_PARTIAL_OUTPUT_ON_ERROR: i64 = 512;
const JSON_THROW_ON_ERROR: i64 = 4194304;
const JSON_OBJECT_AS_ARRAY: i64 = 1; // for json_decode

fn json_encode(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    let flags = match args.get(1) {
        Some(v) => v.to_long(),
        None => 0,
    };
    let throw_on_error = flags & JSON_THROW_ON_ERROR != 0;

    // Check for NAN/INF at top level
    if let Value::Double(f) = val {
        if f.is_nan() || f.is_infinite() {
            if throw_on_error {
                let exc = vm.create_exception(b"ValueError", "Inf and NaN cannot be JSON encoded", 0);
                vm.current_exception = Some(exc);
                return Err(VmError { message: "Uncaught ValueError: Inf and NaN cannot be JSON encoded".to_string(), line: 0 });
            }
            vm.json_last_error = 7; // JSON_ERROR_INF_OR_NAN
            // JSON_PARTIAL_OUTPUT_ON_ERROR flag
            if flags & JSON_PARTIAL_OUTPUT_ON_ERROR != 0 {
                return Ok(Value::String(PhpString::from_bytes(b"0")));
            }
            return Ok(Value::False);
        }
    }

    if !throw_on_error {
        vm.json_last_error = 0;
    }
    let s = json_encode_value_flags(val, 0, flags);
    Ok(Value::String(PhpString::from_string(s)))
}

fn json_encode_value_flags(val: &Value, depth: usize, flags: i64) -> String {
    if depth > 512 {
        return "null".to_string();
    }
    let indent = if flags & JSON_PRETTY_PRINT != 0 {
        "    ".repeat(depth)
    } else {
        String::new()
    };
    let inner_indent = if flags & JSON_PRETTY_PRINT != 0 {
        "    ".repeat(depth + 1)
    } else {
        String::new()
    };
    let nl = if flags & JSON_PRETTY_PRINT != 0 { "\n" } else { "" };
    let sep = if flags & JSON_PRETTY_PRINT != 0 { ": " } else { ":" };

    match val {
        Value::Null | Value::Undef => "null".to_string(),
        Value::True => "true".to_string(),
        Value::False => "false".to_string(),
        Value::Long(n) => n.to_string(),
        Value::Double(f) => {
            if f.is_infinite() || f.is_nan() {
                "null".to_string()
            } else {
                // PHP uses serialize_precision for json_encode
                let sp = goro_core::value::get_php_serialize_precision();
                let s = if sp < 0 {
                    // Shortest roundtrip
                    let mut best = format!("{}", f);
                    for prec in 0..20 {
                        let candidate = format!("{:.prec$}", f, prec = prec);
                        if let Ok(parsed) = candidate.parse::<f64>() {
                            if parsed == *f {
                                best = candidate;
                                break;
                            }
                        }
                    }
                    best
                } else {
                    goro_core::value::format_php_float_with_precision_pub(*f, sp as usize)
                };
                if s.contains('.') || s.contains('e') || s.contains('E') {
                    s
                } else {
                    // Integer-valued float: PHP outputs it as "12.0" not "12"
                    format!("{}.0", s)
                }
            }
        }
        Value::String(s) => {
            json_encode_string(s.as_bytes(), flags)
        }
        Value::Array(arr) => {
            let arr = arr.borrow();
            let force_object = flags & JSON_FORCE_OBJECT != 0;
            let is_list = !force_object && arr.iter().enumerate().all(
                |(i, (k, _))| matches!(k, goro_core::array::ArrayKey::Int(n) if *n == i as i64),
            );
            if arr.len() == 0 {
                if is_list {
                    return "[]".to_string();
                } else {
                    return "{}".to_string();
                }
            }
            if is_list {
                let parts: Vec<String> = arr.values().map(|v| json_encode_value_flags(v, depth + 1, flags)).collect();
                if flags & JSON_PRETTY_PRINT != 0 {
                    format!("[{nl}{}{nl}{}]", parts.iter().map(|p| format!("{}{}", inner_indent, p)).collect::<Vec<_>>().join(&format!(",{nl}")), indent)
                } else {
                    format!("[{}]", parts.join(","))
                }
            } else {
                let parts: Vec<String> = arr
                    .iter()
                    .map(|(k, v)| {
                        let key_str = match k {
                            goro_core::array::ArrayKey::Int(n) => format!("\"{}\"", n),
                            goro_core::array::ArrayKey::String(s) => {
                                json_encode_string(s.as_bytes(), flags)
                            }
                        };
                        format!("{}{}{}", key_str, sep, json_encode_value_flags(v, depth + 1, flags))
                    })
                    .collect();
                if flags & JSON_PRETTY_PRINT != 0 {
                    format!("{{{nl}{}{nl}{}}}", parts.iter().map(|p| format!("{}{}", inner_indent, p)).collect::<Vec<_>>().join(&format!(",{nl}")), indent)
                } else {
                    format!("{{{}}}", parts.join(","))
                }
            }
        }
        Value::Object(obj) => {
            let obj = obj.borrow();
            if obj.properties.is_empty() {
                return "{}".to_string();
            }
            let parts: Vec<String> = obj.properties.iter().map(|(name, val)| {
                let key = json_encode_string(name, flags);
                format!("{}{}{}", key, sep, json_encode_value_flags(val, depth + 1, flags))
            }).collect();
            if flags & JSON_PRETTY_PRINT != 0 {
                format!("{{{nl}{}{nl}{}}}", parts.iter().map(|p| format!("{}{}", inner_indent, p)).collect::<Vec<_>>().join(&format!(",{nl}")), indent)
            } else {
                format!("{{{}}}", parts.join(","))
            }
        }
        Value::Generator(_) => "null".to_string(),
        Value::Reference(r) => json_encode_value_flags(&r.borrow(), depth, flags),
    }
}

fn json_encode_string(bytes: &[u8], flags: i64) -> String {
    let mut result = String::from("\"");
    let unescaped_unicode = flags & JSON_UNESCAPED_UNICODE != 0;
    let unescaped_slashes = flags & JSON_UNESCAPED_SLASHES != 0;
    let hex_tag = flags & JSON_HEX_TAG != 0;
    let hex_amp = flags & JSON_HEX_AMP != 0;
    let hex_apos = flags & JSON_HEX_APOS != 0;
    let hex_quot = flags & JSON_HEX_QUOT != 0;

    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'"' => {
                if hex_quot {
                    result.push_str("\\u0022");
                } else {
                    result.push_str("\\\"");
                }
            }
            b'\\' => result.push_str("\\\\"),
            b'/' => {
                if unescaped_slashes {
                    result.push('/');
                } else {
                    result.push_str("\\/");
                }
            }
            b'\n' => result.push_str("\\n"),
            b'\r' => result.push_str("\\r"),
            b'\t' => result.push_str("\\t"),
            0x08 => result.push_str("\\b"),
            0x0C => result.push_str("\\f"),
            b'<' if hex_tag => result.push_str("\\u003C"),
            b'>' if hex_tag => result.push_str("\\u003E"),
            b'&' if hex_amp => result.push_str("\\u0026"),
            b'\'' if hex_apos => result.push_str("\\u0027"),
            b if b < 0x20 => result.push_str(&format!("\\u{:04x}", b)),
            b if b < 0x80 => result.push(b as char),
            _ => {
                // Multi-byte UTF-8 sequence
                let remaining = &bytes[i..];
                let (codepoint, len) = decode_utf8_char(remaining);
                if let Some(cp) = codepoint {
                    if unescaped_unicode {
                        // Output the raw UTF-8 bytes as a valid UTF-8 string
                        if let Some(c) = char::from_u32(cp) {
                            result.push(c);
                        } else {
                            for j in 0..len {
                                result.push(bytes[i + j] as char);
                            }
                        }
                    } else {
                        // Escape as \uXXXX (or surrogate pair for > U+FFFF)
                        if cp <= 0xFFFF {
                            result.push_str(&format!("\\u{:04x}", cp));
                        } else {
                            // UTF-16 surrogate pair
                            let cp = cp - 0x10000;
                            let high = 0xD800 + (cp >> 10);
                            let low = 0xDC00 + (cp & 0x3FF);
                            result.push_str(&format!("\\u{:04x}\\u{:04x}", high, low));
                        }
                    }
                    i += len;
                    continue;
                } else {
                    // Invalid UTF-8: skip byte
                    result.push_str(&format!("\\u{:04x}", b));
                }
            }
        }
        i += 1;
    }
    result.push('"');
    result
}

fn decode_utf8_char(bytes: &[u8]) -> (Option<u32>, usize) {
    if bytes.is_empty() {
        return (None, 0);
    }
    let b0 = bytes[0];
    if b0 < 0x80 {
        (Some(b0 as u32), 1)
    } else if b0 & 0xE0 == 0xC0 {
        if bytes.len() < 2 || bytes[1] & 0xC0 != 0x80 {
            return (None, 1);
        }
        let cp = ((b0 as u32 & 0x1F) << 6) | (bytes[1] as u32 & 0x3F);
        (Some(cp), 2)
    } else if b0 & 0xF0 == 0xE0 {
        if bytes.len() < 3 || bytes[1] & 0xC0 != 0x80 || bytes[2] & 0xC0 != 0x80 {
            return (None, 1);
        }
        let cp = ((b0 as u32 & 0x0F) << 12) | ((bytes[1] as u32 & 0x3F) << 6) | (bytes[2] as u32 & 0x3F);
        (Some(cp), 3)
    } else if b0 & 0xF8 == 0xF0 {
        if bytes.len() < 4 || bytes[1] & 0xC0 != 0x80 || bytes[2] & 0xC0 != 0x80 || bytes[3] & 0xC0 != 0x80 {
            return (None, 1);
        }
        let cp = ((b0 as u32 & 0x07) << 18) | ((bytes[1] as u32 & 0x3F) << 12) | ((bytes[2] as u32 & 0x3F) << 6) | (bytes[3] as u32 & 0x3F);
        (Some(cp), 4)
    } else {
        (None, 1)
    }
}

fn json_decode(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let json_str = match args.first() {
        Some(v) => v.to_php_string(),
        None => return Ok(Value::Null),
    };
    let json_bytes = json_str.as_bytes();

    // $associative parameter: null (default) means objects become stdClass, true means arrays
    let mut associative = match args.get(1) {
        Some(Value::True) => true,
        Some(Value::Null) | Some(Value::Undef) | None => false,
        Some(Value::False) => false,
        Some(v) => v.is_truthy(),
    };

    // $depth parameter (arg 2) - we don't enforce it strictly but parse it
    let max_depth: usize = match args.get(2) {
        Some(Value::Long(n)) if *n > 0 => *n as usize,
        Some(Value::Long(_)) => return Ok(Value::Null),
        None => 512,
        _ => 512,
    };

    // $flags parameter (arg 3) - parse relevant flags
    let flags: i64 = match args.get(3) {
        Some(v) => v.to_long(),
        None => 0,
    };
    let bigint_as_string = (flags & 2) != 0; // JSON_BIGINT_AS_STRING = 2
    let throw_on_error = (flags & JSON_THROW_ON_ERROR) != 0;

    // JSON_OBJECT_AS_ARRAY flag
    if (flags & JSON_OBJECT_AS_ARRAY) != 0 {
        associative = true;
    }

    let mut parser = JsonParser {
        input: json_bytes,
        pos: 0,
        depth: 0,
        max_depth,
        associative,
        bigint_as_string,
        vm,
    };

    match parser.parse_value() {
        Some(val) => {
            parser.skip_whitespace();
            if parser.pos < parser.input.len() {
                // Trailing data after valid JSON
                if throw_on_error {
                    let exc = vm.create_exception(b"JsonException", "Syntax error", 0);
                    vm.current_exception = Some(exc);
                    return Err(VmError { message: "Uncaught JsonException: Syntax error".to_string(), line: 0 });
                }
                vm.json_last_error = 4; // JSON_ERROR_SYNTAX
                Ok(Value::Null)
            } else {
                if !throw_on_error {
                    vm.json_last_error = 0;
                }
                Ok(val)
            }
        }
        None => {
            // Determine error type
            let error_code;
            let error_msg;
            if json_bytes.is_empty() {
                error_code = 4;
                error_msg = "Syntax error";
            } else if parser.depth > parser.max_depth {
                error_code = 6;
                error_msg = "Maximum stack depth exceeded";
            } else {
                error_code = 4;
                error_msg = "Syntax error";
            }
            if throw_on_error {
                let exc = vm.create_exception(b"JsonException", error_msg, 0);
                vm.current_exception = Some(exc);
                return Err(VmError { message: format!("Uncaught JsonException: {}", error_msg), line: 0 });
            }
            vm.json_last_error = error_code;
            Ok(Value::Null)
        }
    }
}

/// Hand-rolled JSON parser that converts JSON to PHP values.
struct JsonParser<'a, 'b> {
    input: &'a [u8],
    pos: usize,
    depth: usize,
    max_depth: usize,
    associative: bool,
    bigint_as_string: bool,
    vm: &'b mut Vm,
}

impl<'a, 'b> JsonParser<'a, 'b> {
    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<u8> {
        let ch = self.input.get(self.pos).copied();
        if ch.is_some() {
            self.pos += 1;
        }
        ch
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.input.len() {
            match self.input[self.pos] {
                b' ' | b'\t' | b'\n' | b'\r' => self.pos += 1,
                _ => break,
            }
        }
    }

    fn parse_value(&mut self) -> Option<Value> {
        self.skip_whitespace();
        let ch = self.peek()?;
        match ch {
            b'"' => self
                .parse_string()
                .map(|s| Value::String(PhpString::from_vec(s))),
            b'{' => self.parse_object(),
            b'[' => self.parse_array(),
            b't' => self.parse_literal(b"true", Value::True),
            b'f' => self.parse_literal(b"false", Value::False),
            b'n' => self.parse_literal(b"null", Value::Null),
            b'-' | b'0'..=b'9' => self.parse_number(),
            _ => None,
        }
    }

    fn parse_literal(&mut self, expected: &[u8], value: Value) -> Option<Value> {
        if self.input[self.pos..].starts_with(expected) {
            self.pos += expected.len();
            Some(value)
        } else {
            None
        }
    }

    fn parse_string(&mut self) -> Option<Vec<u8>> {
        // Skip opening quote
        if self.advance()? != b'"' {
            return None;
        }

        let mut result = Vec::new();
        loop {
            let ch = self.advance()?;
            match ch {
                b'"' => return Some(result),
                b'\\' => {
                    let esc = self.advance()?;
                    match esc {
                        b'"' => result.push(b'"'),
                        b'\\' => result.push(b'\\'),
                        b'/' => result.push(b'/'),
                        b'n' => result.push(b'\n'),
                        b'r' => result.push(b'\r'),
                        b't' => result.push(b'\t'),
                        b'b' => result.push(0x08),
                        b'f' => result.push(0x0C),
                        b'u' => {
                            let cp = self.parse_hex4()?;
                            // Handle UTF-16 surrogate pairs
                            if (0xD800..=0xDBFF).contains(&cp) {
                                // High surrogate, expect \uXXXX low surrogate
                                if self.advance()? != b'\\' {
                                    return None;
                                }
                                if self.advance()? != b'u' {
                                    return None;
                                }
                                let low = self.parse_hex4()?;
                                if !(0xDC00..=0xDFFF).contains(&low) {
                                    return None;
                                }
                                let codepoint =
                                    0x10000 + ((cp as u32 - 0xD800) << 10) + (low as u32 - 0xDC00);
                                if let Some(c) = char::from_u32(codepoint) {
                                    let mut buf = [0u8; 4];
                                    let s = c.encode_utf8(&mut buf);
                                    result.extend_from_slice(s.as_bytes());
                                } else {
                                    return None;
                                }
                            } else if (0xDC00..=0xDFFF).contains(&cp) {
                                // Lone low surrogate is invalid
                                return None;
                            } else {
                                if let Some(c) = char::from_u32(cp as u32) {
                                    let mut buf = [0u8; 4];
                                    let s = c.encode_utf8(&mut buf);
                                    result.extend_from_slice(s.as_bytes());
                                } else {
                                    return None;
                                }
                            }
                        }
                        _ => return None, // Invalid escape sequence
                    }
                }
                // Control characters (0x00-0x1F) are invalid unescaped in JSON
                b if b < 0x20 => return None,
                _ => result.push(ch),
            }
        }
    }

    fn parse_hex4(&mut self) -> Option<u16> {
        let mut val: u16 = 0;
        for _ in 0..4 {
            let ch = self.advance()?;
            let digit = match ch {
                b'0'..=b'9' => ch - b'0',
                b'a'..=b'f' => ch - b'a' + 10,
                b'A'..=b'F' => ch - b'A' + 10,
                _ => return None,
            };
            val = val * 16 + digit as u16;
        }
        Some(val)
    }

    fn parse_number(&mut self) -> Option<Value> {
        let start = self.pos;

        // Optional minus
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }

        // Integer part
        match self.peek() {
            Some(b'0') => {
                self.pos += 1;
                // After leading 0, next char must not be a digit (no leading zeros)
            }
            Some(b'1'..=b'9') => {
                self.pos += 1;
                while let Some(b'0'..=b'9') = self.peek() {
                    self.pos += 1;
                }
            }
            _ => return None,
        }

        let mut is_float = false;

        // Fractional part
        if self.peek() == Some(b'.') {
            is_float = true;
            self.pos += 1;
            // Must have at least one digit after decimal point
            match self.peek() {
                Some(b'0'..=b'9') => {}
                _ => return None,
            }
            while let Some(b'0'..=b'9') = self.peek() {
                self.pos += 1;
            }
        }

        // Exponent part
        if let Some(b'e') | Some(b'E') = self.peek() {
            is_float = true;
            self.pos += 1;
            // Optional sign
            if let Some(b'+') | Some(b'-') = self.peek() {
                self.pos += 1;
            }
            // Must have at least one digit
            match self.peek() {
                Some(b'0'..=b'9') => {}
                _ => return None,
            }
            while let Some(b'0'..=b'9') = self.peek() {
                self.pos += 1;
            }
        }

        let num_str = std::str::from_utf8(&self.input[start..self.pos]).ok()?;

        if is_float {
            let f: f64 = num_str.parse().ok()?;
            Some(Value::Double(f))
        } else {
            // Try to parse as i64 first, fall back to f64 for very large numbers
            if let Ok(n) = num_str.parse::<i64>() {
                Some(Value::Long(n))
            } else if self.bigint_as_string {
                Some(Value::String(PhpString::from_string(num_str.to_string())))
            } else {
                // Number too large for i64, parse as float
                let f: f64 = num_str.parse().ok()?;
                Some(Value::Double(f))
            }
        }
    }

    fn parse_array(&mut self) -> Option<Value> {
        // Skip opening bracket
        if self.advance()? != b'[' {
            return None;
        }

        self.depth += 1;
        if self.depth > self.max_depth {
            return None;
        }

        let mut arr = PhpArray::new();

        self.skip_whitespace();
        if self.peek() == Some(b']') {
            self.pos += 1;
            self.depth -= 1;
            return Some(Value::Array(Rc::new(RefCell::new(arr))));
        }

        loop {
            let val = self.parse_value()?;
            arr.push(val);

            self.skip_whitespace();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                }
                Some(b']') => {
                    self.pos += 1;
                    self.depth -= 1;
                    return Some(Value::Array(Rc::new(RefCell::new(arr))));
                }
                _ => return None,
            }
        }
    }

    fn parse_object(&mut self) -> Option<Value> {
        // Skip opening brace
        if self.advance()? != b'{' {
            return None;
        }

        self.depth += 1;
        if self.depth > self.max_depth {
            return None;
        }

        self.skip_whitespace();

        if self.associative {
            // Decode as associative array
            let mut arr = PhpArray::new();

            if self.peek() == Some(b'}') {
                self.pos += 1;
                self.depth -= 1;
                return Some(Value::Array(Rc::new(RefCell::new(arr))));
            }

            loop {
                self.skip_whitespace();
                let key = self.parse_string()?;

                self.skip_whitespace();
                if self.advance()? != b':' {
                    return None;
                }

                let val = self.parse_value()?;
                arr.set(
                    goro_core::array::ArrayKey::String(PhpString::from_vec(key)),
                    val,
                );

                self.skip_whitespace();
                match self.peek() {
                    Some(b',') => {
                        self.pos += 1;
                    }
                    Some(b'}') => {
                        self.pos += 1;
                        self.depth -= 1;
                        return Some(Value::Array(Rc::new(RefCell::new(arr))));
                    }
                    _ => return None,
                }
            }
        } else {
            // Decode as stdClass object
            let obj_id = self.vm.next_object_id();
            let mut obj = PhpObject::new(b"stdClass".to_vec(), obj_id);

            if self.peek() == Some(b'}') {
                self.pos += 1;
                self.depth -= 1;
                return Some(Value::Object(Rc::new(RefCell::new(obj))));
            }

            loop {
                self.skip_whitespace();
                let key = self.parse_string()?;

                self.skip_whitespace();
                if self.advance()? != b':' {
                    return None;
                }

                let val = self.parse_value()?;
                obj.set_property(key, val);

                self.skip_whitespace();
                match self.peek() {
                    Some(b',') => {
                        self.pos += 1;
                    }
                    Some(b'}') => {
                        self.pos += 1;
                        self.depth -= 1;
                        return Some(Value::Object(Rc::new(RefCell::new(obj))));
                    }
                    _ => return None,
                }
            }
        }
    }
}
fn json_last_error(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(vm.json_last_error))
}
fn json_last_error_msg(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let msg = match vm.json_last_error {
        0 => "No error",
        1 => "Maximum stack depth exceeded",
        2 => "Invalid or malformed JSON",
        3 => "Control character error, possibly incorrectly encoded",
        4 => "Syntax error",
        5 => "Malformed UTF-8 characters, possibly incorrectly encoded",
        6 => "Recursion detected",
        7 => "Inf and NaN cannot be JSON encoded",
        8 => "Type is not supported",
        9 => "The decoded property name is invalid",
        10 => "Single unpaired UTF-16 surrogate in unicode escape",
        _ => "Unknown error",
    };
    Ok(Value::String(PhpString::from_bytes(msg.as_bytes())))
}

fn json_validate(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let json_str = match args.first() {
        Some(v) => v.to_php_string(),
        None => return Ok(Value::False),
    };
    let json_bytes = json_str.as_bytes();

    let max_depth: usize = match args.get(1) {
        Some(Value::Long(n)) if *n > 0 => *n as usize,
        Some(Value::Long(_)) => return Ok(Value::False),
        None => 512,
        _ => 512,
    };

    let mut parser = JsonParser {
        input: json_bytes,
        pos: 0,
        depth: 0,
        max_depth,
        associative: true,
        bigint_as_string: false,
        vm,
    };

    match parser.parse_value() {
        Some(_) => {
            parser.skip_whitespace();
            if parser.pos < parser.input.len() {
                Ok(Value::False)
            } else {
                Ok(Value::True)
            }
        }
        None => Ok(Value::False),
    }
}
