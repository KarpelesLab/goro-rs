//! mb_convert_kana, mb_convert_variables (mbstring.c)

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

use crate::mbfl::Encoding;
use crate::mbfl::tables::translit_kana_jisx0201_jisx0208::*;
use crate::mbstring::*;
use crate::state;
use crate::zpp::*;

pub fn register(vm: &mut Vm) {
    vm.register_function(b"mb_convert_kana", mb_convert_kana);
    vm.register_function(b"mb_convert_variables", mb_convert_variables);
}

/* ===== mb_convert_kana ===== */

fn jp_kana_convert(input: &[u8], enc: &Encoding, mode: u32) -> Vec<u8> {
    use crate::mbfl::iso2022::mb_convert_kana_codepoint;
    let w = to_wchars(enc, input);
    let mut out: Vec<u32> = Vec::with_capacity(w.len());
    let n = w.len();
    let mut i = 0;
    while i < n {
        let next = if i + 1 < n { w[i + 1] } else { 0 };
        let mut consumed = false;
        let mut second = 0u32;
        out.push(mb_convert_kana_codepoint(
            w[i],
            next,
            &mut consumed,
            &mut second,
            mode,
        ));
        if second != 0 {
            out.push(second);
        }
        if consumed && i + 1 < n {
            i += 1;
        }
        i += 1;
    }
    from_wchars(enc, &out)
}

const KANA_FLAGS: [u8; 17] = *b"ARNSKHMCarnskhmcV";

fn mb_convert_kana(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "mb_convert_kana";
    let s = str_arg(vm, f, args, 0, "string")?;
    let opt: u32 = if present(args, 1) {
        let optstr = str_arg(vm, f, args, 1, "mode")?;
        let mut opt: u32 = 0;
        for &c in &optstr {
            if c == b'A' {
                opt |= (MBFL_HAN2ZEN_ALL | MBFL_HAN2ZEN_ALPHA | MBFL_HAN2ZEN_NUMERIC) as u32;
            } else if c == b'a' {
                opt |= (MBFL_ZEN2HAN_ALL | MBFL_ZEN2HAN_ALPHA | MBFL_ZEN2HAN_NUMERIC) as u32;
            } else if let Some(i) = KANA_FLAGS.iter().position(|&x| x == c) {
                opt |= 1 << i;
            } else {
                return Err(arg_value_error(
                    vm,
                    f,
                    2,
                    "mode",
                    &format!("contains invalid flag: '{}'", c as char),
                ));
            }
        }
        let bad = ((opt & 0xFF00) >> 8) & opt;
        if bad != 0 {
            let i = bad.trailing_zeros() as usize;
            let mut flag1 = KANA_FLAGS[i] as char;
            let mut flag2 = KANA_FLAGS[i + 8] as char;
            if (flag1 == 'R' || flag1 == 'N') && (opt & MBFL_HAN2ZEN_ALL as u32) != 0 {
                flag1 = 'A';
            }
            if (flag2 == 'r' || flag2 == 'n') && (opt & MBFL_ZEN2HAN_ALL as u32) != 0 {
                flag2 = 'a';
            }
            return Err(arg_value_error(
                vm,
                f,
                2,
                "mode",
                &format!("must not combine '{}' and '{}' flags", flag1, flag2),
            ));
        }
        if (opt & MBFL_HAN2ZEN_HIRAGANA as u32) != 0 && (opt & MBFL_HAN2ZEN_KATAKANA as u32) != 0 {
            return Err(arg_value_error(
                vm,
                f,
                2,
                "mode",
                "must not combine 'H' and 'K' flags",
            ));
        }
        if (opt & MBFL_ZEN2HAN_HIRAGANA as u32) != 0 {
            if (opt & MBFL_ZENKAKU_HIRA2KATA as u32) != 0 {
                return Err(arg_value_error(
                    vm,
                    f,
                    2,
                    "mode",
                    "must not combine 'h' and 'C' flags",
                ));
            } else if (opt & MBFL_ZENKAKU_KATA2HIRA as u32) != 0 {
                return Err(arg_value_error(
                    vm,
                    f,
                    2,
                    "mode",
                    "must not combine 'h' and 'c' flags",
                ));
            }
        } else if (opt & MBFL_ZEN2HAN_KATAKANA as u32) != 0 {
            if (opt & MBFL_ZENKAKU_HIRA2KATA as u32) != 0 {
                return Err(arg_value_error(
                    vm,
                    f,
                    2,
                    "mode",
                    "must not combine 'k' and 'C' flags",
                ));
            } else if (opt & MBFL_ZENKAKU_KATA2HIRA as u32) != 0 {
                return Err(arg_value_error(
                    vm,
                    f,
                    2,
                    "mode",
                    "must not combine 'k' and 'c' flags",
                ));
            }
        }
        opt
    } else {
        (MBFL_HAN2ZEN_KATAKANA | MBFL_HAN2ZEN_GLUE) as u32
    };
    let enc = encoding_arg(vm, f, args, 2, "encoding")?;
    Ok(pstring(jp_kana_convert(&s, enc, opt)))
}

/* ===== mb_convert_variables ===== */

/// C stack a level of mb_recursive_find_strings() /
/// mb_recursive_convert_variable() uses (for mb_check_stack_limit()).
const RECURSION_FRAME_COST: usize = 192;

/// The walk over the variables: the arrays and objects being visited
/// (GC_PROTECT_RECURSION) and the depth.
#[derive(Default)]
struct Walk {
    visiting: std::collections::HashSet<usize>,
    depth: usize,
    /// The stack limit was reached (an Error is to be thrown)
    overflow: bool,
}

impl Walk {
    /// mb_check_stack_limit()
    fn stack_exhausted(&mut self) -> bool {
        let max = goro_core::stack::emulated_size();
        if max > 0 && self.depth.saturating_mul(RECURSION_FRAME_COST) > max {
            self.overflow = true;
        }
        self.overflow
    }

    /// Run `f` one level deeper (on a fresh stack segment when needed).
    fn deeper<R>(&mut self, f: impl FnOnce(&mut Walk) -> R) -> Option<R> {
        self.depth += 1;
        let r = goro_core::stack::with_room(|| f(self)).ok();
        self.depth -= 1;
        r
    }
}

/// C: `mb_recursive_find_strings`. True on recursion.
fn collect_strings(v: &Value, out: &mut Vec<Vec<u8>>, w: &mut Walk) -> bool {
    if w.stack_exhausted() {
        return true;
    }
    let v = v.deref();
    let (key, vals): (usize, Vec<Value>) = match &v {
        Value::String(s) => {
            out.push(s.as_bytes().to_vec());
            return false;
        }
        Value::Array(a) => (
            Rc::as_ptr(a) as *const () as usize,
            a.borrow().values().cloned().collect(),
        ),
        Value::Object(o) => (
            Rc::as_ptr(o) as *const () as usize,
            o.borrow()
                .properties
                .iter()
                .map(|(_, v)| v.clone())
                .collect(),
        ),
        _ => return false,
    };
    if !w.visiting.insert(key) {
        return true;
    }
    let mut rec = false;
    for e in vals {
        if w.deeper(|w| collect_strings(&e, out, w)).unwrap_or(true) {
            rec = true;
            break;
        }
    }
    w.visiting.remove(&key);
    rec
}

/// C: `mb_recursive_convert_variable` on the slot holding `v`: the value
/// the slot gets (a string replaces a reference to a string), None on
/// recursion.
fn convert_value(v: &Value, from: &Encoding, to: &Encoding, w: &mut Walk) -> Option<Value> {
    if w.stack_exhausted() {
        return None;
    }
    let inner = v.deref();
    match &inner {
        Value::String(s) => Some(pstring(convert_ex(s.as_bytes(), to, from))),
        Value::Array(a) => {
            let key = Rc::as_ptr(a) as *const () as usize;
            if !w.visiting.insert(key) {
                return None;
            }
            let entries: Vec<(ArrayKey, Value)> = a
                .borrow()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            // SEPARATE_ARRAY(): the converted array
            let mut na = PhpArray::new();
            let mut ok = true;
            for (k, e) in entries {
                match w.deeper(|w| convert_value(&e, from, to, w)).flatten() {
                    Some(ne) => na.set(k, ne),
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            w.visiting.remove(&key);
            if !ok {
                return None;
            }
            let na = Value::Array(Rc::new(RefCell::new(na)));
            match v {
                // The array inside a reference is changed in place.
                Value::Reference(r) => {
                    *r.borrow_mut() = na;
                    Some(v.clone())
                }
                _ => Some(na),
            }
        }
        Value::Object(o) => {
            let key = Rc::as_ptr(o) as *const () as usize;
            if !w.visiting.insert(key) {
                return None;
            }
            let n = o.borrow().properties.len();
            let mut ok = true;
            for i in 0..n {
                let cur = o.borrow().properties.get(i).map(|(_, v)| v.clone());
                let Some(cur) = cur else { continue };
                if matches!(cur, Value::Undef) {
                    continue;
                }
                match w.deeper(|w| convert_value(&cur, from, to, w)).flatten() {
                    Some(nv) => {
                        if let Some(slot) = o.borrow_mut().properties.get_mut(i) {
                            slot.1 = nv;
                        }
                    }
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            w.visiting.remove(&key);
            ok.then(|| v.clone())
        }
        _ => Some(v.clone()),
    }
}

/// The error mb_check_stack_limit() throws.
fn stack_limit_error(vm: &mut Vm) -> VmError {
    let msg = format!(
        "Maximum call stack size of {} bytes (zend.max_allowed_stack_size - zend.reserved_stack_size) reached. Infinite recursion?",
        goro_core::stack::emulated_size()
    );
    let line = vm.current_line;
    let exc = vm.create_exception(b"Error", &msg, line);
    vm.current_exception = Some(exc);
    VmError {
        message: format!("Uncaught Error: {}", msg),
        line,
    }
}

fn mb_convert_variables(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "mb_convert_variables";
    let to_name = str_arg(vm, f, args, 0, "to_encoding")?;
    let to = get_encoding(vm, f, Some(&to_name), 1, "to_encoding")?;
    let mut order_significant = true;
    let elist = match args.get(1).map(|v| v.deref()) {
        Some(Value::Array(a)) => {
            if is_all_encodings_list(&a) {
                order_significant = false;
            }
            parse_encoding_array(vm, f, &a, 2, "from_encoding")?
        }
        _ => {
            let s = str_arg(vm, f, args, 1, "from_encoding")?;
            parse_encoding_list_arg(vm, f, &s, 2, "from_encoding")?
        }
    };
    if elist.is_empty() {
        return Err(arg_value_error(
            vm,
            f,
            2,
            "from_encoding",
            "must specify at least one encoding",
        ));
    }
    let vars = &args[2.min(args.len())..];
    let from = if elist.len() == 1 {
        elist[0]
    } else {
        let mut strings: Vec<Vec<u8>> = Vec::new();
        for v in vars {
            let mut w = Walk::default();
            if collect_strings(v, &mut strings, &mut w) {
                if w.overflow {
                    return Err(stack_limit_error(vm));
                }
                vm.emit_warning("mb_convert_variables(): Cannot handle recursive references");
                return Ok(Value::False);
            }
        }
        let refs: Vec<&[u8]> = strings.iter().map(|s| s.as_slice()).collect();
        let strict = state::with(|s| s.strict_detection);
        match guess_encoding(&refs, &elist, strict, order_significant) {
            Some(e) => e,
            None => {
                vm.emit_warning("mb_convert_variables(): Unable to detect encoding");
                return Ok(Value::False);
            }
        }
    };
    for v in vars {
        // (the variable itself: through its reference)
        let mut w = Walk::default();
        let r = match v {
            Value::Reference(r) => {
                let inner = r.borrow().clone();
                convert_value(&inner, from, to, &mut w).map(|nv| *r.borrow_mut() = nv)
            }
            _ => convert_value(v, from, to, &mut w).map(|_| ()),
        };
        if r.is_none() {
            if w.overflow {
                return Err(stack_limit_error(vm));
            }
            vm.emit_warning("mb_convert_variables(): Cannot handle recursive references");
            return Ok(Value::False);
        }
    }
    Ok(pstr(from.name.as_bytes()))
}
