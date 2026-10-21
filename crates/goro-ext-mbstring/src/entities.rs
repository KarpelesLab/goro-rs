//! mb_encode_numericentity / mb_decode_numericentity (mbstring.c)

use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

use crate::mbfl::{ConvBuf, Encoding};
use crate::mbstring::*;
use crate::state;
use crate::zpp::*;

pub fn register(vm: &mut Vm) {
    vm.register_function(b"mb_encode_numericentity", mb_encode_numericentity);
    vm.register_function(b"mb_decode_numericentity", mb_decode_numericentity);
}

/// C: `make_conversion_map`
fn make_conversion_map(vm: &mut Vm, func: &str, args: &[Value]) -> Result<Vec<u32>, VmError> {
    let arr = match args.get(1).map(|v| v.deref()) {
        Some(Value::Array(a)) => a,
        other => {
            let t = other
                .as_ref()
                .map(type_name)
                .unwrap_or_else(|| "null".to_string());
            return Err(type_error(
                vm,
                format!(
                    "{}(): Argument #2 ($map) must be of type array, {} given",
                    func, t
                ),
            ));
        }
    };
    let vals: Vec<Value> = arr.borrow().values().cloned().collect();
    if !vals.len().is_multiple_of(4) {
        return Err(arg_value_error(
            vm,
            func,
            2,
            "map",
            "must have a multiple of 4 elements",
        ));
    }
    let mut map = Vec::with_capacity(vals.len());
    for v in vals {
        let v = v.deref();
        let n: i64 = match &v {
            Value::Long(n) => *n,
            Value::Double(d) => {
                if d.is_finite() {
                    *d as i64
                } else {
                    0
                }
            }
            Value::Null | Value::False => 0,
            Value::True => 1,
            Value::String(s) => {
                let t = String::from_utf8_lossy(s.as_bytes()).to_string();
                let tt = t.trim();
                if let Ok(n) = tt.parse::<i64>() {
                    n
                } else if let Ok(f) = tt.parse::<f64>() {
                    f as i64
                } else {
                    // leading-numeric strings give a warning; others fail
                    let digits: String = tt
                        .chars()
                        .take_while(|c| c.is_ascii_digit() || *c == '-')
                        .collect();
                    if !digits.is_empty() && digits != "-" {
                        vm.emit_warning("A non-numeric value encountered");
                        digits.parse::<i64>().unwrap_or(0)
                    } else {
                        return Err(arg_value_error(
                            vm,
                            func,
                            2,
                            "map",
                            "must only be composed of values of type int",
                        ));
                    }
                }
            }
            _ => {
                return Err(arg_value_error(
                    vm,
                    func,
                    2,
                    "map",
                    "must only be composed of values of type int",
                ));
            }
        };
        map.push(n as u32);
    }
    Ok(map)
}

fn entity_convert(w: u32, map: &[u32]) -> Option<u32> {
    for m in map.as_chunks::<4>().0 {
        if w >= m[0] && w <= m[1] {
            return Some(w.wrapping_add(m[2]) & m[3]);
        }
    }
    None
}

fn entity_encode(input: &[u8], enc: &Encoding, map: &[u32], hex: bool) -> Vec<u8> {
    let w = to_wchars(enc, input);
    let mut conv: Vec<u32> = Vec::with_capacity(w.len());
    for &c in &w {
        if let Some(mut v) = entity_convert(c, map) {
            conv.push(b'&' as u32);
            conv.push(b'#' as u32);
            if hex {
                conv.push(b'x' as u32);
            }
            if v == 0 {
                conv.push(b'0' as u32);
            } else {
                let mut digits = Vec::new();
                if hex {
                    while v > 0 {
                        digits.push(b"0123456789ABCDEF"[(v & 0xF) as usize] as u32);
                        v >>= 4;
                    }
                } else {
                    while v > 0 {
                        digits.push((b'0' as u32) + v % 10);
                        v /= 10;
                    }
                }
                conv.extend(digits.iter().rev());
            }
            conv.push(b';' as u32);
        } else {
            conv.push(c);
        }
    }
    from_wchars(enc, &conv)
}

fn mb_encode_numericentity(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "mb_encode_numericentity";
    let s = str_arg(vm, f, args, 0, "string")?;
    if let Some(v) = args.get(1) {
        if !matches!(v.deref(), Value::Array(_)) {
            return Err(type_error(
                vm,
                format!(
                    "{}(): Argument #2 ($map) must be of type array, {} given",
                    f,
                    type_name(&v.deref())
                ),
            ));
        }
    }
    let enc = encoding_arg(vm, f, args, 2, "encoding")?;
    let hex = bool_arg(args, 3);
    let map = make_conversion_map(vm, f, args)?;
    Ok(pstring(entity_encode(&s, enc, &map, hex)))
}

fn entity_deconvert(number: u32, map: &[u32]) -> Option<u32> {
    for m in map.as_chunks::<4>().0 {
        let cp = number.wrapping_sub(m[2]);
        if cp >= m[0] && cp <= m[1] {
            return Some(cp);
        }
    }
    None
}

const DEC_ENTITY_MINLEN: usize = 3;
const HEX_ENTITY_MINLEN: usize = 4;
const DEC_ENTITY_MAXLEN: usize = 12;
const HEX_ENTITY_MAXLEN: usize = 11;

fn is_hex(w: u32) -> bool {
    (b'0' as u32..=b'9' as u32).contains(&w)
        || (b'A' as u32..=b'F' as u32).contains(&w)
        || (b'a' as u32..=b'f' as u32).contains(&w)
}

fn entity_decode(input: &[u8], enc: &Encoding, map: &[u32]) -> Vec<u8> {
    let mut w = to_wchars(enc, input);
    let n = w.len();
    w.push(b'&' as u32); // sentinel
    let amp = b'&' as u32;
    let mut out: Vec<u32> = Vec::with_capacity(n);
    let mut p = 0usize;
    while p < n {
        if w[p] != amp {
            out.push(w[p]);
            p += 1;
            continue;
        }
        let mut p2 = p + 1;
        if w[p2] == b'#' as u32 {
            p2 += 1;
            if w[p2] == b'x' as u32 {
                p2 += 1;
                while is_hex(w[p2]) {
                    p2 += 1;
                }
                let len = p2 - p;
                if !(HEX_ENTITY_MINLEN..=HEX_ENTITY_MAXLEN).contains(&len) {
                    out.extend_from_slice(&w[p..p2]);
                } else {
                    let mut value: u32 = 0;
                    for &c in &w[p + 3..p2] {
                        value = if c <= b'9' as u32 {
                            value.wrapping_mul(16).wrapping_add(c - b'0' as u32)
                        } else if c >= b'a' as u32 {
                            value.wrapping_mul(16).wrapping_add(10 + c - b'a' as u32)
                        } else {
                            value.wrapping_mul(16).wrapping_add(10 + c - b'A' as u32)
                        };
                    }
                    if let Some(cp) = entity_deconvert(value, map) {
                        out.push(cp);
                        if p2 < n && w[p2] == b';' as u32 {
                            p2 += 1;
                        }
                    } else {
                        out.extend_from_slice(&w[p..p2]);
                    }
                }
            } else {
                while (b'0' as u32..=b'9' as u32).contains(&w[p2]) {
                    p2 += 1;
                }
                let len = p2 - p;
                if !(DEC_ENTITY_MINLEN..=DEC_ENTITY_MAXLEN).contains(&len) {
                    out.extend_from_slice(&w[p..p2]);
                } else {
                    let mut value: u32 = 0;
                    let mut too_big = false;
                    for &c in &w[p + 2..p2] {
                        if value > 0x19999999 {
                            too_big = true;
                            break;
                        }
                        value = value.wrapping_mul(10).wrapping_add(c - b'0' as u32);
                    }
                    if too_big {
                        out.extend_from_slice(&w[p..p2]);
                    } else if let Some(cp) = entity_deconvert(value, map) {
                        out.push(cp);
                        if p2 < n && w[p2] == b';' as u32 {
                            p2 += 1;
                        }
                    } else {
                        out.extend_from_slice(&w[p..p2]);
                    }
                }
            }
            p = p2.min(n);
        } else {
            out.push(amp);
            p = p2;
        }
    }
    let (repl, mode) = state::illegal_settings();
    let mut buf = ConvBuf::new(input.len(), repl, mode);
    if let Some(f) = enc.from_wchar {
        f(&out, &mut buf, true);
    }
    buf.out
}

fn mb_decode_numericentity(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "mb_decode_numericentity";
    let s = str_arg(vm, f, args, 0, "string")?;
    if let Some(v) = args.get(1) {
        if !matches!(v.deref(), Value::Array(_)) {
            return Err(type_error(
                vm,
                format!(
                    "{}(): Argument #2 ($map) must be of type array, {} given",
                    f,
                    type_name(&v.deref())
                ),
            ));
        }
    }
    let enc = encoding_arg(vm, f, args, 2, "encoding")?;
    let map = make_conversion_map(vm, f, args)?;
    Ok(pstring(entity_decode(&s, enc, &map)))
}
