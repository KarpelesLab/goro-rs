//! Ports of the array_diff*/array_intersect* family from ext/standard/array.c
//! (php_array_diff, php_array_intersect, php_array_diff_key,
//! php_array_intersect_key and PHP_FUNCTION(array_diff)).
//!
//! The sort-based variants are observable: user callbacks see PHP's exact
//! comparison sequence (zend_sort on each argument, then a merge walk).

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

use crate::misc::{call_user_func, callable_error, key_to_bytes, key_to_value, sort_cmp_string};

const NORMAL: u8 = 1;
const KEY: u8 = 2;
const ASSOC: u8 = 6;

fn arg_type(v: &Value) -> String {
    match v.deref() {
        Value::Null | Value::Undef => "null".into(),
        Value::True => "true".into(),
        Value::False => "false".into(),
        Value::Long(_) => "int".into(),
        Value::Double(_) => "float".into(),
        Value::String(_) => "string".into(),
        Value::Array(_) => "array".into(),
        Value::Object(o) => goro_core::value::display_class_name(&o.borrow().class_name),
        Value::Resource(_) => "resource".into(),
        _ => "mixed".into(),
    }
}

fn throw(vm: &mut Vm, class: &[u8], msg: String) -> Result<Value, VmError> {
    let exc = vm.create_exception(class, &msg, vm.current_line);
    vm.current_exception = Some(exc);
    Err(VmError {
        message: msg,
        line: vm.current_line,
    })
}

fn array_type_error(vm: &mut Vm, fname: &str, i: usize, v: &Value) -> Result<Value, VmError> {
    let label = if i == 0 {
        "Argument #1 ($array)".to_string()
    } else {
        format!("Argument #{}", i + 1)
    };
    throw(
        vm,
        b"TypeError",
        format!(
            "{}(): {} must be of type array, {} given",
            fname,
            label,
            arg_type(v)
        ),
    )
}

/// Split `args` into arrays and `ncb` trailing callbacks, validating them like zpp.
fn parse_args(
    vm: &mut Vm,
    args: &[Value],
    fname: &str,
    ncb: usize,
) -> Result<(Vec<Value>, Vec<Value>), VmError> {
    let min = 1 + ncb;
    if args.len() < min {
        let msg = format!(
            "{}() expects at least {} argument{}, {} given",
            fname,
            min,
            if min == 1 { "" } else { "s" },
            args.len()
        );
        throw(vm, b"ArgumentCountError", msg)?;
    }
    let split = args.len() - ncb;
    let cbs: Vec<Value> = args[split..].iter().map(|v| v.deref()).collect();
    for (j, cb) in cbs.iter().enumerate() {
        if let Some(reason) = callable_error(vm, cb) {
            let msg = format!(
                "{}(): Argument #{} must be a valid callback, {}",
                fname,
                split + j + 1,
                reason
            );
            throw(vm, b"TypeError", msg)?;
        }
    }
    Ok((args[..split].iter().map(|v| v.deref()).collect(), cbs))
}

struct Cmp<'a> {
    fname: &'a str,
    warned: bool,
}

impl<'a> Cmp<'a> {
    /// php_array_user_compare_unstable / php_array_user_key_compare_unstable
    fn user(&mut self, vm: &mut Vm, cb: &Value, a: Value, b: Value) -> i32 {
        if vm.current_exception.is_some() {
            return 0;
        }
        let r = match call_user_func(vm, &[cb.clone(), a.clone(), b.clone()]) {
            Ok(v) => v,
            Err(_) => return 0,
        };
        if matches!(r, Value::True | Value::False) {
            if !self.warned {
                self.warned = true;
                vm.emit_deprecated(&format!("{}(): Returning bool from comparison function is deprecated, return an integer less than, equal to, or greater than zero", self.fname));
            }
            if matches!(r, Value::False) {
                if vm.current_exception.is_some() {
                    return 0;
                }
                let r2 = match call_user_func(vm, &[cb.clone(), b, a]) {
                    Ok(v) => v,
                    Err(_) => return 0,
                };
                let n = r2.to_long();
                return if n > 0 {
                    -1
                } else if n < 0 {
                    1
                } else {
                    0
                };
            }
        }
        let n = r.to_long();
        if n > 0 {
            1
        } else if n < 0 {
            -1
        } else {
            0
        }
    }
}

/// zval_user_compare(): plain user comparison (no bool handling).
fn zval_user_compare(vm: &mut Vm, cb: &Value, a: &Value, b: &Value) -> i32 {
    if vm.current_exception.is_some() {
        return 0;
    }
    match call_user_func(vm, &[cb.clone(), a.clone(), b.clone()]) {
        Ok(v) => {
            let n = v.to_long();
            if n > 0 {
                1
            } else if n < 0 {
                -1
            } else {
                0
            }
        }
        Err(_) => 0,
    }
}

/// string_compare_function()
fn string_compare(vm: &mut Vm, a: &Value, b: &Value) -> i32 {
    let x = sort_cmp_string(vm, a);
    let y = sort_cmp_string(vm, b);
    match x.cmp(&y) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Greater => 1,
        _ => 0,
    }
}

fn key_string_compare(a: &ArrayKey, b: &ArrayKey) -> i32 {
    match key_to_bytes(a).cmp(&key_to_bytes(b)) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Greater => 1,
        _ => 0,
    }
}

fn unwrap_rc1(v: &Value) -> Value {
    match v {
        Value::Reference(r) if Rc::strong_count(r) == 1 => r.borrow().clone(),
        other => other.clone(),
    }
}

fn arr_of(v: &Value) -> Rc<RefCell<PhpArray>> {
    match v {
        Value::Array(a) => a.clone(),
        _ => unreachable!(),
    }
}

/// php_array_diff_key / php_array_intersect_key.
/// data: 0 = none, 1 = internal (string compare), 2 = user
fn key_based(
    vm: &mut Vm,
    args: &[Value],
    fname: &str,
    data: u8,
    intersect: bool,
) -> Result<Value, VmError> {
    let ncb = if data == 2 { 1 } else { 0 };
    let (arrs, cbs) = parse_args(vm, args, fname, ncb)?;
    for (i, a) in arrs.iter().enumerate() {
        if !matches!(a, Value::Array(_)) {
            return array_type_error(vm, fname, i, a);
        }
    }
    let first: Vec<(ArrayKey, Value)> = arr_of(&arrs[0])
        .borrow()
        .iter()
        .map(|(k, v)| (k.clone(), unwrap_rc1(v)))
        .collect();
    let others: Vec<Rc<RefCell<PhpArray>>> = arrs[1..].iter().map(arr_of).collect();
    let mut result = PhpArray::new();
    for (k, val) in first {
        let mut ok = true;
        for o in &others {
            let data_v = o.borrow().get(&k).cloned();
            let matched = match data_v {
                None => false,
                Some(d) => match data {
                    0 => true,
                    1 => string_compare(vm, &val, &d) == 0,
                    _ => zval_user_compare(vm, &cbs[0], &val, &d) == 0,
                },
            };
            if intersect {
                if !matched {
                    ok = false;
                    break;
                }
            } else if matched {
                ok = false;
                break;
            }
        }
        if vm.current_exception.is_some() {
            return Ok(Value::Null);
        }
        if ok {
            result.set(k, val);
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

struct Bucket {
    key: ArrayKey,
    val: Value,
}

fn build_lists(arrs: &[Value]) -> Vec<Vec<Bucket>> {
    arrs.iter()
        .map(|a| {
            arr_of(a)
                .borrow()
                .iter()
                .map(|(k, v)| Bucket {
                    key: k.clone(),
                    val: v.clone(),
                })
                .collect()
        })
        .collect()
}

/// Comparators used by php_array_diff / php_array_intersect.
struct Funcs<'a> {
    data_user: Option<Value>,
    key_user: Option<Value>,
    cmp: Cmp<'a>,
}

impl<'a> Funcs<'a> {
    fn data(&mut self, vm: &mut Vm, a: &Bucket, b: &Bucket) -> i32 {
        match self.data_user.clone() {
            Some(cb) => self.cmp.user(vm, &cb, a.val.clone(), b.val.clone()),
            None => string_compare(vm, &a.val, &b.val),
        }
    }
    fn key(&mut self, vm: &mut Vm, a: &Bucket, b: &Bucket) -> i32 {
        match self.key_user.clone() {
            Some(cb) => self
                .cmp
                .user(vm, &cb, key_to_value(&a.key), key_to_value(&b.key)),
            None => key_string_compare(&a.key, &b.key),
        }
    }
}

fn sort_list(vm: &mut Vm, list: &mut [Bucket], f: &mut Funcs, by_data: bool) {
    if list.len() > 1 {
        crate::zsort::zend_sort(list, &mut |a: &Bucket, b: &Bucket| {
            if by_data {
                f.data(vm, a, b)
            } else {
                f.key(vm, a, b)
            }
        });
    }
}

/// php_array_diff()
fn sorted_diff(
    vm: &mut Vm,
    args: &[Value],
    fname: &str,
    behavior: u8,
    data_user: bool,
    key_user: bool,
) -> Result<Value, VmError> {
    let ncb = data_user as usize + key_user as usize;
    let (arrs, cbs) = parse_args(vm, args, fname, ncb)?;
    let mut f = Funcs {
        data_user: if data_user {
            Some(cbs[0].clone())
        } else {
            None
        },
        key_user: if key_user {
            Some(cbs[cbs.len() - 1].clone())
        } else {
            None
        },
        cmp: Cmp {
            fname,
            warned: false,
        },
    };
    for (i, a) in arrs.iter().enumerate() {
        if !matches!(a, Value::Array(_)) {
            return array_type_error(vm, fname, i, a);
        }
    }
    let mut result = arr_of(&arrs[0]).borrow().clone();
    let mut lists = build_lists(&arrs);
    for l in lists.iter_mut() {
        sort_list(vm, l, &mut f, behavior == NORMAL);
    }
    let n = lists.len();
    let mut ptrs = vec![0usize; n];
    'outer: while ptrs[0] < lists[0].len() {
        if vm.current_exception.is_some() {
            break;
        }
        let mut c: i32 = 1;
        for i in 1..n {
            let mut ptr = ptrs[i];
            if behavior == NORMAL {
                while ptrs[i] < lists[i].len() {
                    c = f.data(vm, &lists[0][ptrs[0]], &lists[i][ptrs[i]]);
                    if c > 0 {
                        ptrs[i] += 1;
                    } else {
                        break;
                    }
                }
            } else {
                while ptr < lists[i].len() {
                    c = f.key(vm, &lists[0][ptrs[0]], &lists[i][ptr]);
                    if c != 0 {
                        ptr += 1;
                    } else {
                        break;
                    }
                }
            }
            if c == 0 {
                if behavior == NORMAL {
                    if ptrs[i] < lists[i].len() {
                        ptrs[i] += 1;
                    }
                    break;
                } else if behavior == ASSOC {
                    if ptr < lists[i].len() {
                        if f.data(vm, &lists[0][ptrs[0]], &lists[i][ptr]) != 0 {
                            c = -1;
                        } else {
                            break;
                        }
                    }
                } else if behavior == KEY {
                    break;
                }
            }
        }
        if c == 0 {
            // present in another argument: delete all entries equal to ptrs[0]
            loop {
                let k = lists[0][ptrs[0]].key.clone();
                result.remove(&k);
                ptrs[0] += 1;
                if ptrs[0] >= lists[0].len() {
                    break 'outer;
                }
                if behavior == NORMAL {
                    if f.data(vm, &lists[0][ptrs[0] - 1], &lists[0][ptrs[0]]) != 0 {
                        break;
                    }
                } else {
                    break;
                }
            }
        } else {
            loop {
                ptrs[0] += 1;
                if ptrs[0] >= lists[0].len() {
                    break 'outer;
                }
                if behavior == NORMAL {
                    if f.data(vm, &lists[0][ptrs[0] - 1], &lists[0][ptrs[0]]) != 0 {
                        break;
                    }
                } else {
                    break;
                }
            }
        }
    }
    if vm.current_exception.is_some() {
        return Ok(Value::Null);
    }
    result.pointer = 0;
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

/// php_array_intersect()
fn sorted_intersect(
    vm: &mut Vm,
    args: &[Value],
    fname: &str,
    behavior: u8,
    data_user: bool,
    key_user: bool,
) -> Result<Value, VmError> {
    let ncb = data_user as usize + key_user as usize;
    let (arrs, cbs) = parse_args(vm, args, fname, ncb)?;
    let mut f = Funcs {
        data_user: if data_user {
            Some(cbs[0].clone())
        } else {
            None
        },
        key_user: if key_user {
            Some(cbs[cbs.len() - 1].clone())
        } else {
            None
        },
        cmp: Cmp {
            fname,
            warned: false,
        },
    };
    for (i, a) in arrs.iter().enumerate() {
        if !matches!(a, Value::Array(_)) {
            return array_type_error(vm, fname, i, a);
        }
    }
    let mut result = arr_of(&arrs[0]).borrow().clone();
    let mut lists = build_lists(&arrs);
    for l in lists.iter_mut() {
        sort_list(vm, l, &mut f, behavior == NORMAL);
    }
    let n = lists.len();
    let mut ptrs = vec![0usize; n];
    let mut c: i32 = 0;
    'outer: while ptrs[0] < lists[0].len() {
        if vm.current_exception.is_some() {
            break;
        }
        let mut last_i = 1usize;
        for i in 1..n {
            last_i = i;
            if behavior == NORMAL {
                while ptrs[i] < lists[i].len() {
                    c = f.data(vm, &lists[0][ptrs[0]], &lists[i][ptrs[i]]);
                    if c > 0 {
                        ptrs[i] += 1;
                    } else {
                        break;
                    }
                }
            } else {
                while ptrs[i] < lists[i].len() {
                    c = f.key(vm, &lists[0][ptrs[0]], &lists[i][ptrs[i]]);
                    if c > 0 {
                        ptrs[i] += 1;
                    } else {
                        break;
                    }
                }
                if c == 0
                    && ptrs[i] < lists[i].len()
                    && behavior == ASSOC
                    && f.data(vm, &lists[0][ptrs[0]], &lists[i][ptrs[i]]) != 0
                {
                    c = 1;
                }
            }
            if ptrs[i] >= lists[i].len() {
                // remaining entries of ptrs[0] are not in this argument
                while ptrs[0] < lists[0].len() {
                    let k = lists[0][ptrs[0]].key.clone();
                    result.remove(&k);
                    ptrs[0] += 1;
                }
                break 'outer;
            }
            if c != 0 {
                break;
            }
            ptrs[i] += 1;
        }
        if c != 0 {
            loop {
                let k = lists[0][ptrs[0]].key.clone();
                result.remove(&k);
                ptrs[0] += 1;
                if ptrs[0] >= lists[0].len() {
                    break 'outer;
                }
                if behavior == NORMAL {
                    if f.data(vm, &lists[0][ptrs[0]], &lists[last_i][ptrs[last_i]]) >= 0 {
                        break;
                    }
                } else {
                    break;
                }
            }
        } else {
            loop {
                ptrs[0] += 1;
                if ptrs[0] >= lists[0].len() {
                    break 'outer;
                }
                if behavior == NORMAL {
                    if f.data(vm, &lists[0][ptrs[0] - 1], &lists[0][ptrs[0]]) != 0 {
                        break;
                    }
                } else {
                    break;
                }
            }
        }
    }
    if vm.current_exception.is_some() {
        return Ok(Value::Null);
    }
    result.pointer = 0;
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

/// PHP_FUNCTION(array_diff): hash-based string comparison.
pub fn array_diff(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let fname = "array_diff";
    let (arrs, _) = parse_args(vm, args, fname, 0)?;
    if !matches!(arrs[0], Value::Array(_)) {
        return array_type_error(vm, fname, 0, &arrs[0]);
    }
    let first = arr_of(&arrs[0]);
    let num = first.borrow().len();
    let check_rest = |vm: &mut Vm| -> Result<(), VmError> {
        for (i, a) in arrs.iter().enumerate().skip(1) {
            if !matches!(a, Value::Array(_)) {
                array_type_error(vm, fname, i, a)?;
            }
        }
        Ok(())
    };
    if num == 0 {
        check_rest(vm)?;
        return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
    }
    if num == 1 {
        let value = first
            .borrow()
            .values()
            .next()
            .cloned()
            .unwrap_or(Value::Null);
        let search = sort_cmp_string(vm, &value);
        let mut found = false;
        for (i, a) in arrs.iter().enumerate().skip(1) {
            if !matches!(a, Value::Array(_)) {
                return array_type_error(vm, fname, i, a);
            }
            if !found {
                let vals: Vec<Value> = arr_of(a).borrow().values().cloned().collect();
                for v in vals {
                    if sort_cmp_string(vm, &v) == search {
                        found = true;
                        break;
                    }
                }
            }
        }
        if vm.current_exception.is_some() {
            return Ok(Value::Null);
        }
        return Ok(Value::Array(Rc::new(RefCell::new(if found {
            PhpArray::new()
        } else {
            first.borrow().clone()
        }))));
    }
    check_rest(vm)?;
    let mut exclude: std::collections::HashSet<Vec<u8>> = std::collections::HashSet::new();
    let mut total = 0;
    for a in &arrs[1..] {
        total += arr_of(a).borrow().len();
    }
    if total == 0 {
        return Ok(Value::Array(Rc::new(RefCell::new(first.borrow().clone()))));
    }
    for a in &arrs[1..] {
        let vals: Vec<Value> = arr_of(a).borrow().values().cloned().collect();
        for v in vals {
            exclude.insert(sort_cmp_string(vm, &v));
        }
    }
    // (zval_add_ref(): a reference only the array holds is copied as its
    // value)
    let entries: Vec<(ArrayKey, Value)> = first
        .borrow()
        .iter()
        .map(|(k, v)| (k.clone(), unwrap_rc1(v)))
        .collect();
    let mut result = PhpArray::new();
    for (k, v) in entries {
        let s = sort_cmp_string(vm, &v);
        if !exclude.contains(&s) {
            result.set(k, v);
        }
    }
    if vm.current_exception.is_some() {
        return Ok(Value::Null);
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

pub fn array_diff_key(vm: &mut Vm, a: &[Value]) -> Result<Value, VmError> {
    key_based(vm, a, "array_diff_key", 0, false)
}
pub fn array_diff_assoc(vm: &mut Vm, a: &[Value]) -> Result<Value, VmError> {
    key_based(vm, a, "array_diff_assoc", 1, false)
}
pub fn array_udiff_assoc(vm: &mut Vm, a: &[Value]) -> Result<Value, VmError> {
    key_based(vm, a, "array_udiff_assoc", 2, false)
}
pub fn array_intersect_key(vm: &mut Vm, a: &[Value]) -> Result<Value, VmError> {
    key_based(vm, a, "array_intersect_key", 0, true)
}
pub fn array_intersect_assoc(vm: &mut Vm, a: &[Value]) -> Result<Value, VmError> {
    key_based(vm, a, "array_intersect_assoc", 1, true)
}
pub fn array_uintersect_assoc(vm: &mut Vm, a: &[Value]) -> Result<Value, VmError> {
    key_based(vm, a, "array_uintersect_assoc", 2, true)
}

pub fn array_diff_ukey(vm: &mut Vm, a: &[Value]) -> Result<Value, VmError> {
    sorted_diff(vm, a, "array_diff_ukey", KEY, false, true)
}
pub fn array_diff_uassoc(vm: &mut Vm, a: &[Value]) -> Result<Value, VmError> {
    sorted_diff(vm, a, "array_diff_uassoc", ASSOC, false, true)
}
pub fn array_udiff(vm: &mut Vm, a: &[Value]) -> Result<Value, VmError> {
    sorted_diff(vm, a, "array_udiff", NORMAL, true, false)
}
pub fn array_udiff_uassoc(vm: &mut Vm, a: &[Value]) -> Result<Value, VmError> {
    sorted_diff(vm, a, "array_udiff_uassoc", ASSOC, true, true)
}

pub fn array_intersect(vm: &mut Vm, a: &[Value]) -> Result<Value, VmError> {
    sorted_intersect(vm, a, "array_intersect", NORMAL, false, false)
}
pub fn array_uintersect(vm: &mut Vm, a: &[Value]) -> Result<Value, VmError> {
    sorted_intersect(vm, a, "array_uintersect", NORMAL, true, false)
}
pub fn array_intersect_ukey(vm: &mut Vm, a: &[Value]) -> Result<Value, VmError> {
    sorted_intersect(vm, a, "array_intersect_ukey", KEY, false, true)
}
pub fn array_intersect_uassoc(vm: &mut Vm, a: &[Value]) -> Result<Value, VmError> {
    sorted_intersect(vm, a, "array_intersect_uassoc", ASSOC, false, true)
}
pub fn array_uintersect_uassoc(vm: &mut Vm, a: &[Value]) -> Result<Value, VmError> {
    sorted_intersect(vm, a, "array_uintersect_uassoc", ASSOC, true, true)
}

pub fn register(vm: &mut Vm) {
    vm.register_function(b"array_diff", array_diff);
    vm.register_function(b"array_diff_key", array_diff_key);
    vm.register_function(b"array_diff_assoc", array_diff_assoc);
    vm.register_function(b"array_udiff_assoc", array_udiff_assoc);
    vm.register_function(b"array_intersect_key", array_intersect_key);
    vm.register_function(b"array_intersect_assoc", array_intersect_assoc);
    vm.register_function(b"array_uintersect_assoc", array_uintersect_assoc);
    vm.register_function(b"array_diff_ukey", array_diff_ukey);
    vm.register_function(b"array_diff_uassoc", array_diff_uassoc);
    vm.register_function(b"array_udiff", array_udiff);
    vm.register_function(b"array_udiff_uassoc", array_udiff_uassoc);
    vm.register_function(b"array_intersect", array_intersect);
    vm.register_function(b"array_uintersect", array_uintersect);
    vm.register_function(b"array_intersect_ukey", array_intersect_ukey);
    vm.register_function(b"array_intersect_uassoc", array_intersect_uassoc);
    vm.register_function(b"array_uintersect_uassoc", array_uintersect_uassoc);
}
