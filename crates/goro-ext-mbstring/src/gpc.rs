//! mb_parse_str (C: mb_gpc.c `_php_mb_encoding_handler_ex` together with
//! main/php_variables.c `php_register_variable_ex`)

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

use crate::mbfl::{self, EncNo, Encoding};
use crate::mbstring::*;
use crate::state;
use crate::zpp::*;

pub fn register(vm: &mut Vm) {
    vm.register_function(b"mb_parse_str", mb_parse_str);
}

/// php_url_decode
fn url_decode(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c == b'+' {
            out.push(b' ');
        } else if c == b'%'
            && i + 2 < s.len()
            && s[i + 1].is_ascii_hexdigit()
            && s[i + 2].is_ascii_hexdigit()
        {
            let h = |b: u8| -> u8 {
                match b {
                    b'0'..=b'9' => b - b'0',
                    b'a'..=b'f' => b - b'a' + 10,
                    _ => b - b'A' + 10,
                }
            };
            out.push((h(s[i + 1]) << 4) | h(s[i + 2]));
            i += 2;
        } else {
            out.push(c);
        }
        i += 1;
    }
    out
}

/// ZEND_HANDLE_NUMERIC_STR: canonical decimal integers become int keys
fn numeric_key(s: &[u8]) -> Option<i64> {
    if s.is_empty() || s.len() > 20 {
        return None;
    }
    let (neg, digits) = if s[0] == b'-' {
        (true, &s[1..])
    } else {
        (false, s)
    };
    if digits.is_empty() || !digits.iter().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if digits[0] == b'0' && (digits.len() > 1 || neg) {
        return None;
    }
    let t = std::str::from_utf8(s).ok()?;
    t.parse::<i64>().ok()
}

fn sym_key(s: &[u8]) -> ArrayKey {
    match numeric_key(s) {
        Some(n) => ArrayKey::Int(n),
        None => ArrayKey::String(PhpString::from_vec(s.to_vec())),
    }
}

fn is_forbidden(mangled: &[u8], pre: &[u8]) -> bool {
    (mangled.starts_with(b"__Host-") && !pre.starts_with(b"__Host-"))
        || (mangled.starts_with(b"__Secure-") && !pre.starts_with(b"__Secure-"))
}

/// C: `php_register_variable_ex`
pub fn register_variable(vm: &mut Vm, track: &Rc<RefCell<PhpArray>>, var_name: &[u8], val: Value) {
    let max_nesting = vm
        .constants
        .get(b"max_input_nesting_level".as_ref())
        .map(|v| v.to_long())
        .unwrap_or(64);
    // C string semantics: stop at the first NUL
    let var_name = match var_name.iter().position(|&c| c == 0) {
        Some(p) => &var_name[..p],
        None => var_name,
    };
    let mut start = 0;
    while start < var_name.len() && var_name[start] == b' ' {
        start += 1;
    }
    let orig = &var_name[start..];
    let mut var: Vec<u8> = orig.to_vec();
    let mut is_array = false;
    let mut ip = 0usize;
    let mut var_len = var.len();
    for (i, c) in var.iter_mut().enumerate() {
        if *c == b' ' || *c == b'.' {
            *c = b'_';
        } else if *c == b'[' {
            is_array = true;
            ip = i;
            var_len = i;
            break;
        }
    }
    if var_len == 0 {
        return;
    }
    // Work on a NUL-terminated copy to mirror the C pointer walking
    var.push(0);
    let mut symtable = track.clone();
    let mut index: Option<(usize, usize)> = Some((0, var_len)); // (start, len) in var
    if is_array {
        var[ip] = 0;
        let mut nest_level = 0;
        loop {
            nest_level += 1;
            if nest_level > max_nesting {
                let base = var[..var_len].to_vec();
                track.borrow_mut().remove(&sym_key(&base));
                return;
            }
            ip += 1;
            let mut index_s = ip;
            if var[ip].is_ascii_whitespace() {
                ip += 1;
            }
            let new_index: Option<(usize, usize)>;
            if var[ip] == b']' {
                new_index = None;
            } else {
                match var[ip..].iter().position(|&c| c == b']') {
                    None => {
                        // not an index; un-terminate the var name
                        var[index_s - 1] = b'_';
                        let mut p = index_s;
                        while var[p] != 0 {
                            if var[p] == b' ' || var[p] == b'.' || var[p] == b'[' {
                                var[p] = b'_';
                            }
                            p += 1;
                        }
                        let index_len = match index {
                            Some((s, _)) => var[s..].iter().position(|&c| c == 0).unwrap_or(0),
                            None => 0,
                        };
                        index = index.map(|(s, _)| (s, index_len));
                        // plain_var
                        plain_var(&symtable, &var, index, orig, val);
                        return;
                    }
                    Some(off) => {
                        ip += off;
                        var[ip] = 0;
                        if index_s > ip {
                            index_s = ip;
                        }
                        let len = var[index_s..].iter().position(|&c| c == 0).unwrap_or(0);
                        new_index = Some((index_s, len));
                    }
                }
            }
            // Descend
            let next: Rc<RefCell<PhpArray>> = match index {
                None => {
                    let a = Rc::new(RefCell::new(PhpArray::new()));
                    symtable.borrow_mut().push(Value::Array(a.clone()));
                    a
                }
                Some((s, l)) => {
                    let key_bytes = var[s..s + l].to_vec();
                    if is_forbidden(&key_bytes, orig) {
                        return;
                    }
                    let key = sym_key(&key_bytes);
                    let existing = symtable.borrow().get(&key).cloned();
                    match existing.map(|v| v.deref()) {
                        Some(Value::Array(a)) => {
                            // SEPARATE_ARRAY
                            if Rc::strong_count(&a) > 1 {
                                let copy = Rc::new(RefCell::new(a.borrow().clone()));
                                symtable.borrow_mut().set(key, Value::Array(copy.clone()));
                                copy
                            } else {
                                a
                            }
                        }
                        _ => {
                            let a = Rc::new(RefCell::new(PhpArray::new()));
                            symtable.borrow_mut().set(key, Value::Array(a.clone()));
                            a
                        }
                    }
                }
            };
            symtable = next;
            index = new_index;
            ip += 1;
            if var[ip] == b'[' {
                var[ip] = 0;
            } else {
                plain_var(&symtable, &var, index, orig, val);
                return;
            }
        }
    } else {
        plain_var(&symtable, &var, index, orig, val);
    }
}

fn plain_var(
    symtable: &Rc<RefCell<PhpArray>>,
    var: &[u8],
    index: Option<(usize, usize)>,
    orig: &[u8],
    val: Value,
) {
    match index {
        None => symtable.borrow_mut().push(val),
        Some((s, l)) => {
            let key_bytes = &var[s..s + l];
            if is_forbidden(key_bytes, orig) {
                return;
            }
            symtable.borrow_mut().set(sym_key(key_bytes), val);
        }
    }
}

/// php_strtok_r: split on any separator byte, skipping empty tokens
fn strtok<'a>(s: &'a [u8], seps: &[u8]) -> Vec<&'a [u8]> {
    s.split(|c| seps.contains(c))
        .filter(|t| !t.is_empty())
        .collect()
}

/// C: `_php_mb_encoding_handler_ex`. Returns the detected encoding.
pub fn encoding_handler(
    vm: &mut Vm,
    track: &Rc<RefCell<PhpArray>>,
    res: &[u8],
    separator: &[u8],
    to: &'static Encoding,
    from_list: &[&'static Encoding],
    report_errors: bool,
) -> Option<&'static Encoding> {
    // C string semantics
    let res = match res.iter().position(|&c| c == 0) {
        Some(p) => &res[..p],
        None => res,
    };
    if res.is_empty() {
        return None;
    }
    let mut list: Vec<Vec<u8>> = Vec::new();
    for var in strtok(res, separator) {
        match var.iter().position(|&c| c == b'=') {
            Some(eq) => {
                list.push(url_decode(&var[..eq]));
                list.push(url_decode(&var[eq + 1..]));
            }
            None => {
                list.push(url_decode(var));
                list.push(Vec::new());
            }
        }
    }
    let max_input_vars = vm
        .constants
        .get(b"max_input_vars".as_ref())
        .map(|v| v.to_long())
        .unwrap_or(1000);
    if list.len() as i64 > max_input_vars.saturating_mul(2) {
        vm.emit_warning(&format!(
            "mb_parse_str(): Input variables exceeded {}. To increase the limit change max_input_vars in php.ini.",
            max_input_vars
        ));
        return None;
    }
    let pass: &'static Encoding = &mbfl::basic::ENC_PASS;
    let from: &'static Encoding = if from_list.is_empty() {
        pass
    } else if from_list.len() == 1 {
        from_list[0]
    } else {
        let refs: Vec<&[u8]> = list.iter().map(|v| v.as_slice()).collect();
        let strict = state::with(|s| s.strict_detection);
        match guess_encoding(&refs, from_list, strict, false) {
            Some(e) => e,
            None => {
                if report_errors {
                    vm.emit_warning("mb_parse_str(): Unable to detect encoding");
                }
                pass
            }
        }
    };
    let mut n = 0;
    while n < list.len() {
        let (var, val) = if from.no != EncNo::Pass && to.no != EncNo::Pass {
            (
                convert_ex(&list[n], to, from),
                convert_ex(&list[n + 1], to, from),
            )
        } else {
            (list[n].clone(), list[n + 1].clone())
        };
        register_variable(vm, track, &var, pstring(val));
        n += 2;
    }
    Some(from)
}

fn mb_parse_str(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = str_arg(vm, "mb_parse_str", args, 0, "string")?;
    state::sync(vm);
    let track = Rc::new(RefCell::new(PhpArray::new()));
    let sep = match vm.constants.get(b"arg_separator.input".as_ref()) {
        Some(v) if !v.to_php_string().is_empty() => v.to_php_string().as_bytes().to_vec(),
        _ => b"&".to_vec(),
    };
    let to = state::current_internal_encoding();
    let from_list = state::with(|st| st.http_input_list.clone());
    let detected = encoding_handler(vm, &track, &s, &sep, to, &from_list, true);
    state::with(|st| st.http_input_identify = detected);
    if let Some(Value::Reference(r)) = args.get(1) {
        *r.borrow_mut() = Value::Array(track);
    }
    Ok(vbool(detected.is_some()))
}
