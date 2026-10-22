//! ext/tokenizer: token_get_all() and PhpToken, ported from
//! php-src ext/tokenizer/tokenizer.c.
//!
//! Tokens come from the re2c-equivalent scanner (`php_scanner`). With
//! TOKEN_PARSE the source is also run through php-src's grammar
//! (`php_recognizer`): lexer diagnostics are raised, a syntax error throws
//! ParseError, and semi-reserved keywords the grammar uses as identifiers
//! are reported as T_STRING (the parser's ON_FEEDBACK event).

use crate::array::PhpArray;
use crate::object::PhpObject;
use crate::string::PhpString;
use crate::value::Value;
use crate::vm::Vm;
use goro_parser::php_scanner::{self as ps, Scanner};
use std::cell::RefCell;
use std::rc::Rc;

pub const TOKEN_PARSE: i64 = 1;

/// One token: id (a byte value below 256 for single-character tokens),
/// byte range in the source, and line.
#[derive(Clone, Copy, Debug)]
pub struct Tok {
    pub id: i64,
    pub start: usize,
    pub end: usize,
    pub line: u32,
}

/// tokenize(): the scanner's tokens, with everything after
/// `__halt_compiler();` returned as one T_INLINE_HTML.
fn scan(src: &[u8]) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut sc = Scanner::new(src, goro_parser::short_open_tag(), false);
    let mut need_tokens: i32 = -1;
    while let Some(t) = sc.next_token() {
        out.push(Tok {
            id: t.id as i64,
            start: t.start,
            end: t.end,
            line: t.line,
        });
        if need_tokens != -1 {
            if !matches!(
                t.id,
                ps::T_WHITESPACE | ps::T_OPEN_TAG | ps::T_COMMENT | ps::T_DOC_COMMENT
            ) {
                need_tokens -= 1;
                if need_tokens == 0 {
                    if t.end < src.len() {
                        out.push(Tok {
                            id: ps::T_INLINE_HTML as i64,
                            start: t.end,
                            end: src.len(),
                            line: t.line,
                        });
                    }
                    break;
                }
            }
        } else if t.id == ps::T_HALT_COMPILER {
            need_tokens = 3;
        }
    }
    out
}

/// Throw `class` with `msg` at `line` of the (nameless) tokenized code.
fn throw_at(vm: &mut Vm, class: &[u8], msg: &str, line: u32) {
    let exc = vm.create_exception(class, msg, line);
    if let Value::Object(o) = &exc {
        let mut o = o.borrow_mut();
        o.set_property(b"file".to_vec(), Value::String(PhpString::empty()));
        o.set_property(b"line".to_vec(), Value::Long(line as i64));
    }
    vm.current_exception = Some(exc);
}

/// tokenize_common(): the tokens, or None when an exception was thrown.
pub fn tokenize(vm: &mut Vm, src: &[u8], flags: i64) -> Option<Vec<Tok>> {
    if flags & TOKEN_PARSE == 0 {
        return Some(scan(src));
    }
    // zendparse() (goro_parser::php_recognizer): php-src's grammar decides
    // the syntax errors and which keywords became identifiers; the
    // scanner's diagnostics (compiled file name "") come first, up to the
    // error.
    let parsed = goro_parser::php_recognizer::tokenize(src, true);
    let err_line = parsed.as_ref().err().map(|e| e.line);
    let mut diags: Vec<(bool, String, u32)> = goro_parser::take_lexer_deprecations()
        .into_iter()
        .map(|(m, l)| (true, m, l))
        .chain(
            goro_parser::take_lexer_warnings()
                .into_iter()
                .map(|(m, l)| (false, m, l)),
        )
        .collect();
    diags.sort_by_key(|(_, _, l)| *l);
    for (deprecation, msg, line) in diags {
        if err_line.is_some_and(|el| line > el) {
            break;
        }
        let saved = std::mem::take(&mut vm.current_file);
        if deprecation {
            vm.emit_deprecated_at(&msg, line);
        } else {
            vm.emit_warning_at(&msg, line);
        }
        vm.current_file = saved;
        if vm.current_exception.is_some() {
            return None;
        }
    }
    match parsed {
        Ok(toks) => Some(
            toks.into_iter()
                .map(|(id, start, end, line)| Tok {
                    id: id as i64,
                    start,
                    end,
                    line,
                })
                .collect(),
        ),
        Err(e) => {
            throw_at(vm, b"ParseError", &e.message, e.line);
            None
        }
    }
}

fn text<'a>(src: &'a [u8], t: &Tok) -> &'a [u8] {
    if t.start < t.end && t.end <= src.len() {
        &src[t.start..t.end]
    } else {
        b""
    }
}

/// token_get_all(): arrays of [id, text, line] (plain strings for
/// single-character tokens).
pub fn token_get_all(vm: &mut Vm, src: &[u8], flags: i64) -> Option<Value> {
    let toks = tokenize(vm, src, flags)?;
    let mut result = PhpArray::new();
    for t in &toks {
        let s = Value::String(PhpString::from_bytes(text(src, t)));
        if t.id < 256 {
            result.push(s);
        } else {
            let mut a = PhpArray::new();
            a.push(Value::Long(t.id));
            a.push(s);
            a.push(Value::Long(t.line as i64));
            result.push(Value::Array(Rc::new(RefCell::new(a))));
        }
    }
    Some(Value::Array(Rc::new(RefCell::new(result))))
}

/// PhpToken::tokenize() called on `class_lower` (PhpToken or a subclass).
pub fn phptoken_tokenize(vm: &mut Vm, class_lower: &[u8], args: &[Value]) -> Option<Value> {
    let src = args.first().cloned().unwrap_or(Value::Null).to_php_string();
    let flags = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let is_base = class_lower == b"phptoken";
    let class_name = if is_base {
        b"PhpToken".to_vec()
    } else {
        let Some(ce) = vm.classes.get(class_lower) else {
            return Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
        };
        // Construction preconditions are checked once, up front.
        if ce.is_abstract && !ce.is_interface && !ce.is_trait && !ce.is_enum {
            let msg = format!(
                "Cannot instantiate abstract class {}",
                String::from_utf8_lossy(&ce.name)
            );
            let exc = vm.create_exception(b"Error", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return None;
        }
        ce.name.clone()
    };
    // zend_update_class_constants(): property defaults are evaluated
    // before any token is created.
    let mut defaults: Vec<(Vec<u8>, Value)> = Vec::new();
    if !is_base {
        let mut chain = Vec::new();
        let mut cur = class_lower.to_vec();
        while let Some(ce) = vm.classes.get(&cur) {
            chain.push(cur.clone());
            match &ce.parent {
                Some(p) => cur = p.to_ascii_lowercase(),
                None => break,
            }
            if chain.len() > 64 {
                break;
            }
        }
        for name in chain.iter().rev() {
            let props: Vec<(Vec<u8>, Value)> = vm
                .classes
                .get(name)
                .map(|ce| {
                    ce.properties
                        .iter()
                        .filter(|p| !p.is_static)
                        .map(|p| (p.name.clone(), p.default.clone()))
                        .collect()
                })
                .unwrap_or_default();
            for (n, d) in props {
                if let Value::String(s) = &d {
                    if let Some(c) = s.as_bytes().strip_prefix(b"__deferred_global_const__::") {
                        let short = c.rsplit(|&b| b == b'\\').next().unwrap_or(c);
                        if !vm.constants.contains_key(c) && !vm.constants.contains_key(short) {
                            let msg =
                                format!("Undefined constant \"{}\"", String::from_utf8_lossy(c));
                            let exc = vm.create_exception(b"Error", &msg, vm.current_line);
                            vm.current_exception = Some(exc);
                            return None;
                        }
                    }
                }
                let v = vm.resolve_deferred_value(&d);
                if vm.current_exception.is_some() {
                    return None;
                }
                match defaults.iter_mut().find(|(k, _)| *k == n) {
                    Some(slot) => slot.1 = v,
                    None => defaults.push((n, v)),
                }
            }
        }
    }
    let src_b = src.as_bytes();
    let toks = tokenize(vm, src_b, flags)?;
    let mut result = PhpArray::new();
    for t in &toks {
        let id = vm.alloc_object_id();
        let mut o = PhpObject::new(class_name.clone(), id);
        o.set_property(b"id".to_vec(), Value::Long(t.id));
        o.set_property(
            b"text".to_vec(),
            Value::String(PhpString::from_bytes(text(src_b, t))),
        );
        o.set_property(b"line".to_vec(), Value::Long(t.line as i64));
        o.set_property(b"pos".to_vec(), Value::Long(t.start as i64));
        for (n, v) in &defaults {
            if !matches!(n.as_slice(), b"id" | b"text" | b"line" | b"pos") {
                o.set_property(n.clone(), v.clone());
            }
        }
        result.push(Value::Object(Rc::new(RefCell::new(o))));
    }
    Some(Value::Array(Rc::new(RefCell::new(result))))
}

/// PhpToken::is(): None when an exception was thrown.
pub fn phptoken_is(vm: &mut Vm, obj: &Rc<RefCell<PhpObject>>, kind: &Value) -> Option<Value> {
    fn type_error(vm: &mut Vm, msg: String) -> Option<Value> {
        let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        None
    }
    fn prop(vm: &mut Vm, obj: &Rc<RefCell<PhpObject>>, name: &str) -> Option<Value> {
        let ob = obj.borrow();
        let v = if ob.has_property(name.as_bytes()) {
            ob.get_property(name.as_bytes())
        } else {
            Value::Undef
        };
        drop(ob);
        if matches!(v, Value::Undef) {
            let msg = format!(
                "Typed property PhpToken::${} must not be accessed before initialization",
                name
            );
            let exc = vm.create_exception(b"Error", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return None;
        }
        Some(v)
    }
    let b = |x: bool| if x { Value::True } else { Value::False };
    match kind.deref() {
        Value::Long(k) => {
            let id = prop(vm, obj, "id")?;
            Some(b(id.to_long() == k))
        }
        Value::String(s) => {
            let t = prop(vm, obj, "text")?;
            Some(b(t.to_php_string().as_bytes() == s.as_bytes()))
        }
        Value::Array(arr) => {
            let entries: Vec<Value> = arr.borrow().iter().map(|(_, v)| v.deref()).collect();
            let (mut id, mut txt) = (None, None);
            for e in entries {
                match &e {
                    Value::Long(k) => {
                        if id.is_none() {
                            id = Some(prop(vm, obj, "id")?.to_long());
                        }
                        if id == Some(*k) {
                            return Some(Value::True);
                        }
                    }
                    Value::String(s) => {
                        if txt.is_none() {
                            txt = Some(prop(vm, obj, "text")?.to_php_string());
                        }
                        if txt.as_ref().is_some_and(|t| t.as_bytes() == s.as_bytes()) {
                            return Some(Value::True);
                        }
                    }
                    other => {
                        return type_error(
                            vm,
                            format!(
                                "PhpToken::is(): Argument #1 ($kind) must only have elements of type string|int, {} given",
                                Vm::value_type_name(other)
                            ),
                        );
                    }
                }
            }
            Some(Value::False)
        }
        other => type_error(
            vm,
            format!(
                "PhpToken::is(): Argument #1 ($kind) must be of type string|int|array, {} given",
                Vm::value_type_name(&other)
            ),
        ),
    }
}
