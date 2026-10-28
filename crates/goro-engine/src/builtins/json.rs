//! JSON (PHP manual: JSON functions, JsonSerializable; RFC 8259).

use goro_runtime::{Key, KeyRef, ObjRef, PhpArray, PhpStr, Value};

use crate::class::{DisplayName, TypeName};
use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "json",
    functions: &[
        (
            "function json_encode(mixed $value, int $flags = 0, int $depth = 512): string|false",
            json_encode,
        ),
        (
            "function json_decode(string $json, ?bool $associative = null, int $depth = 512, int $flags = 0): mixed",
            json_decode,
        ),
        ("function json_last_error(): int", json_last_error),
        (
            "function json_last_error_msg(): string",
            json_last_error_msg,
        ),
        (
            "function json_validate(string $json, int $depth = 512, int $flags = 0): bool",
            json_validate,
        ),
    ],
    classes: &[(
        "jsonexception",
        "<?php class JsonException extends Exception {}",
    )],
    constants: &[
        ("JSON_HEX_TAG", ConstVal::Int(1)),
        ("JSON_HEX_AMP", ConstVal::Int(2)),
        ("JSON_HEX_APOS", ConstVal::Int(4)),
        ("JSON_HEX_QUOT", ConstVal::Int(8)),
        ("JSON_FORCE_OBJECT", ConstVal::Int(16)),
        ("JSON_NUMERIC_CHECK", ConstVal::Int(32)),
        ("JSON_UNESCAPED_SLASHES", ConstVal::Int(64)),
        ("JSON_PRETTY_PRINT", ConstVal::Int(128)),
        ("JSON_UNESCAPED_UNICODE", ConstVal::Int(256)),
        ("JSON_PARTIAL_OUTPUT_ON_ERROR", ConstVal::Int(512)),
        ("JSON_PRESERVE_ZERO_FRACTION", ConstVal::Int(1024)),
        ("JSON_UNESCAPED_LINE_TERMINATORS", ConstVal::Int(2048)),
        ("JSON_OBJECT_AS_ARRAY", ConstVal::Int(1)),
        ("JSON_BIGINT_AS_STRING", ConstVal::Int(2)),
        ("JSON_INVALID_UTF8_IGNORE", ConstVal::Int(1048576)),
        ("JSON_INVALID_UTF8_SUBSTITUTE", ConstVal::Int(2097152)),
        ("JSON_THROW_ON_ERROR", ConstVal::Int(4194304)),
        ("JSON_ERROR_NONE", ConstVal::Int(0)),
        ("JSON_ERROR_DEPTH", ConstVal::Int(1)),
        ("JSON_ERROR_STATE_MISMATCH", ConstVal::Int(2)),
        ("JSON_ERROR_CTRL_CHAR", ConstVal::Int(3)),
        ("JSON_ERROR_SYNTAX", ConstVal::Int(4)),
        ("JSON_ERROR_UTF8", ConstVal::Int(5)),
        ("JSON_ERROR_RECURSION", ConstVal::Int(6)),
        ("JSON_ERROR_INF_OR_NAN", ConstVal::Int(7)),
        ("JSON_ERROR_UNSUPPORTED_TYPE", ConstVal::Int(8)),
        ("JSON_ERROR_INVALID_PROPERTY_NAME", ConstVal::Int(9)),
        ("JSON_ERROR_UTF16", ConstVal::Int(10)),
        ("JSON_ERROR_NON_BACKED_ENUM", ConstVal::Int(11)),
    ],
    listed: true,
    ..Module::EMPTY
};

#[derive(Default)]
pub struct JsonState {
    pub last_error: i64,
    /// Objects being encoded by any json_encode() call (recursion across
    /// nested calls from jsonSerialize()/__serialize(): PHP's recursion
    /// protection is per object).
    pub active: Vec<usize>,
}

/// One UTF-8 sequence at `s[i]` as PHP's JSON scanner reads it: a lead
/// byte announces the length and the continuation bytes that follow are
/// consumed with it, so an invalid or truncated sequence is one unit
/// (observed on PHP 8.5: "\xf0\x80\x80A" substitutes one U+FFFD).
/// Returns (valid, length).
fn utf8_unit(s: &[u8], i: usize) -> (bool, usize) {
    let lead = s[i];
    let need = match lead {
        0xC0..=0xDF => 1,
        0xE0..=0xEF => 2,
        0xF0..=0xF7 => 3,
        _ => return (false, 1),
    };
    let mut k = 0;
    while k < need && s.get(i + 1 + k).is_some_and(|&c| (0x80..0xC0).contains(&c)) {
        k += 1;
    }
    if k < need {
        return (false, 1 + k);
    }
    let len = 1 + need;
    (std::str::from_utf8(&s[i..i + len]).is_ok(), len)
}

/// The length of a UTF-8 sequence from its lead byte.
fn utf8_len(c: u8) -> usize {
    match c {
        0xF0..=0xF7 => 4,
        0xE0..=0xEF => 3,
        0xC0..=0xDF => 2,
        _ => 1,
    }
}

/// The encoder's maximum nesting: deeper structures fail like PHP's
/// stack limit does (test gh15168), before the native stack runs out.
const MAX_NESTING: usize = 10000;

pub fn error_msg(code: i64) -> &'static str {
    match code {
        0 => "No error",
        1 => "Maximum stack depth exceeded",
        2 => "State mismatch (invalid or malformed JSON)",
        3 => "Control character error, possibly incorrectly encoded",
        4 => "Syntax error",
        5 => "Malformed UTF-8 characters, possibly incorrectly encoded",
        6 => "Recursion detected",
        7 => "Inf and NaN cannot be JSON encoded",
        8 => "Type is not supported",
        9 => "The decoded property name is invalid",
        10 => "Single unpaired UTF-16 surrogate in unicode escape",
        11 => "Non-backed enums have no default serialization",
        _ => "Unknown error",
    }
}

const PRETTY: i64 = 128;
const PARTIAL: i64 = 512;
const THROW: i64 = 4194304;

struct Enc {
    out: Vec<u8>,
    flags: i64,
    depth: usize,
    max_depth: usize,
    error: i64,
    stack: Vec<usize>,
}

impl Vm {
    fn json_enc(&mut self, e: &mut Enc, v: &Value) -> R<()> {
        match v {
            Value::Null => e.out.extend_from_slice(b"null"),
            Value::Bool(b) => e.out.extend_from_slice(if *b { b"true" } else { b"false" }),
            Value::Int(i) => e.out.extend_from_slice(i.to_string().as_bytes()),
            Value::Float(f) => {
                if !f.is_finite() {
                    e.error = 7;
                    e.out.push(b'0');
                    return Ok(());
                }
                let mut b = Vec::new();
                goro_runtime::float_fmt::format_php_repr(
                    *f,
                    self.ini.serialize_precision,
                    b'e',
                    &mut b,
                );
                if e.flags & 1024 != 0 && !b.iter().any(|c| matches!(c, b'.' | b'e')) {
                    b.extend_from_slice(b".0");
                }
                e.out.extend_from_slice(&b);
            }
            Value::Str(s) => {
                if e.flags & 32 != 0
                    && let Some(n) = goro_runtime::numeric::numeric_value(s)
                    && goro_runtime::numeric::is_numeric_str(s)
                {
                    let nv = match n {
                        goro_runtime::Number::Int(i) => Value::Int(i),
                        goro_runtime::Number::Float(f) => Value::Float(f),
                    };
                    // Out-of-range numbers keep their text (bug64695).
                    if !matches!(nv, Value::Float(f) if !f.is_finite()) {
                        return self.json_enc(e, &nv);
                    }
                }
                json_string(e, s)
            }
            Value::Array(a) => {
                let id = a.data() as *const _ as usize;
                if e.stack.contains(&id) {
                    e.error = 6;
                    e.out.extend_from_slice(b"null");
                    return Ok(());
                }
                let list = a.data().is_list() && e.flags & 16 == 0;
                if e.depth >= e.max_depth || e.depth >= MAX_NESTING {
                    e.error = 1;
                    if e.depth >= MAX_NESTING {
                        e.out.extend_from_slice(b"null");
                        return Ok(());
                    }
                }
                e.stack.push(id);
                e.depth += 1;
                let r = if list {
                    self.json_list(e, a)
                } else {
                    let pairs: Vec<(Value, Value)> =
                        a.iter().map(|(k, v)| (k.to_value(), v.clone())).collect();
                    self.json_object_pairs(e, pairs)
                };
                e.depth -= 1;
                e.stack.pop();
                r?;
            }
            Value::Object(o) => return self.json_object(e, o),
            Value::Resource(_) => {
                e.error = 8;
                e.out.extend_from_slice(b"null");
            }
            Value::Ref(r) => {
                let v = r.get();
                return self.json_enc(e, &v);
            }
        }
        Ok(())
    }

    fn json_nl(&self, e: &mut Enc) {
        if e.flags & PRETTY != 0 {
            e.out.push(b'\n');
            for _ in 0..e.depth {
                e.out.extend_from_slice(b"    ");
            }
        }
    }

    fn json_list(&mut self, e: &mut Enc, a: &PhpArray) -> R<()> {
        e.out.push(b'[');
        if a.is_empty() {
            e.out.push(b']');
            return Ok(());
        }
        let mut first = true;
        for (_, v) in a.iter() {
            if !first {
                e.out.push(b',');
            }
            first = false;
            self.json_nl(e);
            let v = v.clone();
            self.json_enc(e, &v)?;
        }
        e.depth -= 1;
        self.json_nl(e);
        e.depth += 1;
        e.out.push(b']');
        Ok(())
    }

    fn json_object_pairs(&mut self, e: &mut Enc, pairs: Vec<(Value, Value)>) -> R<()> {
        e.out.push(b'{');
        if pairs.is_empty() {
            e.out.push(b'}');
            return Ok(());
        }
        let mut first = true;
        for (k, v) in pairs {
            if !first {
                e.out.push(b',');
            }
            first = false;
            self.json_nl(e);
            let ks = match &k {
                Value::Str(s) => s.clone(),
                Value::Int(i) => PhpStr::from(i.to_string()),
                _ => PhpStr::empty(),
            };
            let mark = e.out.len();
            json_string(e, &ks);
            if e.out[mark..] == *b"null" {
                // An invalid key (partial output) is written as "" (bug68567).
                e.out.truncate(mark);
                e.out.extend_from_slice(b"\"\"");
            }
            e.out.push(b':');
            if e.flags & PRETTY != 0 {
                e.out.push(b' ');
            }
            self.json_enc(e, &v)?;
        }
        e.depth -= 1;
        self.json_nl(e);
        e.depth += 1;
        e.out.push(b'}');
        Ok(())
    }

    fn json_object(&mut self, e: &mut Enc, o: &ObjRef) -> R<()> {
        let cls = self.class_of(o);
        let id = (o.handle() as usize) | (1 << 62);
        if e.stack.contains(&id) || self.ext::<JsonState>().active.contains(&id) {
            e.error = 6;
            e.out.extend_from_slice(b"null");
            return Ok(());
        }
        if e.depth >= MAX_NESTING {
            e.error = 1;
            e.out.extend_from_slice(b"null");
            return Ok(());
        }
        if let Some(js) = self.find_class(b"JsonSerializable", false)?
            && cls.is_subclass_of(&js)
        {
            e.stack.push(id);
            self.ext::<JsonState>().active.push(id);
            let r = self.call_method(o, b"jsonserialize", Vec::new());
            self.ext::<JsonState>().active.retain(|x| *x != id);
            let r = match r {
                Ok(v) => v,
                Err(err) => {
                    e.stack.pop();
                    return Err(err);
                }
            };
            let res = if let Value::Object(o2) = &r
                && ObjRef::ptr_eq(o2, o)
            {
                // Returning $this serializes its properties.
                let props = self.visible_props(o);
                e.depth += 1;
                let r = self.json_object_pairs(e, props);
                e.depth -= 1;
                r
            } else {
                self.json_enc(e, &r)
            };
            e.stack.pop();
            return res;
        }
        if cls.is_enum() {
            if cls.enum_backing.is_none() {
                e.error = 11;
                e.out.push(b'0');
                return Ok(());
            }
            let v = self.get_prop_raw(o, b"value");
            return self.json_enc(e, &v);
        }
        if e.depth >= e.max_depth {
            e.error = 1;
        }
        let props = self.visible_props(o);
        e.stack.push(id);
        e.depth += 1;
        let r = self.json_object_pairs(e, props);
        e.depth -= 1;
        e.stack.pop();
        r
    }

    pub fn json_encode_value(
        &mut self,
        v: &Value,
        flags: i64,
        depth: usize,
    ) -> R<Result<Vec<u8>, i64>> {
        let mut e = Enc {
            out: Vec::new(),
            flags,
            depth: 0,
            max_depth: depth,
            error: 0,
            stack: Vec::new(),
        };
        self.json_enc(&mut e, v)?;
        if e.error != 0 && flags & PARTIAL == 0 {
            return Ok(Err(e.error));
        }
        if e.error != 0 {
            self.ext::<JsonState>().last_error = e.error;
            return Ok(Ok(e.out));
        }
        Ok(Ok(e.out))
    }
}

fn json_string(e: &mut Enc, s: &[u8]) {
    let flags = e.flags;
    // Validate UTF-8.
    let valid = std::str::from_utf8(s).is_ok();
    if !valid && flags & (1048576 | 2097152) == 0 {
        e.error = 5;
        e.out.extend_from_slice(b"null");
        return;
    }
    e.out.push(b'"');
    let text: String = if valid {
        String::from_utf8(s.to_vec()).unwrap()
    } else {
        // Invalid sequences are substituted or dropped unit by unit.
        let mut out: Vec<u8> = Vec::new();
        let mut i = 0;
        while i < s.len() {
            if s[i] < 0x80 {
                out.push(s[i]);
                i += 1;
                continue;
            }
            let (ok, n) = utf8_unit(s, i);
            if ok {
                out.extend_from_slice(&s[i..i + n]);
            } else if flags & 2097152 != 0 {
                out.extend_from_slice("\u{FFFD}".as_bytes());
            }
            i += n;
        }
        String::from_utf8(out).unwrap_or_default()
    };
    for c in text.chars() {
        match c {
            '"' => {
                if flags & 8 != 0 {
                    e.out.extend_from_slice(b"\\u0022");
                } else {
                    e.out.extend_from_slice(b"\\\"");
                }
            }
            '\\' => e.out.extend_from_slice(b"\\\\"),
            '/' => {
                if flags & 64 != 0 {
                    e.out.push(b'/');
                } else {
                    e.out.extend_from_slice(b"\\/");
                }
            }
            '\u{8}' => e.out.extend_from_slice(b"\\b"),
            '\u{c}' => e.out.extend_from_slice(b"\\f"),
            '\n' => e.out.extend_from_slice(b"\\n"),
            '\r' => e.out.extend_from_slice(b"\\r"),
            '\t' => e.out.extend_from_slice(b"\\t"),
            '<' if flags & 1 != 0 => e.out.extend_from_slice(b"\\u003C"),
            '>' if flags & 1 != 0 => e.out.extend_from_slice(b"\\u003E"),
            '&' if flags & 2 != 0 => e.out.extend_from_slice(b"\\u0026"),
            '\'' if flags & 4 != 0 => e.out.extend_from_slice(b"\\u0027"),
            c if (c as u32) < 0x20 => e
                .out
                .extend_from_slice(format!("\\u{:04x}", c as u32).as_bytes()),
            '\u{2028}' | '\u{2029}' if flags & 256 != 0 && flags & 2048 == 0 => {
                e.out
                    .extend_from_slice(format!("\\u{:04x}", c as u32).as_bytes());
            }
            c if (c as u32) < 0x80 => e.out.push(c as u8),
            c => {
                if flags & 256 != 0 {
                    let mut b = [0u8; 4];
                    e.out.extend_from_slice(c.encode_utf8(&mut b).as_bytes());
                } else {
                    let mut u = [0u16; 2];
                    for unit in c.encode_utf16(&mut u) {
                        e.out
                            .extend_from_slice(format!("\\u{:04x}", unit).as_bytes());
                    }
                }
            }
        }
    }
    e.out.push(b'"');
}

fn json_fail(vm: &mut Vm, code: i64, flags: i64) -> R<Value> {
    if flags & THROW != 0 {
        let e = vm.create_exception("JsonException", error_msg(code), code)?;
        return Err(crate::vm::Unwind::Throw(e));
    }
    vm.ext::<JsonState>().last_error = code;
    Ok(Value::Bool(false))
}

fn json_encode(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let v = a.val(0);
    let flags = a.int_or(1, 0);
    // Observed on PHP 8.5: no validation; 0 allows only scalars.
    let depth = a.int_or(2, 512).max(0);
    if flags & THROW == 0 {
        vm.ext::<JsonState>().last_error = 0;
    }
    match vm.json_encode_value(&v, flags, depth as usize)? {
        Ok(out) => Ok(Value::Str(PhpStr::from(out))),
        Err(code) => json_fail(vm, code, flags),
    }
}

// ---------------------------------------------------------------- decoding

struct Dec<'a> {
    s: &'a [u8],
    pos: usize,
    assoc: bool,
    bigint_string: bool,
    max_depth: usize,
    depth: usize,
    /// 0: error, 1: JSON_INVALID_UTF8_IGNORE, 2: _SUBSTITUTE.
    utf8: u8,
}

enum DErr {
    Code(i64),
    Php(crate::vm::Unwind),
}

impl From<crate::vm::Unwind> for DErr {
    fn from(u: crate::vm::Unwind) -> DErr {
        DErr::Php(u)
    }
}

impl Dec<'_> {
    fn ws(&mut self) {
        while self.pos < self.s.len() && matches!(self.s[self.pos], b' ' | b'\t' | b'\n' | b'\r') {
            self.pos += 1;
        }
    }
}

impl Vm {
    fn json_value(&mut self, d: &mut Dec, build: bool) -> Result<Value, DErr> {
        d.ws();
        let Some(&c) = d.s.get(d.pos) else {
            return Err(DErr::Code(4));
        };
        match c {
            b'{' => {
                d.pos += 1;
                d.depth += 1;
                if d.depth >= d.max_depth {
                    return Err(DErr::Code(1));
                }
                // The object exists before its members (object ids follow
                // PHP's order: test pass001).
                let early = if build && !d.assoc {
                    Some(self.new_std_object()?)
                } else {
                    None
                };
                let mut pairs: Vec<(PhpStr, Value)> = Vec::new();
                d.ws();
                if d.s.get(d.pos) == Some(&b'}') {
                    d.pos += 1;
                } else {
                    loop {
                        d.ws();
                        if d.s.get(d.pos) != Some(&b'"') {
                            return Err(DErr::Code(4));
                        }
                        let k = json_str(d)?;
                        d.ws();
                        if d.s.get(d.pos) != Some(&b':') {
                            return Err(DErr::Code(4));
                        }
                        d.pos += 1;
                        let v = self.json_value(d, build)?;
                        pairs.push((k, v));
                        d.ws();
                        match d.s.get(d.pos) {
                            Some(b',') => d.pos += 1,
                            Some(b'}') => {
                                d.pos += 1;
                                break;
                            }
                            Some(b']') => return Err(DErr::Code(2)),
                            _ => return Err(DErr::Code(4)),
                        }
                    }
                }
                d.depth -= 1;
                if !build {
                    return Ok(Value::Null);
                }
                if d.assoc {
                    let mut a = PhpArray::new();
                    for (k, v) in pairs {
                        a.insert(Key::from_php_str(&k), v);
                    }
                    return Ok(Value::Array(a));
                }
                let o = match early {
                    Some(o) => o,
                    None => self.new_std_object()?,
                };
                {
                    let mut props = o.obj().props.borrow_mut();
                    for (k, v) in pairs {
                        if k.first() == Some(&0) {
                            drop(props);
                            return Err(DErr::Code(9));
                        }
                        props.insert(Key::Str(k), v);
                    }
                }
                Ok(Value::Object(o))
            }
            b'[' => {
                d.pos += 1;
                d.depth += 1;
                if d.depth >= d.max_depth {
                    return Err(DErr::Code(1));
                }
                let mut a = PhpArray::new();
                d.ws();
                if d.s.get(d.pos) == Some(&b']') {
                    d.pos += 1;
                } else {
                    loop {
                        let v = self.json_value(d, build)?;
                        if build {
                            let _ = a.push(v);
                        }
                        d.ws();
                        match d.s.get(d.pos) {
                            Some(b',') => d.pos += 1,
                            Some(b']') => {
                                d.pos += 1;
                                break;
                            }
                            Some(b'}') => return Err(DErr::Code(2)),
                            _ => return Err(DErr::Code(4)),
                        }
                    }
                }
                d.depth -= 1;
                Ok(Value::Array(a))
            }
            b'"' => Ok(Value::Str(json_str(d)?)),
            b't' if d.s[d.pos..].starts_with(b"true") => {
                d.pos += 4;
                Ok(Value::Bool(true))
            }
            b'f' if d.s[d.pos..].starts_with(b"false") => {
                d.pos += 5;
                Ok(Value::Bool(false))
            }
            b'n' if d.s[d.pos..].starts_with(b"null") => {
                d.pos += 4;
                Ok(Value::Null)
            }
            b'-' | b'0'..=b'9' => json_num(d),
            0 => Err(DErr::Code(3)),
            _ => Err(DErr::Code(4)),
        }
    }
}

fn json_num(d: &mut Dec) -> Result<Value, DErr> {
    let start = d.pos;
    let s = d.s;
    if s[d.pos] == b'-' {
        d.pos += 1;
    }
    let int_start = d.pos;
    if d.pos < s.len() && s[d.pos] == b'0' {
        d.pos += 1;
    } else if d.pos < s.len() && s[d.pos].is_ascii_digit() {
        while d.pos < s.len() && s[d.pos].is_ascii_digit() {
            d.pos += 1;
        }
    } else {
        return Err(DErr::Code(4));
    }
    let _ = int_start;
    let mut is_float = false;
    if d.pos < s.len() && s[d.pos] == b'.' {
        is_float = true;
        d.pos += 1;
        if d.pos >= s.len() || !s[d.pos].is_ascii_digit() {
            return Err(DErr::Code(4));
        }
        while d.pos < s.len() && s[d.pos].is_ascii_digit() {
            d.pos += 1;
        }
    }
    if d.pos < s.len() && (s[d.pos] == b'e' || s[d.pos] == b'E') {
        is_float = true;
        d.pos += 1;
        if d.pos < s.len() && (s[d.pos] == b'+' || s[d.pos] == b'-') {
            d.pos += 1;
        }
        if d.pos >= s.len() || !s[d.pos].is_ascii_digit() {
            return Err(DErr::Code(4));
        }
        while d.pos < s.len() && s[d.pos].is_ascii_digit() {
            d.pos += 1;
        }
    }
    let txt = std::str::from_utf8(&s[start..d.pos]).unwrap();
    if !is_float {
        if let Ok(i) = txt.parse::<i64>() {
            return Ok(Value::Int(i));
        }
        if d.bigint_string {
            return Ok(Value::Str(PhpStr::from(txt)));
        }
    }
    Ok(Value::Float(txt.parse::<f64>().unwrap_or(0.0)))
}

fn json_str(d: &mut Dec) -> Result<PhpStr, DErr> {
    d.pos += 1;
    let mut out: Vec<u8> = Vec::new();
    let s = d.s;
    loop {
        let Some(&c) = s.get(d.pos) else {
            return Err(DErr::Code(4));
        };
        match c {
            b'"' => {
                d.pos += 1;
                break;
            }
            b'\\' => {
                let Some(&e) = s.get(d.pos + 1) else {
                    return Err(DErr::Code(4));
                };
                d.pos += 2;
                match e {
                    b'"' => out.push(b'"'),
                    b'\\' => out.push(b'\\'),
                    b'/' => out.push(b'/'),
                    b'b' => out.push(8),
                    b'f' => out.push(12),
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'u' => {
                        let hex = |p: usize| -> Option<u32> {
                            let h = s.get(p..p + 4)?;
                            u32::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok()
                        };
                        let Some(u) = hex(d.pos) else {
                            return Err(DErr::Code(4));
                        };
                        d.pos += 4;
                        let cp = if (0xD800..0xDC00).contains(&u) {
                            if s.get(d.pos) == Some(&b'\\') && s.get(d.pos + 1) == Some(&b'u') {
                                let Some(l) = hex(d.pos + 2) else {
                                    return Err(DErr::Code(4));
                                };
                                if (0xDC00..0xE000).contains(&l) {
                                    d.pos += 6;
                                    0x10000 + ((u - 0xD800) << 10) + (l - 0xDC00)
                                } else {
                                    return Err(DErr::Code(10));
                                }
                            } else {
                                return Err(DErr::Code(10));
                            }
                        } else if (0xDC00..0xE000).contains(&u) {
                            return Err(DErr::Code(10));
                        } else {
                            u
                        };
                        let ch = char::from_u32(cp).unwrap_or('\u{FFFD}');
                        let mut b = [0u8; 4];
                        out.extend_from_slice(ch.encode_utf8(&mut b).as_bytes());
                    }
                    _ => return Err(DErr::Code(4)),
                }
            }
            c if c < 0x20 => return Err(DErr::Code(3)),
            c if c >= 0x80 => {
                // The decoder replaces each maximal invalid subpart
                // (observed on PHP 8.5: "\xf0\x80\x80" is three U+FFFD).
                let end = (d.pos + 4).min(s.len());
                let (ok, n) = match std::str::from_utf8(&s[d.pos..end]) {
                    Ok(_) => (true, utf8_len(c)),
                    Err(err) if err.valid_up_to() > 0 => (true, utf8_len(c)),
                    Err(err) => (false, err.error_len().unwrap_or(end - d.pos).max(1)),
                };
                if ok {
                    out.extend_from_slice(&s[d.pos..d.pos + n]);
                } else {
                    match d.utf8 {
                        1 => {}
                        2 => out.extend_from_slice("\u{FFFD}".as_bytes()),
                        _ => return Err(DErr::Code(5)),
                    }
                }
                d.pos += n;
            }
            c => {
                out.push(c);
                d.pos += 1;
            }
        }
    }
    if std::str::from_utf8(&out).is_err() {
        return Err(DErr::Code(5));
    }
    Ok(PhpStr::from(out))
}

fn json_decode_impl(vm: &mut Vm, a: &Args, build: bool) -> R<Result<Value, i64>> {
    let s = a.str(0);
    let (assoc, depth, flags) = if build {
        let assoc = match a.get(1) {
            Some(Value::Bool(b)) => *b,
            _ => a.int_or(3, 0) & 1 != 0,
        };
        (assoc, a.int_or(2, 512), a.int_or(3, 0))
    } else {
        (false, a.int_or(1, 512), a.int_or(2, 0))
    };
    // Observed on PHP 8.5: an empty string fails before the depth check.
    if s.is_empty() {
        return Ok(Err(4));
    }
    if depth <= 0 {
        let n = if build { 3 } else { 2 };
        return vm.fthrow(
            a,
            "ValueError",
            &format!("Argument #{} ($depth) must be greater than 0", n),
        );
    }
    if depth > i32::MAX as i64 {
        let n = if build { 3 } else { 2 };
        return vm.fthrow(
            a,
            "ValueError",
            &format!("Argument #{} ($depth) must be less than {}", n, i32::MAX),
        );
    }
    if s.is_empty() {
        return Ok(Err(4));
    }
    if std::str::from_utf8(&s).is_err() && flags & (1048576 | 2097152) == 0 {
        return Ok(Err(5));
    }
    let mut d = Dec {
        s: &s,
        pos: 0,
        assoc,
        bigint_string: flags & 2 != 0,
        max_depth: depth as usize,
        depth: 0,
        utf8: if flags & 2097152 != 0 {
            2
        } else if flags & 1048576 != 0 {
            1
        } else {
            0
        },
    };
    match vm.json_value(&mut d, build) {
        Ok(v) => {
            d.ws();
            if d.pos < s.len() {
                return Ok(Err(4));
            }
            Ok(Ok(v))
        }
        Err(DErr::Code(c)) => Ok(Err(c)),
        Err(DErr::Php(u)) => Err(u),
    }
}

fn json_decode(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let flags = a.int_or(3, 0);
    if flags & THROW == 0 {
        vm.ext::<JsonState>().last_error = 0;
    }
    match json_decode_impl(vm, a, true)? {
        Ok(v) => Ok(v),
        Err(code) => {
            if flags & THROW != 0 {
                let e = vm.create_exception("JsonException", error_msg(code), code)?;
                return Err(crate::vm::Unwind::Throw(e));
            }
            vm.ext::<JsonState>().last_error = code;
            Ok(Value::Null)
        }
    }
}

fn json_validate(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let flags = a.int_or(2, 0);
    if flags != 0 && flags != 1048576 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #3 ($flags) must be a valid flag (allowed flags: JSON_INVALID_UTF8_IGNORE)",
        );
    }
    match json_decode_impl(vm, a, false)? {
        Ok(_) => {
            vm.ext::<JsonState>().last_error = 0;
            Ok(Value::Bool(true))
        }
        Err(code) => {
            vm.ext::<JsonState>().last_error = code;
            Ok(Value::Bool(false))
        }
    }
}

fn json_last_error(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Int(vm.ext::<JsonState>().last_error))
}

fn json_last_error_msg(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let c = vm.ext::<JsonState>().last_error;
    Ok(Value::from(error_msg(c)))
}

#[allow(dead_code)]
fn unused(_: KeyRef<'_>, v: &Value) -> String {
    format!("{}{}", v.tname(), PhpStr::empty().dname())
}
