//! Port of ext/json/json_scanner.re and json_parser.y.
//!
//! The parser is an explicit-stack LALR equivalent (bison's parser doesn't
//! recurse either), so deep documents can't overflow the host stack. It
//! reads tokens in the same order bison does, so the first error found, and
//! its code, are the same.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::PhpArray;
use goro_core::object::PhpObject;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;

use crate::*;

#[derive(Debug)]
enum Tok {
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Colon,
    Comma,
    Value(Value),
    /// A string (PHP_JSON_T_STRING / PHP_JSON_T_ESTRING)
    Str(Vec<u8>),
    Eoi,
    Error,
}

/// php_json_scanner
struct Scanner<'a> {
    s: &'a [u8],
    cursor: usize,
    options: i64,
    errcode: i64,
}

#[inline]
fn is_hex(c: u8) -> bool {
    c.is_ascii_hexdigit()
}

fn hex_val(c: u8) -> u32 {
    (c as char).to_digit(16).unwrap_or(0)
}

/// Length of the valid UTF-8 sequence (re2c `UTF8` above ASCII) at `s`.
fn utf8_seq(s: &[u8], i: usize) -> usize {
    let at = |k: usize| s.get(i + k).copied().unwrap_or(0);
    let t = |c: u8| (0x80..=0xBF).contains(&c);
    let c = at(0);
    match c {
        0xC2..=0xDF if t(at(1)) => 2,
        0xE0 if (0xA0..=0xBF).contains(&at(1)) && t(at(2)) => 3,
        0xE1..=0xEC | 0xEE..=0xEF if t(at(1)) && t(at(2)) => 3,
        0xED if (0x80..=0x9F).contains(&at(1)) && t(at(2)) => 3,
        0xF0 if (0x90..=0xBF).contains(&at(1)) && t(at(2)) && t(at(3)) => 4,
        0xF1..=0xF3 if t(at(1)) && t(at(2)) && t(at(3)) => 4,
        0xF4 if (0x80..=0x8F).contains(&at(1)) && t(at(2)) && t(at(3)) => 4,
        _ => 0,
    }
}

fn push_utf8(out: &mut Vec<u8>, cp: u32) {
    if cp < 0x80 {
        out.push(cp as u8);
    } else if cp < 0x800 {
        out.push((0xc0 | (cp >> 6)) as u8);
        out.push((0x80 | (cp & 0x3f)) as u8);
    } else if cp < 0x10000 {
        out.push((0xe0 | (cp >> 12)) as u8);
        out.push((0x80 | ((cp >> 6) & 0x3f)) as u8);
        out.push((0x80 | (cp & 0x3f)) as u8);
    } else {
        out.push((0xf0 | (cp >> 18)) as u8);
        out.push((0x80 | ((cp >> 12) & 0x3f)) as u8);
        out.push((0x80 | ((cp >> 6) & 0x3f)) as u8);
        out.push((0x80 | (cp & 0x3f)) as u8);
    }
}

/// LONG_MIN's digits (PHP_JSON_INT_MAX_LENGTH of them)
const LONG_MIN_DIGITS: &[u8] = b"9223372036854775808";

impl<'a> Scanner<'a> {
    /// The byte at `i` (the string's NUL terminator past the end).
    #[inline]
    fn at(&self, i: usize) -> u8 {
        self.s.get(i).copied().unwrap_or(0)
    }

    fn error(&mut self, code: i64) -> Tok {
        self.errcode = code;
        Tok::Error
    }

    fn scan(&mut self) -> Tok {
        loop {
            let start = self.cursor;
            let c = self.at(start);
            self.cursor += 1;
            match c {
                b'{' => return Tok::LBrace,
                b'}' => return Tok::RBrace,
                b'[' => return Tok::LBracket,
                b']' => return Tok::RBracket,
                b':' => return Tok::Colon,
                b',' => return Tok::Comma,
                b' ' | b'\t' | b'\r' | b'\n' => continue,
                0 => {
                    // EOI
                    if self.s.len() < self.cursor {
                        return Tok::Eoi;
                    }
                    return self.error(ERROR_CTRL_CHAR);
                }
                b'"' => return self.scan_string(),
                b'n' | b't' | b'f' => {
                    let rest = &self.s[start..];
                    for (lit, v) in [
                        (&b"null"[..], Value::Null),
                        (b"true", Value::True),
                        (b"false", Value::False),
                    ] {
                        if rest.starts_with(lit) {
                            self.cursor = start + lit.len();
                            return Tok::Value(v);
                        }
                    }
                    return self.error(ERROR_SYNTAX);
                }
                b'-' | b'0'..=b'9' => {
                    if let Some(t) = self.scan_number(start) {
                        return t;
                    }
                    return self.error(ERROR_SYNTAX);
                }
                0x01..=0x1f => return self.error(ERROR_CTRL_CHAR),
                0x80..=0xff => {
                    if utf8_seq(self.s, start) > 0 {
                        return self.error(ERROR_SYNTAX);
                    }
                    return self.error(ERROR_UTF8);
                }
                _ => return self.error(ERROR_SYNTAX),
            }
        }
    }

    /// INT, FLOAT and EXP (longest match).
    fn scan_number(&mut self, start: usize) -> Option<Tok> {
        let mut i = start;
        let negative = self.at(i) == b'-';
        if negative {
            i += 1;
        }
        // UINT = "0" | [1-9][0-9]*
        match self.at(i) {
            b'0' => i += 1,
            b'1'..=b'9' => {
                while self.at(i).is_ascii_digit() {
                    i += 1;
                }
            }
            _ => return None,
        }
        let int_end = i;
        let mut is_float = false;
        if self.at(i) == b'.' && self.at(i + 1).is_ascii_digit() {
            i += 1;
            while self.at(i).is_ascii_digit() {
                i += 1;
            }
            is_float = true;
        }
        if matches!(self.at(i), b'e' | b'E') {
            let mut j = i + 1;
            if matches!(self.at(j), b'+' | b'-') {
                j += 1;
            }
            if self.at(j).is_ascii_digit() {
                while self.at(j).is_ascii_digit() {
                    j += 1;
                }
                i = j;
                is_float = true;
            }
        }
        self.cursor = i;
        let text = &self.s[start..i];
        if is_float {
            return Some(Tok::Value(Value::Double(strtod(text))));
        }
        let _ = int_end;
        let digits = text.len() - negative as usize;
        let mut bigint = false;
        if digits >= LONG_MIN_DIGITS.len() {
            if digits == LONG_MIN_DIGITS.len() {
                let d = &text[negative as usize..];
                let cmp = d.cmp(LONG_MIN_DIGITS);
                if !(cmp == std::cmp::Ordering::Less
                    || (cmp == std::cmp::Ordering::Equal && negative))
                {
                    bigint = true;
                }
            } else {
                bigint = true;
            }
        }
        if !bigint {
            let n = std::str::from_utf8(text)
                .ok()
                .and_then(|t| t.parse::<i64>().ok())
                .unwrap_or(0);
            Some(Tok::Value(Value::Long(n)))
        } else if self.options & BIGINT_AS_STRING != 0 {
            Some(Tok::Str(text.to_vec()))
        } else {
            Some(Tok::Value(Value::Double(strtod(text))))
        }
    }

    /// STR_P1 + STR_P2: a string body after its opening quote.
    fn scan_string(&mut self) -> Tok {
        let mut out = Vec::new();
        loop {
            let i = self.cursor;
            let c = self.at(i);
            match c {
                0x00..=0x1f => return self.error(ERROR_CTRL_CHAR),
                b'"' => {
                    self.cursor += 1;
                    return Tok::Str(out);
                }
                b'\\' => {
                    let e = self.at(i + 1);
                    match e {
                        b'u' => {
                            let h = |k: usize| self.at(i + 2 + k);
                            if !(0..4).all(|k| is_hex(h(k))) {
                                return self.error(ERROR_SYNTAX);
                            }
                            let cp = (0..4).fold(0u32, |a, k| (a << 4) | hex_val(h(k)));
                            if (0xD800..=0xDBFF).contains(&cp) {
                                // UTF16_4: a surrogate pair
                                let j = i + 6;
                                let lo_ok = self.at(j) == b'\\'
                                    && self.at(j + 1) == b'u'
                                    && (0..4).all(|k| is_hex(self.at(j + 2 + k)));
                                let lo = if lo_ok {
                                    (0..4).fold(0u32, |a, k| (a << 4) | hex_val(self.at(j + 2 + k)))
                                } else {
                                    0
                                };
                                if !(0xDC00..=0xDFFF).contains(&lo) {
                                    return self.error(ERROR_UTF16);
                                }
                                let utf32 = ((cp & 0x3FF) << 10) + (lo & 0x3FF) + 0x10000;
                                push_utf8(&mut out, utf32);
                                self.cursor = i + 12;
                            } else if (0xDC00..=0xDFFF).contains(&cp) {
                                return self.error(ERROR_UTF16);
                            } else {
                                push_utf8(&mut out, cp);
                                self.cursor = i + 6;
                            }
                        }
                        b'"' | b'\\' | b'/' => {
                            out.push(e);
                            self.cursor = i + 2;
                        }
                        b'b' | b'f' | b'n' | b'r' | b't' => {
                            out.push(match e {
                                b'b' => 0x08,
                                b'f' => 0x0c,
                                b'n' => b'\n',
                                b'r' => b'\r',
                                _ => b'\t',
                            });
                            self.cursor = i + 2;
                        }
                        _ => return self.error(ERROR_SYNTAX),
                    }
                }
                0x20..=0x7f => {
                    out.push(c);
                    self.cursor += 1;
                }
                _ => {
                    let n = utf8_seq(self.s, i);
                    if n > 0 {
                        out.extend_from_slice(&self.s[i..i + n]);
                        self.cursor += n;
                    } else if self.options & (INVALID_UTF8_IGNORE | INVALID_UTF8_SUBSTITUTE) != 0 {
                        if self.options & INVALID_UTF8_SUBSTITUTE != 0 {
                            out.extend_from_slice(b"\xef\xbf\xbd");
                        }
                        self.cursor += 1;
                    } else {
                        return self.error(ERROR_UTF8);
                    }
                }
            }
        }
    }
}

/// zend_strtod() of a JSON number
fn strtod(text: &[u8]) -> f64 {
    std::str::from_utf8(text)
        .ok()
        .and_then(|t| t.parse::<f64>().ok())
        .unwrap_or(0.0)
}

/// An object or array being built.
enum Container {
    Array(PhpArray),
    Object(Rc<RefCell<PhpObject>>),
}

enum Frame {
    /// '[' elements: the array (created with the first element), whether
    /// an element was parsed
    Array(Option<PhpArray>, bool),
    /// '{' members: the container (created with the first member), the
    /// key of the member whose value is being parsed, whether a member
    /// precedes it
    Object(Option<Container>, Vec<u8>, bool),
}

/// YYMAXDEPTH of the bison parser: its state stack holds fewer entries.
const YYMAXDEPTH: usize = 10000;

pub struct Parser<'a, 'v> {
    scanner: Scanner<'a>,
    /// Entries on the bison parser's state stack (one per grammar symbol
    /// of the rules in progress, plus the start state): too deep a nesting
    /// exhausts it, a syntax error.
    slots: usize,
    depth: i32,
    max_depth: i32,
    validate: bool,
    vm: &'v mut Vm,
}

impl<'a, 'v> Parser<'a, 'v> {
    pub fn new(vm: &'v mut Vm, s: &'a [u8], options: i64, max_depth: i32, validate: bool) -> Self {
        Parser {
            scanner: Scanner {
                s,
                cursor: 0,
                options,
                errcode: 0,
            },
            slots: 1,
            depth: 1,
            max_depth,
            validate,
            vm,
        }
    }

    pub fn error_code(&self) -> i64 {
        self.scanner.errcode
    }

    /// yyerror(): a syntax error unless the scanner reported one.
    fn fail<T>(&mut self) -> Result<T, ()> {
        if self.scanner.errcode == 0 {
            self.scanner.errcode = ERROR_SYNTAX;
        }
        Err(())
    }

    fn fail_with<T>(&mut self, code: i64) -> Result<T, ()> {
        self.scanner.errcode = code;
        Err(())
    }

    /// Push `n` states on the parser stack ("memory exhausted" when it is
    /// full: yyerror()).
    fn push(&mut self, n: usize) -> Result<(), ()> {
        for _ in 0..n {
            self.slots += 1;
            if self.slots >= YYMAXDEPTH {
                return self.fail();
            }
        }
        Ok(())
    }

    /// Reduce a rule of `n` symbols (its result is pushed in their place).
    fn reduce(&mut self, n: usize) {
        self.slots -= n - 1;
    }

    fn depth_inc(&mut self) -> Result<(), ()> {
        if self.max_depth != 0 && self.depth >= self.max_depth {
            return self.fail_with(ERROR_DEPTH);
        }
        self.depth += 1;
        Ok(())
    }

    fn object_as_array(&self) -> bool {
        self.scanner.options & OBJECT_AS_ARRAY != 0
    }

    fn object_create(&mut self) -> Container {
        if self.object_as_array() {
            Container::Array(PhpArray::new())
        } else {
            let id = self.vm.next_object_id();
            Container::Object(Rc::new(RefCell::new(PhpObject::new(
                b"stdClass".to_vec(),
                id,
            ))))
        }
    }

    fn object_update(&mut self, c: &mut Container, key: Vec<u8>, v: Value) -> Result<(), ()> {
        if self.validate {
            return Ok(());
        }
        match c {
            Container::Array(a) => {
                let k = Vm::value_to_array_key(Value::String(PhpString::from_vec(key)));
                a.set(k, v);
            }
            Container::Object(o) => {
                if key.first() == Some(&0) {
                    return self.fail_with(ERROR_INVALID_PROPERTY_NAME);
                }
                o.borrow_mut().set_property(key, v);
            }
        }
        Ok(())
    }

    fn finish_object(&mut self, c: Option<Container>) -> Value {
        let c = match c {
            Some(c) => c,
            None => {
                if self.validate {
                    return Value::Null;
                }
                if self.object_as_array() {
                    Container::Array(PhpArray::new())
                } else {
                    self.object_create()
                }
            }
        };
        match c {
            Container::Array(a) => Value::Array(Rc::new(RefCell::new(a))),
            Container::Object(o) => Value::Object(o),
        }
    }

    /// Expects `key ':'` (the key token already read); returns the key.
    fn member_key(&mut self, t: Tok) -> Result<Vec<u8>, ()> {
        let Tok::Str(k) = t else {
            return self.fail();
        };
        self.push(1)?;
        match self.scanner.scan() {
            Tok::Colon => {
                self.push(1)?;
                Ok(k)
            }
            _ => self.fail(),
        }
    }

    /// start: value PHP_JSON_T_EOI
    pub fn parse(&mut self) -> Result<Value, ()> {
        let mut stack: Vec<Frame> = Vec::new();
        let mut tok = self.scanner.scan();
        loop {
            // Parse a value starting with `tok`.
            let mut value = match tok {
                Tok::LBrace => {
                    // '{' $@1
                    self.push(1)?;
                    self.depth_inc()?;
                    self.push(1)?;
                    let t = self.scanner.scan();
                    match t {
                        Tok::RBrace => {
                            // members: %empty, then '}'
                            self.push(2)?;
                            self.reduce(4);
                            self.depth -= 1;
                            self.finish_object(None)
                        }
                        Tok::RBracket => return self.fail_with(ERROR_STATE_MISMATCH),
                        t => {
                            let key = self.member_key(t)?;
                            stack.push(Frame::Object(None, key, false));
                            tok = self.scanner.scan();
                            continue;
                        }
                    }
                }
                Tok::LBracket => {
                    // '[' $@2
                    self.push(1)?;
                    self.depth_inc()?;
                    self.push(1)?;
                    let t = self.scanner.scan();
                    match t {
                        Tok::RBracket => {
                            // elements: %empty, then ']'
                            self.push(2)?;
                            self.reduce(4);
                            self.depth -= 1;
                            if self.validate {
                                Value::Null
                            } else {
                                Value::Array(Rc::new(RefCell::new(PhpArray::new())))
                            }
                        }
                        Tok::RBrace => return self.fail_with(ERROR_STATE_MISMATCH),
                        t => {
                            stack.push(Frame::Array(None, false));
                            tok = t;
                            continue;
                        }
                    }
                }
                Tok::Value(v) => {
                    self.push(1)?;
                    v
                }
                Tok::Str(s) => {
                    self.push(1)?;
                    if self.validate {
                        Value::Null
                    } else {
                        Value::String(PhpString::from_vec(s))
                    }
                }
                _ => return self.fail(),
            };
            // Reduce: hand the value to the enclosing containers.
            loop {
                match stack.last_mut() {
                    None => {
                        return match self.scanner.scan() {
                            Tok::Eoi => Ok(value),
                            _ => self.fail(),
                        };
                    }
                    Some(Frame::Array(arr, more)) => {
                        // element: value | element ',' value
                        if *more {
                            self.reduce(3);
                        }
                        *more = true;
                        if !self.validate {
                            arr.get_or_insert_with(PhpArray::new).push(value);
                        }
                        match self.scanner.scan() {
                            Tok::Comma => {
                                self.push(1)?;
                                tok = self.scanner.scan();
                                break;
                            }
                            Tok::RBracket => {
                                // elements: element, then ']'
                                self.push(1)?;
                                self.reduce(4);
                                let Some(Frame::Array(arr, _)) = stack.pop() else {
                                    unreachable!()
                                };
                                self.depth -= 1;
                                value = if self.validate {
                                    Value::Null
                                } else {
                                    Value::Array(Rc::new(RefCell::new(arr.unwrap_or_default())))
                                };
                            }
                            Tok::RBrace => return self.fail_with(ERROR_STATE_MISMATCH),
                            _ => return self.fail(),
                        }
                    }
                    Some(Frame::Object(..)) => {
                        let Some(Frame::Object(c, key, more)) = stack.pop() else {
                            unreachable!()
                        };
                        // member: key ':' value | member ',' key ':' value
                        self.reduce(if more { 5 } else { 3 });
                        let mut c = match c {
                            Some(c) => Some(c),
                            None if self.validate => None,
                            None => Some(self.object_create()),
                        };
                        if let Some(c) = c.as_mut() {
                            self.object_update(c, key, value)?;
                        }
                        match self.scanner.scan() {
                            Tok::Comma => {
                                self.push(1)?;
                                let t = self.scanner.scan();
                                let key = self.member_key(t)?;
                                stack.push(Frame::Object(c, key, true));
                                tok = self.scanner.scan();
                                break;
                            }
                            Tok::RBrace => {
                                // members: member, then '}'
                                self.push(1)?;
                                self.reduce(4);
                                self.depth -= 1;
                                value = self.finish_object(c);
                            }
                            Tok::RBracket => return self.fail_with(ERROR_STATE_MISMATCH),
                            _ => return self.fail(),
                        }
                    }
                }
            }
        }
    }
}
