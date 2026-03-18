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

fn json_encode(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    let s = json_encode_value(val);
    Ok(Value::String(PhpString::from_string(s)))
}

fn json_encode_value(val: &Value) -> String {
    json_encode_value_depth(val, 0)
}

fn json_encode_value_depth(val: &Value, depth: usize) -> String {
    if depth > 512 {
        return "null".to_string();
    }
    match val {
        Value::Null | Value::Undef => "null".to_string(),
        Value::True => "true".to_string(),
        Value::False => "false".to_string(),
        Value::Long(n) => n.to_string(),
        Value::Double(f) => {
            if f.is_infinite() || f.is_nan() {
                "null".to_string()
            } else {
                format!("{}", f)
            }
        }
        Value::String(s) => {
            let mut result = String::from("\"");
            for &b in s.as_bytes() {
                match b {
                    b'"' => result.push_str("\\\""),
                    b'\\' => result.push_str("\\\\"),
                    b'\n' => result.push_str("\\n"),
                    b'\r' => result.push_str("\\r"),
                    b'\t' => result.push_str("\\t"),
                    b if b < 0x20 => result.push_str(&format!("\\u{:04x}", b)),
                    _ => result.push(b as char),
                }
            }
            result.push('"');
            result
        }
        Value::Array(arr) => {
            let arr = arr.borrow();
            let is_list = arr.iter().enumerate().all(
                |(i, (k, _))| matches!(k, goro_core::array::ArrayKey::Int(n) if *n == i as i64),
            );
            if is_list {
                let parts: Vec<String> = arr.values().map(|v| json_encode_value_depth(v, depth + 1)).collect();
                format!("[{}]", parts.join(","))
            } else {
                let parts: Vec<String> = arr
                    .iter()
                    .map(|(k, v)| {
                        let key_str = match k {
                            goro_core::array::ArrayKey::Int(n) => format!("\"{}\"", n),
                            goro_core::array::ArrayKey::String(s) => {
                                format!("\"{}\"", s.to_string_lossy())
                            }
                        };
                        format!("{}:{}", key_str, json_encode_value_depth(v, depth + 1))
                    })
                    .collect();
                format!("{{{}}}", parts.join(","))
            }
        }
        Value::Object(obj) => {
            let obj = obj.borrow();
            if obj.properties.is_empty() {
                return "{}".to_string();
            }
            let parts: Vec<String> = obj.properties.iter().map(|(name, val)| {
                let key = String::from_utf8_lossy(name);
                format!("\"{}\":{}", key, json_encode_value_depth(val, depth + 1))
            }).collect();
            format!("{{{}}}", parts.join(","))
        }
        Value::Generator(_) => "null".to_string(),
        Value::Reference(r) => json_encode_value_depth(&r.borrow(), depth),
    }
}

fn json_decode(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let json_str = match args.first() {
        Some(v) => v.to_php_string(),
        None => return Ok(Value::Null),
    };
    let json_bytes = json_str.as_bytes();

    // $associative parameter: null (default) means objects become stdClass, true means arrays
    let associative = match args.get(1) {
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
                Ok(Value::Null)
            } else {
                Ok(val)
            }
        }
        None => Ok(Value::Null),
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
fn json_last_error(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(0))
}
fn json_last_error_msg(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(PhpString::from_bytes(b"No error")))
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
