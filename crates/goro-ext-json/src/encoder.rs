//! Port of ext/json/json_encoder.c.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use goro_core::array::ArrayKey;
use goro_core::object::PhpObject;
use goro_core::value::{NumStr, Value};
use goro_core::vm::Vm;

use crate::*;

/// php_json_encoder
pub struct Encoder {
    pub depth: i32,
    pub max_depth: i32,
    pub error_code: i64,
}

impl Encoder {
    pub fn new(max_depth: i32) -> Self {
        Encoder {
            depth: 0,
            max_depth,
            error_code: ERROR_NONE,
        }
    }
}

std::thread_local! {
    /// Arrays and objects being encoded (GC_PROTECT_RECURSION), across nested
    /// json_encode() calls.
    static PROTECTED: RefCell<HashSet<usize>> = RefCell::new(HashSet::new());
}

/// Emulated C stack a php_json_encode_array() + php_json_encode_zval()
/// level uses (php-src gets ~21.6k levels deep in an 8 MiB stack).
const ENCODE_LEVEL_COST: usize = 385;

fn is_protected(id: usize) -> bool {
    PROTECTED.with(|p| p.borrow().contains(&id))
}
fn protect(id: usize) {
    PROTECTED.with(|p| p.borrow_mut().insert(id));
}
fn unprotect(id: usize) {
    PROTECTED.with(|p| p.borrow_mut().remove(&id));
}

fn pretty_print_char(buf: &mut Vec<u8>, options: i64, c: u8) {
    if options & PRETTY_PRINT != 0 {
        buf.push(c);
    }
}

fn pretty_print_indent(buf: &mut Vec<u8>, options: i64, enc: &Encoder) {
    if options & PRETTY_PRINT != 0 {
        for _ in 0..enc.depth {
            buf.extend_from_slice(b"    ");
        }
    }
}

fn is_valid_double(d: f64) -> bool {
    d.is_finite()
}

/// zend_gcvt(value, ndigit, '.', 'e')
pub fn gcvt(value: f64, ndigit: i32) -> Vec<u8> {
    goro_core::value::zend_gcvt(value, ndigit, b'.', b'e')
}

fn encode_double(buf: &mut Vec<u8>, d: f64, options: i64) {
    let mut num = gcvt(d, goro_core::value::get_php_serialize_precision());
    if options & PRESERVE_ZERO_FRACTION != 0 && !num.contains(&b'.') {
        num.extend_from_slice(b".0");
    }
    buf.extend_from_slice(&num);
}

/// php_next_utf8_char(): (code point, bytes consumed, valid).
fn next_utf8_char(s: &[u8]) -> (u32, usize, bool) {
    let lead = |c: u8| c < 0x80 || (0xC2..=0xF4).contains(&c);
    let trail = |c: u8| (0x80..=0xBF).contains(&c);
    let avail = s.len();
    let c = s[0];
    if c < 0x80 {
        return (c as u32, 1, true);
    }
    if c < 0xc2 {
        return (0, 1, false);
    }
    if c < 0xe0 {
        if avail < 2 {
            return (0, 1, false);
        }
        if !trail(s[1]) {
            return (0, if lead(s[1]) { 1 } else { 2 }, false);
        }
        let ch = ((c as u32 & 0x1f) << 6) | (s[1] as u32 & 0x3f);
        if ch < 0x80 {
            return (0, 2, false);
        }
        return (ch, 2, true);
    }
    if c < 0xf0 {
        if avail < 3 || !trail(s[1]) || !trail(s[2]) {
            let n = if avail < 2 || lead(s[1]) {
                1
            } else if avail < 3 || lead(s[2]) {
                2
            } else {
                3
            };
            return (0, n, false);
        }
        let ch = ((c as u32 & 0x0f) << 12) | ((s[1] as u32 & 0x3f) << 6) | (s[2] as u32 & 0x3f);
        if ch < 0x800 || (0xd800..=0xdfff).contains(&ch) {
            return (0, 3, false);
        }
        return (ch, 3, true);
    }
    if c < 0xf5 {
        if avail < 4 || !trail(s[1]) || !trail(s[2]) || !trail(s[3]) {
            let n = if avail < 2 || lead(s[1]) {
                1
            } else if avail < 3 || lead(s[2]) {
                2
            } else if avail < 4 || lead(s[3]) {
                3
            } else {
                4
            };
            return (0, n, false);
        }
        let ch = ((c as u32 & 0x07) << 18)
            | ((s[1] as u32 & 0x3f) << 12)
            | ((s[2] as u32 & 0x3f) << 6)
            | (s[3] as u32 & 0x3f);
        if !(0x10000..=0x10FFFF).contains(&ch) {
            return (0, 4, false);
        }
        return (ch, 4, true);
    }
    (0, 1, false)
}

const DIGITS: &[u8; 16] = b"0123456789abcdef";

fn push_u16_escape(buf: &mut Vec<u8>, us: u32) {
    buf.extend_from_slice(b"\\u");
    buf.push(DIGITS[((us >> 12) & 0xf) as usize]);
    buf.push(DIGITS[((us >> 8) & 0xf) as usize]);
    buf.push(DIGITS[((us >> 4) & 0xf) as usize]);
    buf.push(DIGITS[(us & 0xf) as usize]);
}

/// php_json_escape_string(): false on failure.
pub fn escape_string(buf: &mut Vec<u8>, s: &[u8], options: i64, enc: &mut Encoder) -> bool {
    if s.is_empty() {
        buf.extend_from_slice(b"\"\"");
        return true;
    }
    if options & NUMERIC_CHECK != 0 {
        match goro_core::value::parse_numeric_str(s, false) {
            Some((NumStr::Long(p), _)) => {
                buf.extend_from_slice(p.to_string().as_bytes());
                return true;
            }
            Some((NumStr::Double(d), _)) if is_valid_double(d) => {
                encode_double(buf, d, options);
                return true;
            }
            _ => {}
        }
    }
    let checkpoint = buf.len();
    buf.reserve(s.len() + 2);
    buf.push(b'"');
    let mut i = 0;
    while i < s.len() {
        let us = s[i];
        if us >= 0x80 {
            let (cp, n, ok) = next_utf8_char(&s[i..]);
            if !ok {
                if options & INVALID_UTF8_IGNORE != 0 {
                    // ignore invalid UTF8 character
                } else if options & INVALID_UTF8_SUBSTITUTE != 0 {
                    if options & UNESCAPED_UNICODE != 0 {
                        buf.extend_from_slice(b"\xef\xbf\xbd");
                    } else {
                        buf.extend_from_slice(b"\\ufffd");
                    }
                } else {
                    buf.truncate(checkpoint);
                    enc.error_code = ERROR_UTF8;
                    if options & PARTIAL_OUTPUT_ON_ERROR != 0 {
                        buf.extend_from_slice(b"null");
                    }
                    return false;
                }
            } else if options & UNESCAPED_UNICODE != 0
                && (options & UNESCAPED_LINE_TERMINATORS != 0 || !(0x2028..=0x2029).contains(&cp))
            {
                buf.extend_from_slice(&s[i..i + n]);
            } else if cp >= 0x10000 {
                let c = cp - 0x10000;
                push_u16_escape(buf, (c >> 10) | 0xd800);
                push_u16_escape(buf, (c & 0x3ff) | 0xdc00);
            } else {
                push_u16_escape(buf, cp);
            }
            i += n;
            continue;
        }
        i += 1;
        match us {
            b'"' => {
                if options & HEX_QUOT != 0 {
                    buf.extend_from_slice(b"\\u0022");
                } else {
                    buf.extend_from_slice(b"\\\"");
                }
            }
            b'\\' => buf.extend_from_slice(b"\\\\"),
            b'/' => {
                if options & UNESCAPED_SLASHES != 0 {
                    buf.push(b'/');
                } else {
                    buf.extend_from_slice(b"\\/");
                }
            }
            0x08 => buf.extend_from_slice(b"\\b"),
            0x0c => buf.extend_from_slice(b"\\f"),
            b'\n' => buf.extend_from_slice(b"\\n"),
            b'\r' => buf.extend_from_slice(b"\\r"),
            b'\t' => buf.extend_from_slice(b"\\t"),
            b'<' if options & HEX_TAG != 0 => buf.extend_from_slice(b"\\u003C"),
            b'>' if options & HEX_TAG != 0 => buf.extend_from_slice(b"\\u003E"),
            b'&' if options & HEX_AMP != 0 => buf.extend_from_slice(b"\\u0026"),
            b'\'' if options & HEX_APOS != 0 => buf.extend_from_slice(b"\\u0027"),
            c if c < b' ' => push_u16_escape(buf, c as u32),
            c => buf.push(c),
        }
    }
    buf.push(b'"');
    true
}

fn class_lower(obj: &Rc<RefCell<PhpObject>>) -> Vec<u8> {
    obj.borrow().class_name.to_ascii_lowercase()
}

fn has_exception(vm: &Vm) -> bool {
    vm.current_exception.is_some()
}

/// A property table entry: its value, or a hooked property read lazily.
enum PropVal {
    Val(Value),
    Hook(Vec<u8>, Value),
}

/// zend_get_properties_for(obj, ZEND_PROP_PURPOSE_JSON) and the object
/// whose table it is (the recursion guard).
fn json_properties(
    vm: &mut Vm,
    obj: &Rc<RefCell<PhpObject>>,
) -> Option<(Vec<(ArrayKey, PropVal)>, usize)> {
    let id = Rc::as_ptr(obj) as usize;
    if obj.borrow().is_closure() {
        return Some((Vec::new(), id));
    }
    if let Some(f) = goro_core::vm::native_handlers(obj).and_then(|h| h.get_properties) {
        if let Some(t) = f(vm, obj, 1) {
            let v = t
                .iter()
                .map(|(k, v)| (k.clone(), PropVal::Val(v.clone())))
                .collect();
            return Some((v, id));
        }
    }
    // A lazy object is initialized first; a proxy shows its instance.
    vm.maybe_run_lazy_init(obj);
    if has_exception(vm) {
        return None;
    }
    let target = vm.proxy_redirect(obj);
    let id = Rc::as_ptr(&target) as usize;
    let cl = class_lower(&target);
    if vm.class_has_hooked_props(&cl) {
        let mut out = Vec::new();
        for e in vm.object_prop_table_info_order(&target) {
            let key = ArrayKey::String(goro_core::string::PhpString::from_vec(e.key));
            match e.slot {
                Some(slot) if slot.has_get_hook => {
                    out.push((key, PropVal::Hook(slot.name, e.value)));
                }
                _ => out.push((key, PropVal::Val(e.value))),
            }
        }
        return Some((out, id));
    }
    let arr = vm.object_to_array(&target);
    let v = arr
        .borrow()
        .iter()
        .map(|(k, v)| (k.clone(), PropVal::Val(v.clone())))
        .collect();
    Some((v, id))
}

/// php_json_encode_array(): arrays and plain objects.
pub fn encode_array(
    vm: &mut Vm,
    buf: &mut Vec<u8>,
    val: &Value,
    options: i64,
    enc: &mut Encoder,
) -> bool {
    // php_json_check_stack_limit()
    goro_core::stack::native_level(ENCODE_LEVEL_COST, || {
        encode_array_level(vm, buf, val, options, enc)
    })
    .unwrap_or_else(|| {
        enc.error_code = ERROR_DEPTH;
        if options & PARTIAL_OUTPUT_ON_ERROR != 0 {
            buf.extend_from_slice(b"null");
        }
        false
    })
}

fn encode_array_level(
    vm: &mut Vm,
    buf: &mut Vec<u8>,
    val: &Value,
    options: i64,
    enc: &mut Encoder,
) -> bool {
    let is_object;
    let mut encode_as_object = options & FORCE_OBJECT != 0;
    let arr_rc;
    let entries;
    let recursion_id;
    match val {
        Value::Array(a) => {
            is_object = false;
            // (the zend_array it stands for: unmodified copies share it)
            recursion_id = (a.borrow().identity() | (1u64 << 63)) as usize;
            arr_rc = Some(a.clone());
            entries = None;
            let b = a.borrow();
            let is_list = b
                .iter()
                .enumerate()
                .all(|(i, (k, _))| matches!(k, ArrayKey::Int(n) if *n == i as i64));
            encode_as_object = encode_as_object || !is_list;
        }
        Value::Object(o) => {
            is_object = true;
            let Some((e, id)) = json_properties(vm, o) else {
                return false;
            };
            recursion_id = id;
            arr_rc = None;
            entries = Some(e);
            encode_as_object = true;
        }
        Value::Generator(g) => {
            is_object = true;
            recursion_id = Rc::as_ptr(g) as *const u8 as usize;
            arr_rc = None;
            entries = Some(Vec::new());
            encode_as_object = true;
        }
        _ => return false,
    }

    if is_protected(recursion_id) {
        enc.error_code = ERROR_RECURSION;
        buf.extend_from_slice(b"null");
        return false;
    }
    protect(recursion_id);

    buf.push(if encode_as_object { b'{' } else { b'[' });
    enc.depth += 1;

    let mut need_comma = false;
    let len = match (&arr_rc, &entries) {
        (Some(a), _) => a.borrow().len(),
        (None, Some(e)) => e.len(),
        _ => 0,
    };
    let mut entries = entries.map(|e| e.into_iter());
    for pos in 0..len {
        let (key, pv) = match (&arr_rc, &mut entries) {
            (Some(a), _) => {
                let b = a.borrow();
                match b.entry_at(pos) {
                    Some((k, v)) => (k.clone(), PropVal::Val(v.clone())),
                    None => break,
                }
            }
            (None, Some(it)) => match it.next() {
                Some(e) => e,
                None => break,
            },
            _ => break,
        };
        let data = if !encode_as_object {
            if need_comma {
                buf.push(b',');
            } else {
                need_comma = true;
            }
            pretty_print_char(buf, options, b'\n');
            pretty_print_indent(buf, options, enc);
            match pv {
                PropVal::Val(v) => v,
                PropVal::Hook(_, v) => v,
            }
        } else {
            match &key {
                ArrayKey::String(k) => {
                    let kb = k.as_bytes();
                    if is_object && kb.first() == Some(&0) {
                        // Skip protected and private members.
                        continue;
                    }
                    let data = match pv {
                        PropVal::Val(v) => v,
                        PropVal::Hook(name, backing) => {
                            match vm.call_get_hook_if_accessible(
                                match val {
                                    Value::Object(o) => o,
                                    _ => unreachable!(),
                                },
                                &name,
                            ) {
                                Some(Ok(v)) => v,
                                Some(Err(_)) => {
                                    unprotect(recursion_id);
                                    return false;
                                }
                                None => {
                                    if matches!(backing, Value::Undef) {
                                        continue;
                                    }
                                    backing
                                }
                            }
                        }
                    };
                    if has_exception(vm) {
                        unprotect(recursion_id);
                        return false;
                    }
                    if need_comma {
                        buf.push(b',');
                    } else {
                        need_comma = true;
                    }
                    pretty_print_char(buf, options, b'\n');
                    pretty_print_indent(buf, options, enc);
                    if !escape_string(buf, kb, options & !NUMERIC_CHECK, enc)
                        && options & PARTIAL_OUTPUT_ON_ERROR != 0
                    {
                        buf.truncate(buf.len() - 4);
                        buf.extend_from_slice(b"\"\"");
                    }
                    data
                }
                ArrayKey::Int(index) => {
                    if need_comma {
                        buf.push(b',');
                    } else {
                        need_comma = true;
                    }
                    pretty_print_char(buf, options, b'\n');
                    pretty_print_indent(buf, options, enc);
                    buf.push(b'"');
                    buf.extend_from_slice(index.to_string().as_bytes());
                    buf.push(b'"');
                    match pv {
                        PropVal::Val(v) => v,
                        PropVal::Hook(_, v) => v,
                    }
                }
            }
        };
        if encode_as_object {
            buf.push(b':');
            pretty_print_char(buf, options, b' ');
        }
        if !encode_zval(vm, buf, &data, options, enc) && options & PARTIAL_OUTPUT_ON_ERROR == 0 {
            unprotect(recursion_id);
            return false;
        }
    }

    unprotect(recursion_id);
    if enc.depth > enc.max_depth {
        enc.error_code = ERROR_DEPTH;
        if options & PARTIAL_OUTPUT_ON_ERROR == 0 {
            return false;
        }
    }
    enc.depth -= 1;

    // Only keep closing bracket on same line for empty arrays/objects
    if need_comma {
        pretty_print_char(buf, options, b'\n');
        pretty_print_indent(buf, options, enc);
    }
    buf.push(if encode_as_object { b'}' } else { b']' });
    true
}

/// php_json_encode_serializable_object()
fn encode_serializable_object(
    vm: &mut Vm,
    buf: &mut Vec<u8>,
    val: &Value,
    obj: &Rc<RefCell<PhpObject>>,
    options: i64,
    enc: &mut Encoder,
) -> bool {
    let guard = Rc::as_ptr(obj) as usize;
    if vm.json_encoding_objects.contains(&guard) {
        enc.error_code = ERROR_RECURSION;
        if options & PARTIAL_OUTPUT_ON_ERROR != 0 {
            buf.extend_from_slice(b"null");
        }
        return false;
    }
    vm.json_encoding_objects.insert(guard);
    let retval = vm.call_object_method(val, b"jsonSerialize", &[]);
    let retval = match retval {
        Some(v) if !has_exception(vm) => v,
        // An exception has occurred
        _ => {
            if options & PARTIAL_OUTPUT_ON_ERROR != 0 {
                buf.extend_from_slice(b"null");
            }
            vm.json_encoding_objects.remove(&guard);
            return false;
        }
    };
    let retval = retval.deref();
    if matches!(&retval, Value::Object(o) if Rc::ptr_eq(o, obj)) {
        // jsonSerialize() returned $this: encode its properties
        vm.json_encoding_objects.remove(&guard);
        encode_array(vm, buf, &retval, options, enc)
    } else {
        let r = encode_zval(vm, buf, &retval, options, enc);
        vm.json_encoding_objects.remove(&guard);
        r
    }
}

/// php_json_encode_serializable_enum()
fn encode_serializable_enum(
    vm: &mut Vm,
    buf: &mut Vec<u8>,
    obj: &Rc<RefCell<PhpObject>>,
    options: i64,
    enc: &mut Encoder,
) -> bool {
    let value = {
        let b = obj.borrow();
        if !b.has_property(b"__enum_backing_type") && !b.has_property(b"value") {
            None
        } else {
            Some(b.get_property(b"value"))
        }
    };
    match value {
        None => {
            enc.error_code = ERROR_NON_BACKED_ENUM;
            buf.push(b'0');
            false
        }
        Some(v) => encode_zval(vm, buf, &v, options, enc),
    }
}

fn is_json_serializable(vm: &Vm, val: &Value) -> bool {
    vm.obj_instanceof(val, b"jsonserializable")
}

/// php_json_encode_zval()
pub fn encode_zval(
    vm: &mut Vm,
    buf: &mut Vec<u8>,
    val: &Value,
    options: i64,
    enc: &mut Encoder,
) -> bool {
    match val {
        Value::Null | Value::Undef => buf.extend_from_slice(b"null"),
        Value::True => buf.extend_from_slice(b"true"),
        Value::False => buf.extend_from_slice(b"false"),
        Value::Long(n) => buf.extend_from_slice(n.to_string().as_bytes()),
        Value::Double(d) => {
            if is_valid_double(*d) {
                encode_double(buf, *d, options);
            } else {
                enc.error_code = ERROR_INF_OR_NAN;
                buf.push(b'0');
            }
        }
        Value::String(s) => return escape_string(buf, s.as_bytes(), options, enc),
        Value::Object(o) => {
            if is_json_serializable(vm, val) {
                return encode_serializable_object(vm, buf, val, o, options, enc);
            }
            if o.borrow().has_property(b"__enum_case") {
                return encode_serializable_enum(vm, buf, o, options, enc);
            }
            return encode_array(vm, buf, val, options, enc);
        }
        Value::Array(_) | Value::Generator(_) => return encode_array(vm, buf, val, options, enc),
        Value::Reference(r) => {
            let v = r.borrow().clone();
            return encode_zval(vm, buf, &v, options, enc);
        }
        _ => {
            enc.error_code = ERROR_UNSUPPORTED_TYPE;
            if options & PARTIAL_OUTPUT_ON_ERROR != 0 {
                buf.extend_from_slice(b"null");
            }
            return false;
        }
    }
    true
}
