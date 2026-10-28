//! Array functions of the standard extension (PHP manual: Array
//! functions).
//!
//! Sorting goes through `goro_ext_standard::sort`, which makes the same
//! comparisons in the same order as PHP (observable with user comparators)
//! and is stable (RFC wiki.php.net/rfc/stable_sorting). Messages and edge
//! cases come from the tests ext/standard/tests/array/*.phpt and
//! observations of PHP 8.5.

use std::cmp::Ordering;

use goro_ext_standard as std_ext;
use goro_runtime::compare::{loose_eq, strict_eq};
use goro_runtime::{Key, KeyRef, PhpArray, PhpRef, PhpStr, Value};

use super::math::php_compare;
use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Unwind, Vm};

pub static MODULE: Module = Module {
    name: "standard",
    functions: &[
        (
            "function count(Countable|array $value, int $mode = COUNT_NORMAL): int",
            count,
        ),
        (
            "function sizeof(Countable|array $value, int $mode = COUNT_NORMAL): int",
            count,
        ),
        (
            "function in_array(mixed $needle, array $haystack, bool $strict = false): bool",
            in_array,
        ),
        (
            "function array_search(mixed $needle, array $haystack, bool $strict = false): string|int|false",
            array_search,
        ),
        (
            "function array_keys(array $array, mixed $filter_value = UNKNOWN, bool $strict = false): array",
            array_keys,
        ),
        ("function array_values(array $array): array", array_values),
        ("function array_merge(array ...$arrays): array", array_merge),
        (
            "function array_merge_recursive(array ...$arrays): array",
            array_merge_recursive,
        ),
        (
            "function array_replace(array $array, array ...$replacements): array",
            array_replace,
        ),
        (
            "function array_replace_recursive(array $array, array ...$replacements): array",
            array_replace_recursive,
        ),
        (
            "function array_combine(array $keys, array $values): array",
            array_combine,
        ),
        ("function array_flip(array $array): array", array_flip),
        (
            "function array_fill(int $start_index, int $count, mixed $value): array",
            array_fill,
        ),
        (
            "function array_fill_keys(array $keys, mixed $value): array",
            array_fill_keys,
        ),
        (
            "function array_pad(array $array, int $length, mixed $value): array",
            array_pad,
        ),
        (
            "function range($start, $end, int|float $step = 1): array",
            range,
        ),
        (
            "function array_key_exists($key, array $array): bool",
            array_key_exists,
        ),
        (
            "function key_exists($key, array $array): bool",
            array_key_exists,
        ),
        (
            "function array_key_first(array $array): string|int|null",
            array_key_first,
        ),
        (
            "function array_key_last(array $array): string|int|null",
            array_key_last,
        ),
        ("function array_first(array $array): mixed", array_first),
        ("function array_last(array $array): mixed", array_last),
        (
            "function array_slice(array $array, int $offset, ?int $length = null, bool $preserve_keys = false): array",
            array_slice,
        ),
        (
            "function array_splice(array &$array, int $offset, ?int $length = null, mixed $replacement = []): array",
            array_splice,
        ),
        (
            "function array_map(?callable $callback, array $array, array ...$arrays): array",
            array_map,
        ),
        (
            "function array_filter(array $array, ?callable $callback = null, int $mode = 0): array",
            array_filter,
        ),
        (
            "function array_reduce(array $array, callable $callback, mixed $initial = null): mixed",
            array_reduce,
        ),
        (
            "function array_walk(object|array &$array, callable $callback, mixed $arg = UNKNOWN): true",
            array_walk,
        ),
        (
            "function array_walk_recursive(object|array &$array, callable $callback, mixed $arg = UNKNOWN): true",
            array_walk_recursive,
        ),
        ("function array_sum(array $array): int|float", array_sum),
        (
            "function array_product(array $array): int|float",
            array_product,
        ),
        (
            "function array_unique(array $array, int $flags = SORT_STRING): array",
            array_unique,
        ),
        (
            "function array_count_values(array $array): array",
            array_count_values,
        ),
        (
            "function array_column(array $array, string|int|null $column_key, string|int|null $index_key = null): array",
            array_column,
        ),
        (
            "function array_chunk(array $array, int $length, bool $preserve_keys = false): array",
            array_chunk,
        ),
        (
            "function array_diff(array $array, array ...$arrays): array",
            array_diff,
        ),
        (
            "function array_diff_key(array $array, array ...$arrays): array",
            array_diff_key,
        ),
        (
            "function array_diff_assoc(array $array, array ...$arrays): array",
            array_diff_assoc,
        ),
        (
            "function array_udiff(array $array, ...$rest): array",
            array_udiff,
        ),
        (
            "function array_diff_ukey(array $array, ...$rest): array",
            array_diff_ukey,
        ),
        (
            "function array_diff_uassoc(array $array, ...$rest): array",
            array_diff_uassoc,
        ),
        (
            "function array_udiff_assoc(array $array, ...$rest): array",
            array_udiff_assoc,
        ),
        (
            "function array_udiff_uassoc(array $array, ...$rest): array",
            array_udiff_uassoc,
        ),
        (
            "function array_intersect(array $array, array ...$arrays): array",
            array_intersect,
        ),
        (
            "function array_intersect_key(array $array, array ...$arrays): array",
            array_intersect_key,
        ),
        (
            "function array_intersect_assoc(array $array, array ...$arrays): array",
            array_intersect_assoc,
        ),
        (
            "function array_uintersect(array $array, ...$rest): array",
            array_uintersect,
        ),
        (
            "function array_intersect_ukey(array $array, ...$rest): array",
            array_intersect_ukey,
        ),
        (
            "function array_intersect_uassoc(array $array, ...$rest): array",
            array_intersect_uassoc,
        ),
        (
            "function array_uintersect_assoc(array $array, ...$rest): array",
            array_uintersect_assoc,
        ),
        (
            "function array_uintersect_uassoc(array $array, ...$rest): array",
            array_uintersect_uassoc,
        ),
        (
            "function array_push(array &$array, mixed ...$values): int",
            array_push,
        ),
        ("function array_pop(array &$array): mixed", array_pop),
        ("function array_shift(array &$array): mixed", array_shift),
        (
            "function array_unshift(array &$array, mixed ...$values): int",
            array_unshift,
        ),
        (
            "function array_reverse(array $array, bool $preserve_keys = false): array",
            array_reverse,
        ),
        (
            "function array_rand(array $array, int $num = 1): array|string|int",
            array_rand,
        ),
        ("function shuffle(array &$array): true", shuffle),
        (
            "function sort(array &$array, int $flags = SORT_REGULAR): true",
            sort,
        ),
        (
            "function rsort(array &$array, int $flags = SORT_REGULAR): true",
            rsort,
        ),
        (
            "function asort(array &$array, int $flags = SORT_REGULAR): true",
            asort,
        ),
        (
            "function arsort(array &$array, int $flags = SORT_REGULAR): true",
            arsort,
        ),
        (
            "function ksort(array &$array, int $flags = SORT_REGULAR): true",
            ksort,
        ),
        (
            "function krsort(array &$array, int $flags = SORT_REGULAR): true",
            krsort,
        ),
        (
            "function usort(array &$array, callable $callback): true",
            usort,
        ),
        (
            "function uasort(array &$array, callable $callback): true",
            uasort,
        ),
        (
            "function uksort(array &$array, callable $callback): true",
            uksort,
        ),
        ("function natsort(array &$array): true", natsort),
        ("function natcasesort(array &$array): true", natcasesort),
        (
            "function array_multisort(&$array, &...$rest): true",
            array_multisort,
        ),
        ("function current(object|array $array): mixed", current),
        ("function pos(object|array $array): mixed", current),
        ("function key(object|array $array): string|int|null", key),
        ("function next(object|array &$array): mixed", next),
        ("function prev(object|array &$array): mixed", prev),
        ("function reset(object|array &$array): mixed", reset),
        ("function end(object|array &$array): mixed", end),
        ("function array_is_list(array $array): bool", array_is_list),
        (
            "function array_find(array $array, callable $callback): mixed",
            array_find,
        ),
        (
            "function array_find_key(array $array, callable $callback): mixed",
            array_find_key,
        ),
        (
            "function array_any(array $array, callable $callback): bool",
            array_any,
        ),
        (
            "function array_all(array $array, callable $callback): bool",
            array_all,
        ),
        (
            "function array_change_key_case(array $array, int $case = CASE_LOWER): array",
            array_change_key_case,
        ),
    ],
    constants: &[
        ("COUNT_NORMAL", ConstVal::Int(0)),
        ("COUNT_RECURSIVE", ConstVal::Int(1)),
        ("SORT_ASC", ConstVal::Int(4)),
        ("SORT_DESC", ConstVal::Int(3)),
        ("SORT_REGULAR", ConstVal::Int(0)),
        ("SORT_NUMERIC", ConstVal::Int(1)),
        ("SORT_STRING", ConstVal::Int(2)),
        ("SORT_LOCALE_STRING", ConstVal::Int(5)),
        ("SORT_NATURAL", ConstVal::Int(6)),
        ("SORT_FLAG_CASE", ConstVal::Int(8)),
        ("CASE_LOWER", ConstVal::Int(0)),
        ("CASE_UPPER", ConstVal::Int(1)),
        ("ARRAY_FILTER_USE_BOTH", ConstVal::Int(1)),
        ("ARRAY_FILTER_USE_KEY", ConstVal::Int(2)),
    ],
    listed: false,
    ..Module::EMPTY
};

const SORT_REGULAR: i64 = 0;
const SORT_NUMERIC: i64 = 1;
const SORT_STRING: i64 = 2;
const SORT_LOCALE_STRING: i64 = 5;
const SORT_NATURAL: i64 = 6;
const SORT_FLAG_CASE: i64 = 8;

// ---------------------------------------------------------------- helpers

fn ord_of(c: i32) -> Ordering {
    c.cmp(&0)
}

fn php_loose_eq(vm: &mut Vm, a: &Value, b: &Value) -> R<bool> {
    let r = loose_eq(a, b, vm);
    vm.check_deferred()?;
    r.map_err(|e| vm.php_error(e))
}

/// The array held by by-reference argument `i`, or the TypeError PHP gives
/// for another type (the engine leaves by-reference arguments unchecked).
fn ref_array(vm: &mut Vm, a: &Args, i: usize, pname: &str) -> R<(PhpRef, PhpArray)> {
    let r = match a.ref_at(i) {
        Some(r) => r,
        None => PhpRef::new(a.val(i)),
    };
    let v = r.get();
    match v {
        Value::Array(arr) => Ok((r, arr)),
        other => vm.fthrow(
            a,
            "TypeError",
            &format!(
                "Argument #{} (${}) must be of type array, {} given",
                i + 1,
                pname,
                Vm::type_name_given(&other)
            ),
        ),
    }
}

/// Takes the array out of a reference (so it can be modified without a
/// copy); the caller writes it back.
fn take_ref_array(r: &PhpRef) -> PhpArray {
    let mut b = r.borrow_mut();
    match std::mem::take(&mut *b) {
        Value::Array(a) => a,
        other => {
            *b = other;
            PhpArray::new()
        }
    }
}

fn key_value(k: KeyRef<'_>) -> Value {
    k.to_value()
}

/// Calls a callback, returning its result.
fn call(vm: &mut Vm, cb: &Value, args: Vec<Value>) -> R<Value> {
    vm.call_value(cb, args)
}

/// The array's elements as (key, value) pairs; shared references are kept.
fn entries(arr: &PhpArray) -> Vec<(Key, Value)> {
    // References only the array holds are plain values (their count would
    // otherwise go up with the copies made here).
    arr.iter()
        .map(|(k, v)| (k.to_owned(), deref_shared(v)))
        .collect()
}

/// Rebuilds a list (keys 0..n) from values.
fn list_of(vals: impl IntoIterator<Item = Value>) -> PhpArray {
    PhpArray::from_values(vals)
}

/// Appends with renumbering, as array_merge() does for int keys.
fn push_or_err(vm: &mut Vm, out: &mut PhpArray, v: Value) -> R<()> {
    if out.push(v).is_err() {
        return vm.throw(
            "Error",
            "Cannot add element to the array as the next element is already occupied",
        );
    }
    Ok(())
}

// ---------------------------------------------------------------- counting / searching

fn count(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let mode = a.int_or(1, 0);
    if mode != 0 && mode != 1 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($mode) must be either COUNT_NORMAL or COUNT_RECURSIVE",
        );
    }
    let v = a.val(0);
    if mode == 1
        && let Value::Array(arr) = &v
    {
        let mut seen = Vec::new();
        return Ok(Value::Int(count_recursive(vm, a, arr, &mut seen)?));
    }
    Ok(Value::Int(vm.count_value(&v, mode == 1)?))
}

fn count_recursive(vm: &mut Vm, a: &Args, arr: &PhpArray, seen: &mut Vec<*const ()>) -> R<i64> {
    let id = arr.data() as *const _ as *const ();
    if seen.contains(&id) {
        vm.fwarn(a, "Recursion detected")?;
        return Ok(0);
    }
    seen.push(id);
    let mut n = arr.len() as i64;
    for (_, v) in arr.iter() {
        if let Value::Array(inner) = v.deref() {
            n += count_recursive(vm, a, &inner, seen)?;
        }
    }
    seen.pop();
    Ok(n)
}

fn search(vm: &mut Vm, a: &Args) -> R<Option<Key>> {
    let needle = a.val(0);
    let hay = a.array(1);
    let strict = a.bool_or(2, false);
    for (k, v) in hay.iter() {
        let v = v.deref();
        let eq = if strict {
            strict_eq(&v, &needle)
        } else {
            php_loose_eq(vm, &v, &needle)?
        };
        if eq {
            return Ok(Some(k.to_owned()));
        }
    }
    Ok(None)
}

fn in_array(vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Bool(search(vm, a)?.is_some()))
}

fn array_search(vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(match search(vm, a)? {
        Some(k) => k.to_value(),
        None => Value::Bool(false),
    })
}

fn array_keys(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let arr = a.array(0);
    if !a.has(1) {
        return Ok(Value::Array(list_of(arr.iter().map(|(k, _)| key_value(k)))));
    }
    let needle = a.val(1);
    let strict = a.bool_or(2, false);
    let mut out = PhpArray::new();
    for (k, v) in arr.iter() {
        let v = v.deref();
        let eq = if strict {
            strict_eq(&v, &needle)
        } else {
            php_loose_eq(vm, &v, &needle)?
        };
        if eq {
            let _ = out.push(key_value(k));
        }
    }
    Ok(Value::Array(out))
}

fn array_values(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let arr = a.array(0);
    // Tests bug75433.phpt, bug75653.phpt: the result appends after its
    // last element.
    if arr.data().is_list()
        && arr.data().next_index() == Some(arr.len() as i64)
        && !arr.iter().any(|(_, v)| v.is_ref())
    {
        return Ok(Value::Array(arr));
    }
    // Test array_values_variation6.phpt: shared references stay.
    Ok(Value::Array(list_of(
        arr.iter().map(|(_, v)| deref_shared(v)),
    )))
}

// ---------------------------------------------------------------- merge / replace

fn arrays_from(vm: &mut Vm, a: &Args, start: usize) -> R<Vec<PhpArray>> {
    let mut out = Vec::new();
    for i in start..a.len() {
        match a.val(i) {
            Value::Array(arr) => out.push(arr),
            other => {
                return vm.fthrow(
                    a,
                    "TypeError",
                    &format!(
                        "Argument #{} must be of type array, {} given",
                        i + 1,
                        Vm::type_name_given(&other)
                    ),
                );
            }
        }
    }
    Ok(out)
}

fn array_merge(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let arrays = arrays_from(vm, a, 0)?;
    if arrays.len() == 1
        && arrays[0].data().is_list()
        && arrays[0].data().next_index() == Some(arrays[0].len() as i64)
    {
        let arr = &arrays[0];
        if !arr.iter().any(|(_, v)| v.is_ref()) {
            return Ok(Value::Array(arr.clone()));
        }
    }
    let total: usize = arrays.iter().map(|x| x.len()).sum();
    // Test GHSA-h96m-rvf9-jgm2.phpt.
    if total >= 1 << 30 {
        return vm.throw(
            "Error",
            &format!(
                "The total number of elements must be lower than {}",
                1u64 << 30
            ),
        );
    }
    let mut out = PhpArray::with_capacity(total);
    for arr in &arrays {
        for (k, v) in arr.iter() {
            let v = deref_shared(v);
            match k {
                KeyRef::Int(_) => push_or_err(vm, &mut out, v)?,
                KeyRef::Str(s) => {
                    out.insert(Key::Str(s.clone()), v);
                }
            }
        }
    }
    Ok(Value::Array(out))
}

/// An element copied out of an array: references held only by the array
/// are dereferenced (PHP manual: references inside arrays are kept only
/// when shared).
fn deref_shared(v: &Value) -> Value {
    match v {
        Value::Ref(r) if r.refcount() == 1 => r.get(),
        v => v.clone(),
    }
}

/// The address of a reference element, for recursion detection.
fn ref_addr(v: &Value) -> Option<usize> {
    match v {
        Value::Ref(r) => Some(r.addr()),
        _ => None,
    }
}

fn merge_recursive_into(
    vm: &mut Vm,
    out: &mut PhpArray,
    src: &PhpArray,
    stack: &mut Vec<usize>,
) -> R<()> {
    for (k, v) in src.iter() {
        match k {
            KeyRef::Int(_) => push_or_err(vm, out, deref_shared(v))?,
            KeyRef::Str(s) => {
                let key = Key::Str(s.clone());
                let existing = out.get(key.as_ref()).cloned();
                match existing {
                    None => {
                        out.insert(key, deref_shared(v));
                    }
                    Some(old) => {
                        // Tests bug42177.phpt, bug43495.phpt: merging an
                        // array into itself through a reference.
                        let addrs = [ref_addr(&old), ref_addr(v)];
                        if addrs.iter().flatten().any(|a| stack.contains(a)) {
                            return vm.throw("Error", "Recursion detected");
                        }
                        let n = stack.len();
                        stack.extend(addrs.iter().flatten());
                        // Tests gh16053.phpt (objects are cast) and
                        // array_merge_recursive_variation9.phpt (null kept).
                        let mut merged = match old.deref() {
                            Value::Array(x) => x,
                            o @ Value::Object(_) => {
                                match vm.cast(crate::bytecode::CastTo::Array, o)? {
                                    Value::Array(x) => x,
                                    _ => PhpArray::new(),
                                }
                            }
                            other => list_of([other]),
                        };
                        let r = match v.deref() {
                            Value::Array(inner) => {
                                merge_recursive_into(vm, &mut merged, &inner, stack)
                            }
                            other => push_or_err(vm, &mut merged, other),
                        };
                        stack.truncate(n);
                        r?;
                        out.insert(key, Value::Array(merged));
                    }
                }
            }
        }
    }
    Ok(())
}

fn array_merge_recursive(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let arrays = arrays_from(vm, a, 0)?;
    let mut out = PhpArray::new();
    let mut stack = Vec::new();
    for arr in &arrays {
        merge_recursive_into(vm, &mut out, arr, &mut stack)?;
    }
    Ok(Value::Array(out))
}

fn array_replace(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let arrays = arrays_from(vm, a, 0)?;
    let mut out = arrays[0].clone();
    for arr in &arrays[1..] {
        for (k, v) in arr.iter() {
            out.insert(k.to_owned(), deref_shared(v));
        }
    }
    Ok(Value::Array(out))
}

fn replace_recursive(
    vm: &mut Vm,
    base: &mut PhpArray,
    src: &PhpArray,
    stack: &mut Vec<usize>,
) -> R<()> {
    for (k, v) in src.iter() {
        let key = k.to_owned();
        let existing = base.get(key.as_ref()).cloned();
        match (existing.as_ref().map(|x| x.deref()), v.deref()) {
            (Some(Value::Array(mut inner)), Value::Array(srcin)) => {
                let addrs = [existing.as_ref().and_then(ref_addr), ref_addr(v)];
                if addrs.iter().flatten().any(|a| stack.contains(a)) {
                    return vm.throw("Error", "Recursion detected");
                }
                let n = stack.len();
                stack.extend(addrs.iter().flatten());
                let r = replace_recursive(vm, &mut inner, &srcin, stack);
                stack.truncate(n);
                r?;
                base.insert(key, Value::Array(inner));
            }
            _ => {
                base.insert(key, deref_shared(v));
            }
        }
    }
    Ok(())
}

fn array_replace_recursive(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let arrays = arrays_from(vm, a, 0)?;
    let mut out = arrays[0].clone();
    for arr in &arrays[1..] {
        replace_recursive(vm, &mut out, arr, &mut Vec::new())?;
    }
    Ok(Value::Array(out))
}

/// A value used as an array key by array_combine()/array_fill_keys():
/// ints stay, everything else goes through string conversion.
fn string_key(vm: &mut Vm, v: &Value) -> R<Key> {
    match v {
        Value::Int(i) => Ok(Key::Int(*i)),
        Value::Str(s) => Ok(Key::from_php_str(s)),
        other => {
            let s = vm.to_php_str(other)?;
            Ok(Key::from_php_str(&s))
        }
    }
}

fn array_combine(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let keys = a.array(0);
    let vals = a.array(1);
    if keys.len() != vals.len() {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($keys) and argument #2 ($values) must have the same number of elements",
        );
    }
    let mut out = PhpArray::with_capacity(keys.len());
    for ((_, k), (_, v)) in keys.iter().zip(vals.iter()) {
        let key = string_key(vm, &k.deref())?;
        out.insert(key, deref_shared(v));
    }
    Ok(Value::Array(out))
}

fn array_flip(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let arr = a.array(0);
    let mut out = PhpArray::with_capacity(arr.len());
    for (k, v) in arr.iter() {
        match v.deref() {
            Value::Int(i) => {
                out.insert(Key::Int(i), key_value(k));
            }
            Value::Str(s) => {
                out.insert(Key::from_php_str(&s), key_value(k));
            }
            _ => vm.fwarn(a, "Can only flip string and integer values, entry skipped")?,
        }
    }
    Ok(Value::Array(out))
}

fn array_fill(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let start = a.int(0);
    let n = a.int(1);
    let v = a.val(2);
    if n < 0 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($count) must be greater than or equal to 0",
        );
    }
    if n >= (1 << 31) {
        return vm.fthrow(a, "ValueError", "Argument #2 ($count) is too large");
    }
    if n == 0 {
        return Ok(Value::Array(PhpArray::new()));
    }
    if start == 0 {
        return Ok(Value::Array(list_of(std::iter::repeat_n(v, n as usize))));
    }
    let mut out = PhpArray::with_capacity(n as usize);
    out.insert(Key::Int(start), v.clone());
    for _ in 1..n {
        push_or_err(vm, &mut out, v.clone())?;
    }
    Ok(Value::Array(out))
}

fn array_fill_keys(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let keys = a.array(0);
    let v = a.val(1);
    let mut out = PhpArray::with_capacity(keys.len());
    for (_, k) in keys.iter() {
        let key = string_key(vm, &k.deref())?;
        out.insert(key, v.clone());
    }
    Ok(Value::Array(out))
}

fn array_pad(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let arr = a.array(0);
    let len = a.int(1);
    let v = a.val(2);
    let n = arr.len() as i64;
    let target = len.unsigned_abs();
    if target <= n as u64 {
        return Ok(Value::Array(arr));
    }
    if target > (1 << 26) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($length) must not exceed the maximum allowed array size",
        );
    }
    let pad = (target - n as u64) as usize;
    let mut out = PhpArray::with_capacity(target as usize);
    if len < 0 {
        for _ in 0..pad {
            let _ = out.push(v.clone());
        }
    }
    for (k, x) in arr.iter() {
        match k {
            KeyRef::Int(_) => push_or_err(vm, &mut out, deref_shared(x))?,
            KeyRef::Str(s) => {
                out.insert(Key::Str(s.clone()), deref_shared(x));
            }
        }
    }
    if len > 0 {
        for _ in 0..pad {
            push_or_err(vm, &mut out, v.clone())?;
        }
    }
    Ok(Value::Array(out))
}

// ---------------------------------------------------------------- range

/// A classified range() boundary.
#[derive(Clone, Copy)]
enum Bound {
    Int(i64),
    Float(f64),
}

/// A range() argument: its numeric value, and its byte when it is a
/// one-byte string (or a longer non-numeric one) eligible for a character
/// range.
struct RangeArg {
    num: Bound,
    ch: Option<u8>,
    /// Non-numeric string (a letter, not a digit).
    non_numeric: bool,
}

/// The largest array range() builds (observed on PHP 8.5: "Maximum size:
/// 1073741824").
const RANGE_MAX: u64 = 1 << 30;

/// A float as range()'s messages print it ("0.1", "100000000000.0").
fn range_float(vm: &Vm, f: f64) -> String {
    let mut out = Vec::new();
    vm.repr_float(f, &mut out);
    let mut s = String::from_utf8_lossy(&out).into_owned();
    if !s.contains(['.', 'E', 'N', 'I']) {
        s.push_str(".0");
    }
    s
}

fn range_classify(vm: &mut Vm, a: &Args, v: &Value, argn: usize) -> R<RangeArg> {
    use goro_runtime::numeric::numeric_value;
    let name = if argn == 1 { "start" } else { "end" };
    let plain = |num| RangeArg {
        num,
        ch: None,
        non_numeric: false,
    };
    Ok(match v {
        Value::Int(i) => plain(Bound::Int(*i)),
        Value::Float(f) => plain(Bound::Float(*f)),
        Value::Bool(b) => plain(Bound::Int(*b as i64)),
        Value::Null => {
            vm.fdeprecated(
                a,
                &format!(
                    "Passing null to parameter #{} (${}) of type string|int|float is deprecated",
                    argn, name
                ),
            )?;
            plain(Bound::Int(0))
        }
        Value::Str(s) => {
            if s.is_empty() {
                vm.fwarn(
                    a,
                    &format!(
                        "Argument #{} (${}) must not be empty, casted to 0",
                        argn, name
                    ),
                )?;
                return Ok(plain(Bound::Int(0)));
            }
            let num = match numeric_value(s) {
                Some(goro_runtime::Number::Int(i)) => Some(Bound::Int(i)),
                Some(goro_runtime::Number::Float(f)) => Some(Bound::Float(f)),
                None => None,
            };
            match num {
                Some(n) if s.len() > 1 => plain(n),
                Some(n) => RangeArg {
                    num: n,
                    ch: Some(s[0]),
                    non_numeric: false,
                },
                None => {
                    if s.len() > 1 {
                        vm.fwarn(
                            a,
                            &format!(
                                "Argument #{} (${}) must be a single byte, subsequent bytes are ignored",
                                argn, name
                            ),
                        )?;
                    }
                    RangeArg {
                        num: Bound::Int(0),
                        ch: Some(s[0]),
                        non_numeric: true,
                    }
                }
            }
        }
        Value::Object(o) if vm.class_of(o).magic.borrow().to_string.is_some() => {
            let s = vm.obj_to_string(o)?;
            return range_classify(vm, a, &Value::Str(s), argn);
        }
        other => {
            return vm.fthrow(
                a,
                "TypeError",
                &format!(
                    "Argument #{} (${}) must be of type string|int|float, {} given",
                    argn,
                    name,
                    Vm::type_name_given(other)
                ),
            );
        }
    })
}

/// range() (RFC wiki.php.net/rfc/proper-range-semantics; tests
/// ext/standard/tests/array/range/*.phpt; observed on PHP 8.5).
fn range(vm: &mut Vm, a: &mut Args) -> R<Value> {
    const SPANNED: &str = "Argument #3 ($step) must be less than the range spanned by argument #1 ($start) and argument #2 ($end)";
    const INCREASING: &str = "Argument #3 ($step) must be greater than 0 for increasing ranges";
    let step_v = if a.has(2) { a.val(2) } else { Value::Int(1) };
    let (step_f, step_frac) = match &step_v {
        Value::Int(i) => (*i as f64, false),
        Value::Float(f) => (*f, f.fract() != 0.0),
        _ => (1.0, false),
    };
    if step_f.is_nan() || step_f.is_infinite() {
        let what = if step_f.is_nan() { "NAN" } else { "INF" };
        return vm.fthrow(
            a,
            "ValueError",
            &format!(
                "Argument #3 ($step) must be a finite number, {} provided",
                what
            ),
        );
    }
    let step_neg = step_f < 0.0;
    let mut s = range_classify(vm, a, &a.val(0), 1)?;
    let mut e = range_classify(vm, a, &a.val(1), 2)?;
    for (n, r) in [(1, &s), (2, &e)] {
        if let Bound::Float(f) = r.num
            && !f.is_finite()
        {
            let name = if n == 1 { "start" } else { "end" };
            let what = if f.is_nan() { "NAN" } else { "INF" };
            return vm.fthrow(
                a,
                "ValueError",
                &format!(
                    "Argument #{} (${}) must be a finite number, {} provided",
                    n, name, what
                ),
            );
        }
    }
    if step_f == 0.0 {
        return vm.fthrow(a, "ValueError", "Argument #3 ($step) cannot be 0");
    }
    let float_step = step_frac;
    match (s.ch, e.ch) {
        (Some(x), Some(y)) => {
            if step_frac {
                if s.non_numeric || e.non_numeric {
                    vm.fwarn(
                        a,
                        "Argument #3 ($step) must be of type int when generating an array of characters, inputs converted to 0",
                    )?;
                    s.num = Bound::Int(0);
                    e.num = Bound::Int(0);
                }
            } else {
                // A character range.
                let st = step_f.abs() as u64;
                let mut out = PhpArray::new();
                if x == y {
                    let _ = out.push(Value::Str(PhpStr::from_byte(x)));
                    return Ok(Value::Array(out));
                }
                if x < y && step_neg {
                    return vm.fthrow(a, "ValueError", INCREASING);
                }
                let span = (x as i64 - y as i64).unsigned_abs();
                if st > span {
                    return vm.fthrow(a, "ValueError", SPANNED);
                }
                let mut c = x as i64;
                loop {
                    let _ = out.push(Value::Str(PhpStr::from_byte(c as u8)));
                    if x < y {
                        c += st as i64;
                        if c > y as i64 {
                            break;
                        }
                    } else {
                        c -= st as i64;
                        if c < y as i64 {
                            break;
                        }
                    }
                }
                return Ok(Value::Array(out));
            }
        }
        (Some(_), None) if s.non_numeric => {
            vm.fwarn(
                a,
                "Argument #2 ($end) must be a single byte string if argument #1 ($start) is a single byte string, argument #1 ($start) converted to 0",
            )?;
            s.num = Bound::Int(0);
        }
        (None, Some(_)) if e.non_numeric => {
            vm.fwarn(
                a,
                "Argument #1 ($start) must be a single byte string if argument #2 ($end) is a single byte string, argument #2 ($end) converted to 0",
            )?;
            e.num = Bound::Int(0);
        }
        _ => {}
    }
    let float_mode =
        float_step || matches!(s.num, Bound::Float(_)) || matches!(e.num, Bound::Float(_));
    let to_f = |b: Bound| match b {
        Bound::Int(i) => i as f64,
        Bound::Float(f) => f,
    };
    let mut out = PhpArray::new();
    if float_mode {
        let (x, y) = (to_f(s.num), to_f(e.num));
        let st = step_f.abs();
        if x == y {
            let _ = out.push(Value::Float(x));
            return Ok(Value::Array(out));
        }
        if x < y && step_neg {
            return vm.fthrow(a, "ValueError", INCREASING);
        }
        let span = (x - y).abs();
        if st > span {
            return vm.fthrow(a, "ValueError", SPANNED);
        }
        // Observed on PHP 8.5: range(4.5, 4.2, 0.1) has 4 elements and
        // range(9.9, 0) has 10 elements: the quotient is taken to 15
        // significant digits before truncation.
        let q = span / st;
        let n = format!("{:.14e}", q).parse::<f64>().unwrap_or(q).floor();
        if n + 1.0 > RANGE_MAX as f64 {
            let msg = format!(
                "The supplied range exceeds the maximum array size by {} elements: start={}, end={}, step={}. Max size: {}",
                range_float(vm, n + 1.0 - RANGE_MAX as f64),
                range_float(vm, x),
                range_float(vm, y),
                range_float(vm, st),
                RANGE_MAX
            );
            return vm.throw("ValueError", &msg);
        }
        let n = n as u64;
        let mut out = PhpArray::with_capacity(n as usize + 1);
        for i in 0..=n {
            let v = if x < y {
                x + i as f64 * st
            } else {
                x - i as f64 * st
            };
            let _ = out.push(Value::Float(v));
        }
        return Ok(Value::Array(out));
    }
    let (Bound::Int(x), Bound::Int(y)) = (s.num, e.num) else {
        unreachable!()
    };
    if x == y {
        let _ = out.push(Value::Int(x));
        return Ok(Value::Array(out));
    }
    if x < y && step_neg {
        return vm.fthrow(a, "ValueError", INCREASING);
    }
    let st = step_f.abs() as u64;
    let span = (x as i128 - y as i128).unsigned_abs() as u64;
    if st > span {
        return vm.fthrow(a, "ValueError", SPANNED);
    }
    let calc = span / st;
    if calc >= RANGE_MAX {
        let msg = format!(
            "The supplied range exceeds the maximum array size by {} elements: start={}, end={}, step={}. Calculated size: {}. Maximum size: {}.",
            calc.wrapping_add(1).wrapping_sub(RANGE_MAX),
            x,
            y,
            st,
            calc,
            RANGE_MAX
        );
        return vm.throw("ValueError", &msg);
    }
    let mut out = PhpArray::with_capacity(calc as usize + 1);
    for i in 0..=calc {
        let v = if x < y {
            (x as i128 + i as i128 * st as i128) as i64
        } else {
            (x as i128 - i as i128 * st as i128) as i64
        };
        let _ = out.push(Value::Int(v));
    }
    Ok(Value::Array(out))
}

// ---------------------------------------------------------------- keys

fn array_key_exists(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let k = a.val(0);
    let arr = a.array(1);
    let key = match &k {
        Value::Int(i) => Key::Int(*i),
        Value::Str(s) => Key::from_php_str(s),
        Value::Null => {
            vm.deprecated(
                "Using null as the key parameter for array_key_exists() is deprecated, use an empty string instead",
            )?;
            Key::Str(PhpStr::empty())
        }
        other => vm.to_key(other, "array")?,
    };
    Ok(Value::Bool(arr.get(key.as_ref()).is_some()))
}

fn array_key_first(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let arr = a.array(0);
    Ok(arr
        .iter()
        .next()
        .map(|(k, _)| key_value(k))
        .unwrap_or(Value::Null))
}

fn array_key_last(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let arr = a.array(0);
    let d = arr.data();
    Ok(match d.last_pos().and_then(|p| d.key_at(p)) {
        Some(k) => key_value(k),
        None => Value::Null,
    })
}

fn array_first(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let arr = a.array(0);
    Ok(arr
        .iter()
        .next()
        .map(|(_, v)| v.deref())
        .unwrap_or(Value::Null))
}

fn array_last(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let arr = a.array(0);
    let d = arr.data();
    Ok(match d.last_pos().and_then(|p| d.entry_at(p)) {
        Some((_, v)) => v.deref(),
        None => Value::Null,
    })
}

fn array_is_list(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Bool(a.array(0).data().is_list()))
}

fn array_change_key_case(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let arr = a.array(0);
    let upper = a.int_or(1, 0) != 0;
    let mut out = PhpArray::with_capacity(arr.len());
    for (k, v) in arr.iter() {
        let key = match k {
            KeyRef::Int(i) => Key::Int(i),
            KeyRef::Str(s) => {
                let b: Vec<u8> = if upper {
                    s.to_ascii_uppercase()
                } else {
                    s.to_ascii_lowercase()
                };
                Key::Str(PhpStr::from(b))
            }
        };
        out.insert(key, deref_shared(v));
    }
    Ok(Value::Array(out))
}

// ---------------------------------------------------------------- slicing

/// Resolves an offset/length pair against `n` elements (array_slice and
/// array_splice): the start index and the count.
fn slice_bounds(n: usize, offset: i64, length: Option<i64>) -> (usize, usize) {
    let n = n as i64;
    let start = if offset < 0 {
        (n + offset).max(0)
    } else {
        offset.min(n)
    };
    let end = match length {
        None => n,
        Some(l) if l < 0 => (n + l).max(start),
        Some(l) => start.saturating_add(l).min(n),
    };
    (start as usize, (end - start).max(0) as usize)
}

fn array_slice(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let arr = a.array(0);
    let preserve = a.bool_or(3, false);
    let (start, cnt) = slice_bounds(arr.len(), a.int(1), a.opt_int(2));
    let mut out = PhpArray::with_capacity(cnt);
    for (k, v) in arr.iter().skip(start).take(cnt) {
        let v = deref_shared(v);
        match k {
            KeyRef::Int(i) if preserve => {
                out.insert(Key::Int(i), v);
            }
            KeyRef::Int(_) => {
                let _ = out.push(v);
            }
            KeyRef::Str(s) => {
                out.insert(Key::Str(s.clone()), v);
            }
        }
    }
    Ok(Value::Array(out))
}

fn array_splice(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (r, arr) = ref_array(vm, a, 0, "array")?;
    let (start, cnt) = slice_bounds(arr.len(), a.int(1), a.opt_int(2));
    let replacement: Vec<Value> = if a.has(3) {
        match a.val(3) {
            Value::Array(x) => x.iter().map(|(_, v)| deref_shared(v)).collect(),
            Value::Null => Vec::new(),
            other => vec![other],
        }
    } else {
        Vec::new()
    };
    drop(arr);
    let arr = take_ref_array(&r);
    let mut removed = PhpArray::new();
    let mut out = PhpArray::with_capacity(arr.len() - cnt + replacement.len());
    for (i, (k, v)) in arr.iter().enumerate() {
        if i == start {
            for x in &replacement {
                let _ = out.push(x.clone());
            }
        }
        if i >= start && i < start + cnt {
            match k {
                KeyRef::Int(_) => {
                    let _ = removed.push(v.clone());
                }
                KeyRef::Str(s) => {
                    removed.insert(Key::Str(s.clone()), v.clone());
                }
            }
            continue;
        }
        match k {
            KeyRef::Int(_) => {
                let _ = out.push(v.clone());
            }
            KeyRef::Str(s) => {
                out.insert(Key::Str(s.clone()), v.clone());
            }
        }
    }
    if start >= arr.len() {
        for x in replacement {
            let _ = out.push(x);
        }
    }
    drop(arr);
    r.set(Value::Array(out));
    Ok(Value::Array(removed))
}

// ---------------------------------------------------------------- callbacks

fn array_map(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let cb = a.val(0);
    let arrays = arrays_from(vm, a, 1)?;
    if arrays.len() == 1 {
        let arr = &arrays[0];
        if cb.is_null() {
            return Ok(Value::Array(arr.clone()));
        }
        let mut out = PhpArray::with_capacity(arr.len());
        for (k, v) in entries(arr) {
            let r = call(vm, &cb, vec![v.deref()])?;
            out.insert(k, r);
        }
        return Ok(Value::Array(out));
    }
    let maxlen = arrays.iter().map(|x| x.len()).max().unwrap_or(0);
    // Without a callback the elements are zipped as they are (test
    // array_map_variation2.phpt keeps references).
    let cols: Vec<Vec<Value>> = arrays
        .iter()
        .map(|x| {
            x.iter()
                .map(|(_, v)| {
                    if cb.is_null() {
                        deref_shared(v)
                    } else {
                        v.deref()
                    }
                })
                .collect()
        })
        .collect();
    let mut out = PhpArray::with_capacity(maxlen);
    for i in 0..maxlen {
        let args: Vec<Value> = cols
            .iter()
            .map(|c| c.get(i).cloned().unwrap_or(Value::Null))
            .collect();
        let v = if cb.is_null() {
            Value::Array(list_of(args))
        } else {
            call(vm, &cb, args)?
        };
        let _ = out.push(v);
    }
    Ok(Value::Array(out))
}

fn array_filter(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let arr = a.array(0);
    let cb = if a.has(1) { a.val(1) } else { Value::Null };
    let mode = a.int_or(2, 0);
    let mut out = PhpArray::new();
    for (k, v) in entries(&arr) {
        let keep = if cb.is_null() {
            vm.to_bool(&v.deref())
        } else {
            let args = match mode {
                2 => vec![k.to_value()],
                1 => vec![v.deref(), k.to_value()],
                _ => vec![v.deref()],
            };
            let r = call(vm, &cb, args)?;
            vm.to_bool(&r)
        };
        if keep {
            out.insert(k, deref_shared(&v));
        }
    }
    Ok(Value::Array(out))
}

fn array_reduce(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let arr = a.array(0);
    let cb = a.val(1);
    let mut acc = if a.has(2) { a.val(2) } else { Value::Null };
    for (_, v) in entries(&arr) {
        acc = call(vm, &cb, vec![acc, v.deref()])?;
    }
    Ok(acc)
}

fn find_common(vm: &mut Vm, a: &Args) -> R<Option<(Key, Value)>> {
    let arr = a.array(0);
    let cb = a.val(1);
    for (k, v) in entries(&arr) {
        let v = v.deref();
        let r = call(vm, &cb, vec![v.clone(), k.to_value()])?;
        if vm.to_bool(&r) {
            return Ok(Some((k, v)));
        }
    }
    Ok(None)
}

fn array_find(vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(find_common(vm, a)?.map(|x| x.1).unwrap_or(Value::Null))
}
fn array_find_key(vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(find_common(vm, a)?
        .map(|x| x.0.to_value())
        .unwrap_or(Value::Null))
}
fn array_any(vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Bool(find_common(vm, a)?.is_some()))
}
fn array_all(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let arr = a.array(0);
    let cb = a.val(1);
    for (k, v) in entries(&arr) {
        let r = call(vm, &cb, vec![v.deref(), k.to_value()])?;
        if !vm.to_bool(&r) {
            return Ok(Value::Bool(false));
        }
    }
    Ok(Value::Bool(true))
}

/// Makes element `k` of the array in `r` a reference and returns it, or
/// None if the variable no longer holds an array with that key.
fn element_ref(r: &PhpRef, k: &Key) -> Option<PhpRef> {
    let mut b = r.borrow_mut();
    let Value::Array(arr) = &mut *b else {
        return None;
    };
    arr.get(k.as_ref())?;
    let slot = arr.make_mut().get_mut(k.as_ref())?;
    Some(slot.make_ref())
}

/// After a callback: an element reference only the array holds goes back
/// to a plain value.
fn unref_element(r: &PhpRef, k: &Key) {
    let mut b = r.borrow_mut();
    let Value::Array(arr) = &mut *b else {
        return;
    };
    let shared = match arr.get(k.as_ref()) {
        Some(Value::Ref(er)) => er.refcount() > 1,
        _ => return,
    };
    if shared {
        return;
    }
    if let Some(slot) = arr.make_mut().get_mut(k.as_ref())
        && let Value::Ref(er) = slot
    {
        let v = er.get();
        *slot = v;
    }
}

fn array_identity(r: &PhpRef) -> Option<*const goro_runtime::ArrayData> {
    match &*r.borrow() {
        Value::Array(a) => Some(a.data() as *const _),
        _ => None,
    }
}

fn walk_ref(
    vm: &mut Vm,
    r: &PhpRef,
    cb: &Value,
    extra: Option<&Value>,
    recursive: bool,
    depth: usize,
) -> R<()> {
    let snapshot = |r: &PhpRef| -> Vec<Key> {
        match &*r.borrow() {
            Value::Array(arr) => arr.iter().map(|(k, _)| k.to_owned()).collect(),
            _ => Vec::new(),
        }
    };
    let mut keys = snapshot(r);
    let mut idx = 0;
    while idx < keys.len() {
        let k = keys[idx].clone();
        idx += 1;
        let Some(er) = element_ref(r, &k) else {
            continue;
        };
        if recursive && matches!(&*er.borrow(), Value::Array(_)) {
            if depth > 256 {
                vm.warn("array_walk_recursive(): Recursion detected")?;
                continue;
            }
            walk_ref(vm, &er, cb, extra, recursive, depth + 1)?;
            drop(er);
            unref_element(r, &k);
            continue;
        }
        let mut args = vec![Value::Ref(er.clone()), k.to_value()];
        if let Some(x) = extra {
            args.push(x.clone());
        }
        drop(er);
        let ident = array_identity(r);
        let res = call(vm, cb, args);
        res?;
        // Test bug70713.phpt: the callback replaced the walked variable.
        let now = array_identity(r);
        if now.is_none() {
            return vm.throw(
                "TypeError",
                "Iterated value is no longer an array or object",
            );
        }
        if now != ident {
            // Test bug69068.phpt: a new array is walked from its start.
            let still_same = match &*r.borrow() {
                Value::Array(arr) => arr.get(k.as_ref()).is_some_and(|v| v.is_ref()),
                _ => false,
            };
            if !still_same {
                keys = snapshot(r);
                idx = 0;
                continue;
            }
        }
        unref_element(r, &k);
    }
    Ok(())
}

fn array_walk_common(vm: &mut Vm, a: &mut Args, recursive: bool) -> R<Value> {
    let cb = a.val(1);
    let extra = if a.has(2) { Some(a.val(2)) } else { None };
    let r = match a.ref_at(0) {
        Some(r) => r,
        None => PhpRef::new(a.val(0)),
    };
    let target = r.get();
    match target {
        Value::Array(_) => walk_ref(vm, &r, &cb, extra.as_ref(), recursive, 0)?,
        Value::Object(o) => {
            // The property table, with mangled names for private and
            // protected properties (test array_walk/array_walk_objects
            // .phpt); changed public values are written back.
            let props = vm.object_props_array(&o, true);
            for (k, v) in entries(&props) {
                let old = v.deref();
                let pr = PhpRef::new(old.clone());
                let mut args = vec![Value::Ref(pr.clone()), k.to_value()];
                if let Some(x) = &extra {
                    args.push(x.clone());
                }
                call(vm, &cb, args)?;
                let nv = pr.get();
                if let Key::Str(name) = &k
                    && !name.starts_with(b"\0")
                    && !goro_runtime::compare::strict_eq(&nv, &old)
                {
                    vm.write_prop(&o, name, nv, None)?;
                }
            }
        }
        other => {
            return vm.fthrow(
                a,
                "TypeError",
                &format!(
                    "Argument #1 ($array) must be of type array, {} given",
                    Vm::type_name_given(&other)
                ),
            );
        }
    }
    Ok(Value::Bool(true))
}

fn array_walk(vm: &mut Vm, a: &mut Args) -> R<Value> {
    array_walk_common(vm, a, false)
}
fn array_walk_recursive(vm: &mut Vm, a: &mut Args) -> R<Value> {
    array_walk_common(vm, a, true)
}

// ---------------------------------------------------------------- sum / product

fn sum_product(vm: &mut Vm, a: &mut Args, product: bool) -> R<Value> {
    use goro_runtime::ops;
    let arr = a.array(0);
    let mut acc = Value::Int(if product { 1 } else { 0 });
    let what = if product {
        "Multiplication"
    } else {
        "Addition"
    };
    for (_, v) in arr.iter() {
        let v = v.deref();
        match &v {
            Value::Array(_) => {
                vm.fwarn(a, &format!("{} is not supported on type array", what))?;
                continue;
            }
            // Objects with a numeric cast (GMP) count as their number
            // (test ext/gmp/tests/gh16890.phpt).
            Value::Object(o)
                if let Some(h) = vm.class_of(o).native.cast
                    && let Some(r) = h(vm, o, crate::bytecode::CastTo::Int) =>
            {
                let n = r?;
                let r = if product {
                    ops::mul(&acc, &n, vm)
                } else {
                    ops::add(&acc, &n, vm)
                };
                acc = r.map_err(|e| vm.php_error(e))?;
                continue;
            }
            Value::Object(o) => {
                let n = o.class_name().to_string_lossy();
                vm.fwarn(a, &format!("{} is not supported on type {}", what, n))?;
                continue;
            }
            Value::Resource(_) => {
                vm.fwarn(a, &format!("{} is not supported on type resource", what))?;
                // Resources count as their id.
            }
            Value::Str(s)
                if goro_runtime::numeric::numeric_value(s).is_none() && !leading_numeric(s) =>
            {
                vm.fwarn(a, &format!("{} is not supported on type string", what))?;
                let n = vm.cast(crate::bytecode::CastTo::Int, v.clone());
                let n = match goro_runtime::numeric::numeric_value(s) {
                    Some(goro_runtime::Number::Float(f)) => Value::Float(f),
                    _ => n?,
                };
                let r = if product {
                    ops::mul(&acc, &n, vm)
                } else {
                    ops::add(&acc, &n, vm)
                };
                vm.check_deferred()?;
                acc = r.map_err(|e| vm.php_error(e))?;
                continue;
            }
            _ => {}
        }
        let operand = match &v {
            Value::Resource(_) => vm.cast(crate::bytecode::CastTo::Int, v.clone())?,
            Value::Null => Value::Int(0),
            Value::Bool(b) => Value::Int(*b as i64),
            _ => v.clone(),
        };
        let r = if product {
            ops::mul(&acc, &operand, vm)
        } else {
            ops::add(&acc, &operand, vm)
        };
        vm.check_deferred()?;
        acc = r.map_err(|e| vm.php_error(e))?;
    }
    Ok(acc)
}

/// Whether a string starts with a number (leading-numeric: "5abc").
fn leading_numeric(s: &[u8]) -> bool {
    let mut i = 0;
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
        i += 1;
    }
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        i += 1;
    }
    if i < s.len() && s[i] == b'.' {
        i += 1;
    }
    i < s.len() && s[i].is_ascii_digit()
}

fn array_sum(vm: &mut Vm, a: &mut Args) -> R<Value> {
    sum_product(vm, a, false)
}
fn array_product(vm: &mut Vm, a: &mut Args) -> R<Value> {
    sum_product(vm, a, true)
}

// ---------------------------------------------------------------- sorting

/// A comparison under sort `flags`.
fn flag_compare(vm: &mut Vm, flags: i64, x: &Value, y: &Value) -> R<Ordering> {
    let base = flags & !SORT_FLAG_CASE;
    let ci = flags & SORT_FLAG_CASE != 0;
    Ok(match base {
        SORT_NUMERIC => {
            let fx = numeric_of(vm, x)?;
            let fy = numeric_of(vm, y)?;
            fx.partial_cmp(&fy).unwrap_or(Ordering::Equal)
        }
        SORT_STRING | SORT_LOCALE_STRING => {
            let sx = vm.to_php_str(x)?;
            let sy = vm.to_php_str(y)?;
            if ci {
                std_ext::strings::strcasecmp(&sx, &sy).cmp(&0)
            } else {
                sx.as_bytes().cmp(sy.as_bytes())
            }
        }
        SORT_NATURAL => {
            let sx = vm.to_php_str(x)?;
            let sy = vm.to_php_str(y)?;
            std_ext::natcmp::natcmp(&sx, &sy, ci)
        }
        _ => ord_of(php_compare(vm, x, y)?),
    })
}

fn numeric_of(vm: &mut Vm, v: &Value) -> R<f64> {
    Ok(match v {
        Value::Int(i) => *i as f64,
        Value::Float(f) => *f,
        Value::Str(s) => std_ext::printf::str_to_float(s),
        other => vm.to_float(other)?,
    })
}

/// Sorts `items` with a fallible comparator, PHP's algorithm and stability.
fn sort_items<T>(
    vm: &mut Vm,
    items: &mut [T],
    mut cmp: impl FnMut(&mut Vm, &T, &T) -> R<Ordering>,
) -> R<()> {
    let mut err: Option<Unwind> = None;
    std_ext::sort::stable_sort_by(items, |x, y| {
        if err.is_some() {
            return Ordering::Equal;
        }
        match cmp(vm, x, y) {
            Ok(o) => o,
            Err(e) => {
                err = Some(e);
                Ordering::Equal
            }
        }
    });
    match err {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// The ordering a user comparator's result means (bools are deprecated
/// and resolved by asking again with the arguments swapped).
fn user_cmp(
    vm: &mut Vm,
    a: &Args,
    cb: &Value,
    x: Value,
    y: Value,
    warned: &mut bool,
) -> R<Ordering> {
    let r = call(vm, cb, vec![x.clone(), y.clone()])?;
    if let Value::Bool(b) = r {
        if !*warned {
            *warned = true;
            vm.fdeprecated(
                a,
                "Returning bool from comparison function is deprecated, return an integer less than, equal to, or greater than zero",
            )?;
        }
        if b {
            return Ok(Ordering::Greater);
        }
        let r2 = call(vm, cb, vec![y, x])?;
        return Ok(if vm.to_bool(&r2) {
            Ordering::Less
        } else {
            Ordering::Equal
        });
    }
    let n = match r {
        Value::Int(i) => i,
        Value::Float(f) => {
            if f.is_nan() {
                0
            } else {
                f as i64
            }
        }
        other => vm.to_int(&other)?,
    };
    Ok(n.cmp(&0))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SortBy {
    Value,
    Key,
}

/// The shared body of the sort functions.
fn do_sort(
    vm: &mut Vm,
    a: &mut Args,
    by: SortBy,
    reverse: bool,
    keep_keys: bool,
    user: bool,
    flags: i64,
) -> R<Value> {
    let (r, arr) = ref_array(vm, a, 0, "array")?;
    let cb = if user { a.val(1) } else { Value::Null };
    let mut items = entries(&arr);
    drop(arr);
    let mut warned = false;
    sort_items(vm, &mut items, |vm, x, y| {
        let (pv, qv) = match by {
            SortBy::Value => (x.1.deref(), y.1.deref()),
            SortBy::Key => (x.0.to_value(), y.0.to_value()),
        };
        let o = if user {
            user_cmp(vm, a, &cb, pv, qv, &mut warned)?
        } else {
            flag_compare(vm, flags, &pv, &qv)?
        };
        // Reverse sorts negate the comparison (test
        // sort/rsort_variation9.phpt: uncomparable arrays).
        Ok(if reverse { o.reverse() } else { o })
    })?;
    let out = if keep_keys {
        PhpArray::from_pairs(items)
    } else {
        list_of(items.into_iter().map(|(_, v)| v))
    };
    r.set(Value::Array(out));
    Ok(Value::Bool(true))
}

fn sort(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.int_or(1, SORT_REGULAR);
    do_sort(vm, a, SortBy::Value, false, false, false, f)
}
fn rsort(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.int_or(1, SORT_REGULAR);
    do_sort(vm, a, SortBy::Value, true, false, false, f)
}
fn asort(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.int_or(1, SORT_REGULAR);
    do_sort(vm, a, SortBy::Value, false, true, false, f)
}
fn arsort(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.int_or(1, SORT_REGULAR);
    do_sort(vm, a, SortBy::Value, true, true, false, f)
}
fn ksort(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.int_or(1, SORT_REGULAR);
    do_sort(vm, a, SortBy::Key, false, true, false, f)
}
fn krsort(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.int_or(1, SORT_REGULAR);
    do_sort(vm, a, SortBy::Key, true, true, false, f)
}
fn usort(vm: &mut Vm, a: &mut Args) -> R<Value> {
    do_sort(vm, a, SortBy::Value, false, false, true, 0)
}
fn uasort(vm: &mut Vm, a: &mut Args) -> R<Value> {
    do_sort(vm, a, SortBy::Value, false, true, true, 0)
}
fn uksort(vm: &mut Vm, a: &mut Args) -> R<Value> {
    do_sort(vm, a, SortBy::Key, false, true, true, 0)
}
fn natsort(vm: &mut Vm, a: &mut Args) -> R<Value> {
    do_sort(vm, a, SortBy::Value, false, true, false, SORT_NATURAL)
}
fn natcasesort(vm: &mut Vm, a: &mut Args) -> R<Value> {
    do_sort(
        vm,
        a,
        SortBy::Value,
        false,
        true,
        false,
        SORT_NATURAL | SORT_FLAG_CASE,
    )
}

fn array_multisort(vm: &mut Vm, a: &mut Args) -> R<Value> {
    // Arrays, each followed by an optional order and an optional sort flag.
    struct Col {
        r: Option<PhpRef>,
        arr: PhpArray,
        desc: bool,
        flags: i64,
    }
    let mut cols: Vec<Col> = Vec::new();
    let mut order_set = false;
    let mut flag_set = false;
    for i in 0..a.len() {
        let v = a.val(i);
        // Tests array_multisort_variation1/2.phpt: only the first
        // parameter is named in messages.
        let what = if i == 0 {
            "Argument #1 ($array)".to_string()
        } else {
            format!("Argument #{}", i + 1)
        };
        match v {
            Value::Array(arr) => {
                cols.push(Col {
                    r: a.ref_at(i),
                    arr,
                    desc: false,
                    flags: SORT_REGULAR,
                });
                order_set = false;
                flag_set = false;
            }
            Value::Int(n) => {
                let is_order = n == 3 || n == 4;
                let is_flag = matches!(n & !SORT_FLAG_CASE, 0 | 1 | 2 | 5 | 6);
                if !is_order && !is_flag {
                    return vm.fthrow(
                        a,
                        "ValueError",
                        &format!("{} must be a valid sort flag", what),
                    );
                }
                let dup = cols.is_empty() || (is_order && order_set) || (is_flag && flag_set);
                if dup {
                    return vm.fthrow(
                        a,
                        "TypeError",
                        &format!(
                            "{} must be an array or a sort flag that has not already been specified",
                            what
                        ),
                    );
                }
                let col = cols.last_mut().unwrap();
                if is_order {
                    order_set = true;
                    col.desc = n == 3;
                } else {
                    flag_set = true;
                    col.flags = n;
                }
            }
            _ => {
                return vm.fthrow(
                    a,
                    "TypeError",
                    &format!("{} must be an array or a sort flag", what),
                );
            }
        }
    }
    if cols.is_empty() {
        return Ok(Value::Bool(true));
    }
    let n = cols[0].arr.len();
    if cols.iter().any(|c| c.arr.len() != n) {
        return vm.throw("ValueError", "Array sizes are inconsistent");
    }
    let data: Vec<Vec<(Key, Value)>> = cols.iter().map(|c| entries(&c.arr)).collect();
    let mut idx: Vec<usize> = (0..n).collect();
    let specs: Vec<(bool, i64)> = cols.iter().map(|c| (c.desc, c.flags)).collect();
    sort_items(vm, &mut idx, |vm, &x, &y| {
        for (c, &(desc, flags)) in specs.iter().enumerate() {
            let o = flag_compare(vm, flags, &data[c][x].1.deref(), &data[c][y].1.deref())?;
            let o = if desc { o.reverse() } else { o };
            if o != Ordering::Equal {
                return Ok(o);
            }
        }
        Ok(Ordering::Equal)
    })?;
    for (c, col) in cols.iter().enumerate() {
        let mut out = PhpArray::with_capacity(n);
        for &i in &idx {
            let (k, v) = &data[c][i];
            match k {
                Key::Int(_) => {
                    let _ = out.push(v.clone());
                }
                Key::Str(s) => {
                    out.insert(Key::Str(s.clone()), v.clone());
                }
            }
        }
        if let Some(r) = &col.r {
            r.set(Value::Array(out));
        }
    }
    Ok(Value::Bool(true))
}

// ---------------------------------------------------------------- unique / count / column / chunk

fn array_unique(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let arr = a.array(0);
    let flags = a.int_or(1, SORT_STRING);
    let items = entries(&arr);
    if flags == SORT_STRING {
        let mut seen = std::collections::HashSet::new();
        let mut out = PhpArray::new();
        for (k, v) in items {
            let s = vm.to_php_str(&v.deref())?;
            if seen.insert(s) {
                out.insert(k, deref_shared(&v));
            }
        }
        return Ok(Value::Array(out));
    }
    let n = items.len();
    let mut idx: Vec<usize> = (0..n).collect();
    sort_items(vm, &mut idx, |vm, &x, &y| {
        flag_compare(vm, flags, &items[x].1.deref(), &items[y].1.deref())
    })?;
    let mut drop_it = vec![false; n];
    let mut last = None::<usize>;
    for &i in &idx {
        match last {
            Some(l)
                if flag_compare(vm, flags, &items[l].1.deref(), &items[i].1.deref())?
                    == Ordering::Equal =>
            {
                drop_it[i] = true;
            }
            _ => last = Some(i),
        }
    }
    let mut out = PhpArray::new();
    for (i, (k, v)) in items.into_iter().enumerate() {
        if !drop_it[i] {
            out.insert(k, deref_shared(&v));
        }
    }
    Ok(Value::Array(out))
}

fn array_count_values(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let arr = a.array(0);
    let mut out = PhpArray::new();
    for (_, v) in arr.iter() {
        let key = match v.deref() {
            Value::Int(i) => Key::Int(i),
            Value::Str(s) => Key::from_php_str(&s),
            _ => {
                vm.fwarn(a, "Can only count string and integer values, entry skipped")?;
                continue;
            }
        };
        let e = out.entry(key);
        *e = match e {
            Value::Int(n) => Value::Int(*n + 1),
            _ => Value::Int(1),
        };
    }
    Ok(Value::Array(out))
}

/// A column of a row (array_column()): array element or accessible
/// object property.
fn row_get(vm: &mut Vm, row: &Value, k: &Value) -> R<Option<Value>> {
    match row {
        Value::Array(arr) => {
            let key = match k {
                Value::Int(i) => Key::Int(*i),
                Value::Str(s) => Key::from_php_str(s),
                _ => return Ok(None),
            };
            Ok(arr.get(key.as_ref()).map(|v| v.deref()))
        }
        Value::Object(o) => {
            let name = match k {
                Value::Str(s) => s.clone(),
                Value::Int(i) => PhpStr::from(i.to_string()),
                _ => return Ok(None),
            };
            // Test bug72031.phpt: a visible property counts even when null.
            for (pk, pv) in vm.visible_props(o) {
                let matches = match &pk {
                    Value::Str(a) => a.as_bytes() == name.as_bytes(),
                    Value::Int(i) => i.to_string().as_bytes() == name.as_bytes(),
                    _ => false,
                };
                if matches {
                    return Ok(Some(pv.deref()));
                }
            }
            // Magic properties: __isset() decides, __get() reads.
            let cls = vm.class_of(o);
            let has_magic = cls.magic.borrow().get.is_some();
            if !has_magic || !vm.prop_isset(o, &name, false)? {
                return Ok(None);
            }
            let v = vm.call_method(o, b"__get", vec![Value::Str(name)])?;
            Ok(Some(v))
        }
        _ => Ok(None),
    }
}

fn array_column(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let arr = a.array(0);
    let col = a.val(1);
    let idx = if a.has(2) { a.val(2) } else { Value::Null };
    let mut out = PhpArray::new();
    for (_, row) in entries(&arr) {
        let row = row.deref();
        let v = if col.is_null() {
            Some(row.clone())
        } else {
            row_get(vm, &row, &col)?
        };
        let Some(v) = v else { continue };
        let key = if idx.is_null() {
            None
        } else {
            row_get(vm, &row, &idx)?
        };
        match key {
            Some(Value::Int(i)) => {
                out.insert(Key::Int(i), v);
            }
            Some(Value::Str(s)) => {
                out.insert(Key::from_php_str(&s), v);
            }
            Some(other @ (Value::Null | Value::Bool(_) | Value::Float(_))) => {
                let key = vm.to_key(&other, "array")?;
                out.insert(key, v);
            }
            Some(Value::Object(o)) if vm.class_of(&o).magic.borrow().to_string.is_some() => {
                let s = vm.obj_to_string(&o)?;
                out.insert(Key::from_php_str(&s), v);
            }
            Some(other) => {
                return vm.type_error(&format!(
                    "Cannot access offset of type {} on array",
                    other.type_name_for_error()
                ));
            }
            None => push_or_err(vm, &mut out, v)?,
        }
    }
    Ok(Value::Array(out))
}

fn array_chunk(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let arr = a.array(0);
    let size = a.int(1);
    let preserve = a.bool_or(2, false);
    if size < 1 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($length) must be greater than 0",
        );
    }
    let mut out = PhpArray::new();
    let mut cur = PhpArray::new();
    for (k, v) in arr.iter() {
        let v = deref_shared(v);
        if preserve {
            cur.insert(k.to_owned(), v);
        } else {
            let _ = cur.push(v);
        }
        if cur.len() as i64 == size {
            let _ = out.push(Value::Array(std::mem::take(&mut cur)));
        }
    }
    if !cur.is_empty() {
        let _ = out.push(Value::Array(cur));
    }
    Ok(Value::Array(out))
}

// ---------------------------------------------------------------- diff / intersect

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cmp {
    /// Not compared.
    None,
    /// `(string)$a === (string)$b` (values) or key identity (keys).
    Builtin,
    /// A user callback.
    User,
}

struct SetOp {
    values: Cmp,
    keys: Cmp,
    intersect: bool,
}

fn callable_or_error(vm: &mut Vm, a: &Args, i: usize) -> R<Value> {
    let v = a.val(i);
    if vm.is_callable_value(&v, false) {
        vm.callable_deprecation(&v)?;
        return Ok(v);
    }
    let why = vm.callable_error(&v);
    vm.fthrow(
        a,
        "TypeError",
        &format!("Argument #{} must be a valid callback, {}", i + 1, why),
    )
}

/// A value comparison of the set functions: `(string)$a === (string)$b`
/// ordering, or a user callback.
fn set_cmp(
    vm: &mut Vm,
    a: &Args,
    cb: Option<&Value>,
    x: &Value,
    y: &Value,
    warned: &mut bool,
) -> R<Ordering> {
    match cb {
        Some(cb) => user_cmp(vm, a, cb, x.deref(), y.deref(), warned),
        None => flag_compare(vm, SORT_STRING, &x.deref(), &y.deref()),
    }
}

/// Sorts the positions of `list` by value (or by key) for the sort-merge
/// set functions.
fn sorted_positions(
    vm: &mut Vm,
    a: &Args,
    list: &[(Key, Value)],
    cb: Option<&Value>,
    by_key: bool,
    warned: &mut bool,
) -> R<Vec<usize>> {
    let mut idx: Vec<usize> = (0..list.len()).collect();
    sort_items(vm, &mut idx, |vm, &x, &y| {
        if by_key {
            let (kx, ky) = (list[x].0.to_value(), list[y].0.to_value());
            set_cmp(vm, a, cb, &kx, &ky, warned)
        } else {
            set_cmp(vm, a, cb, &list[x].1, &list[y].1, warned)
        }
    })?;
    Ok(idx)
}

/// The diff/intersect functions. Which elements PHP compares, and in
/// which order, is observable through callbacks and conversion warnings;
/// this follows the traces observed on PHP 8.5 (user comparators logging
/// their calls, and tests array_intersect_variation9.phpt, bug28739.phpt):
///
/// - value comparisons without keys (array_intersect, array_u*): every
///   array is sorted, then the sorted first array is walked against a
///   cursor in each other array;
/// - key callbacks (array_*_ukey, array_*_uassoc): every array is sorted
///   by key; intersections advance a cursor in each other array, diffs
///   scan the other arrays from their start;
/// - builtin keys with value comparison (*_assoc): direct key lookups;
/// - array_diff(): the string forms are compared directly.
fn set_op(vm: &mut Vm, a: &mut Args, op: SetOp) -> R<Value> {
    let ncb = (op.values == Cmp::User) as usize + (op.keys == Cmp::User) as usize;
    let n = a.len();
    if ncb > 0 && n < ncb + 1 {
        return Err(vm.throw_error_msg(
            "ArgumentCountError",
            &format!(
                "{}() expects at least {} arguments, {} given",
                a.fname(),
                ncb + 1,
                n
            ),
        ));
    }
    let narr = n - ncb;
    // Callbacks are checked first (observed on PHP 8.5: a bad callback is
    // reported before bad arrays).
    let mut cbs = Vec::new();
    for i in narr..n {
        cbs.push(callable_or_error(vm, a, i)?);
    }
    let arrays = arrays_from_range(vm, a, 0, narr)?;
    let (vcb, kcb) = match (op.values, op.keys) {
        (Cmp::User, Cmp::User) => (Some(cbs[0].clone()), Some(cbs[1].clone())),
        (Cmp::User, _) => (Some(cbs[0].clone()), None),
        (_, Cmp::User) => (None, Some(cbs[0].clone())),
        _ => (None, None),
    };
    let lists: Vec<Vec<(Key, Value)>> = arrays.iter().map(entries).collect();
    let n0 = lists[0].len();
    let mut keep = vec![!op.intersect; n0];
    let mut warned = false;
    let m = lists.len();

    match (op.values, op.keys) {
        // Keys only, builtin: lookups.
        (Cmp::None, Cmp::Builtin) => {
            for (j, (k, _)) in lists[0].iter().enumerate() {
                let mut in_all = true;
                let mut in_any = false;
                for arr in &arrays[1..] {
                    if arr.get(k.as_ref()).is_some() {
                        in_any = true;
                    } else {
                        in_all = false;
                    }
                }
                keep[j] = if op.intersect { in_all } else { !in_any };
            }
        }
        // array_diff(): string forms, each converted once.
        (Cmp::Builtin, Cmp::None) if !op.intersect => {
            let mut others = std::collections::HashSet::new();
            let mut first = Vec::with_capacity(n0);
            for (_, v) in &lists[0] {
                first.push(vm.to_php_str(&v.deref())?);
            }
            for list in &lists[1..] {
                for (_, v) in list {
                    others.insert(vm.to_php_str(&v.deref())?);
                }
            }
            for (j, s) in first.iter().enumerate() {
                keep[j] = !others.contains(s);
            }
        }
        // Values without keys: sort-merge.
        (Cmp::Builtin | Cmp::User, Cmp::None) => {
            let cb = vcb.as_ref();
            let mut sorted = Vec::with_capacity(m);
            for list in &lists {
                sorted.push(sorted_positions(vm, a, list, cb, false, &mut warned)?);
            }
            let val = |i: usize, p: usize| &lists[i][sorted[i][p]].1;
            let mut p = vec![0usize; m];
            let mut j = 0usize;
            if op.intersect {
                'top: while j < n0 {
                    let e = sorted[0][j];
                    let mut failed = None;
                    for i in 1..m {
                        let mut c;
                        loop {
                            if p[i] >= lists[i].len() {
                                // An exhausted array: nothing further is common.
                                break 'top;
                            }
                            c = set_cmp(vm, a, cb, &lists[0][e].1, val(i, p[i]), &mut warned)?;
                            if c == Ordering::Greater {
                                p[i] += 1;
                                continue;
                            }
                            break;
                        }
                        if c != Ordering::Equal {
                            failed = Some(i);
                            break;
                        }
                        p[i] += 1;
                    }
                    match failed {
                        None => {
                            keep[e] = true;
                            while j + 1 < n0
                                && set_cmp(
                                    vm,
                                    a,
                                    cb,
                                    &lists[0][sorted[0][j]].1,
                                    &lists[0][sorted[0][j + 1]].1,
                                    &mut warned,
                                )? == Ordering::Equal
                            {
                                j += 1;
                                keep[sorted[0][j]] = true;
                            }
                            j += 1;
                        }
                        Some(i) => {
                            // Drop elements while they stay below the
                            // other array's cursor.
                            loop {
                                j += 1;
                                if j >= n0 {
                                    break 'top;
                                }
                                let e2 = sorted[0][j];
                                let c =
                                    set_cmp(vm, a, cb, &lists[0][e2].1, val(i, p[i]), &mut warned)?;
                                if c != Ordering::Less {
                                    break;
                                }
                            }
                        }
                    }
                }
            } else {
                while j < n0 {
                    let e = sorted[0][j];
                    let mut found = false;
                    for i in 1..m {
                        let mut c = Ordering::Less;
                        while p[i] < lists[i].len() {
                            c = set_cmp(vm, a, cb, &lists[0][e].1, val(i, p[i]), &mut warned)?;
                            if c == Ordering::Greater {
                                p[i] += 1;
                                c = Ordering::Less;
                                continue;
                            }
                            break;
                        }
                        if c == Ordering::Equal {
                            found = true;
                            p[i] += 1;
                            break;
                        }
                    }
                    keep[e] = !found;
                    while j + 1 < n0
                        && set_cmp(
                            vm,
                            a,
                            cb,
                            &lists[0][sorted[0][j]].1,
                            &lists[0][sorted[0][j + 1]].1,
                            &mut warned,
                        )? == Ordering::Equal
                    {
                        j += 1;
                        keep[sorted[0][j]] = !found;
                    }
                    j += 1;
                }
            }
        }
        // Builtin keys with value comparison: lookups.
        (_, Cmp::Builtin) => {
            let cb = vcb.as_ref();
            for (j, (k, v)) in lists[0].iter().enumerate() {
                let mut in_all = true;
                let mut in_any = false;
                for arr in &arrays[1..] {
                    let hit = match arr.get(k.as_ref()) {
                        Some(o) => {
                            let o = o.deref();
                            set_cmp(vm, a, cb, v, &o, &mut warned)? == Ordering::Equal
                        }
                        None => false,
                    };
                    if hit {
                        in_any = true;
                    } else {
                        in_all = false;
                    }
                    if (op.intersect && !in_all) || (!op.intersect && in_any) {
                        break;
                    }
                }
                keep[j] = if op.intersect { in_all } else { !in_any };
            }
        }
        // Key callbacks.
        (_, Cmp::User) => {
            let kc = kcb.as_ref();
            let mut sorted = Vec::with_capacity(m);
            for list in &lists {
                sorted.push(sorted_positions(vm, a, list, kc, true, &mut warned)?);
            }
            let values = op.values != Cmp::None;
            let vc = vcb.as_ref();
            let mut p = vec![0usize; m];
            for j in 0..n0 {
                let e = sorted[0][j];
                let ek = lists[0][e].0.to_value();
                if op.intersect {
                    let mut all = true;
                    for i in 1..m {
                        // Observed on PHP 8.5: with two callbacks, keys of
                        // the third and later arrays go through the value
                        // callback.
                        let key_cb = if i >= 2 && vc.is_some() { vc } else { kc };
                        let mut c = Ordering::Less;
                        let mut hit = false;
                        while p[i] < lists[i].len() {
                            let o = &lists[i][sorted[i][p[i]]];
                            c = set_cmp(vm, a, key_cb, &ek, &o.0.to_value(), &mut warned)?;
                            if c == Ordering::Greater {
                                p[i] += 1;
                                continue;
                            }
                            break;
                        }
                        if p[i] < lists[i].len() && c == Ordering::Equal {
                            let o = &lists[i][sorted[i][p[i]]];
                            hit = !values
                                || set_cmp(vm, a, vc, &lists[0][e].1, &o.1, &mut warned)?
                                    == Ordering::Equal;
                            if hit {
                                p[i] += 1;
                            }
                        }
                        if !hit {
                            all = false;
                            break;
                        }
                    }
                    keep[e] = all;
                } else {
                    let mut found = false;
                    'arrays: for i in 1..m {
                        for q in 0..lists[i].len() {
                            let o = &lists[i][sorted[i][q]];
                            let c = set_cmp(vm, a, kc, &ek, &o.0.to_value(), &mut warned)?;
                            if c == Ordering::Equal {
                                if !values
                                    || set_cmp(vm, a, vc, &lists[0][e].1, &o.1, &mut warned)?
                                        == Ordering::Equal
                                {
                                    found = true;
                                    break 'arrays;
                                }
                                break;
                            }
                        }
                    }
                    keep[e] = !found;
                }
            }
        }
        (Cmp::None, Cmp::None) => {}
    }
    let mut out = PhpArray::new();
    for (j, (k, v)) in lists[0].iter().enumerate() {
        if keep[j] {
            out.insert(k.clone(), v.clone());
        }
    }
    Ok(Value::Array(out))
}

fn arrays_from_range(vm: &mut Vm, a: &Args, start: usize, end: usize) -> R<Vec<PhpArray>> {
    let mut out = Vec::new();
    for i in start..end {
        match a.val(i) {
            Value::Array(arr) => out.push(arr),
            other => {
                return vm.fthrow(
                    a,
                    "TypeError",
                    &format!(
                        "Argument #{} must be of type array, {} given",
                        i + 1,
                        Vm::type_name_given(&other)
                    ),
                );
            }
        }
    }
    Ok(out)
}

macro_rules! set_ops {
    ($($name:ident => ($v:ident, $k:ident, $i:expr)),* $(,)?) => {
        $(
            fn $name(vm: &mut Vm, a: &mut Args) -> R<Value> {
                set_op(vm, a, SetOp { values: Cmp::$v, keys: Cmp::$k, intersect: $i })
            }
        )*
    };
}

set_ops! {
    array_diff => (Builtin, None, false),
    array_diff_key => (None, Builtin, false),
    array_diff_assoc => (Builtin, Builtin, false),
    array_udiff => (User, None, false),
    array_diff_ukey => (None, User, false),
    array_diff_uassoc => (Builtin, User, false),
    array_udiff_assoc => (User, Builtin, false),
    array_udiff_uassoc => (User, User, false),
    array_intersect => (Builtin, None, true),
    array_intersect_key => (None, Builtin, true),
    array_intersect_assoc => (Builtin, Builtin, true),
    array_uintersect => (User, None, true),
    array_intersect_ukey => (None, User, true),
    array_intersect_uassoc => (Builtin, User, true),
    array_uintersect_assoc => (User, Builtin, true),
    array_uintersect_uassoc => (User, User, true),
}

// ---------------------------------------------------------------- push / pop / shift

fn array_push(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (r, arr) = ref_array(vm, a, 0, "array")?;
    drop(arr);
    let mut arr = take_ref_array(&r);
    for i in 1..a.len() {
        let v = a.val(i);
        if arr.push(v).is_err() {
            r.set(Value::Array(arr));
            return vm.throw(
                "Error",
                "Cannot add element to the array as the next element is already occupied",
            );
        }
    }
    let n = arr.len();
    r.set(Value::Array(arr));
    Ok(Value::Int(n as i64))
}

fn array_pop(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (r, arr) = ref_array(vm, a, 0, "array")?;
    if arr.is_empty() {
        return Ok(Value::Null);
    }
    drop(arr);
    let mut arr = take_ref_array(&r);
    let d = arr.make_mut();
    let last = d.last_pos().and_then(|p| d.key_at(p)).map(|k| k.to_owned());
    let next = d.next_index();
    let v = match &last {
        Some(k) => d.remove(k.as_ref()),
        None => None,
    };
    // Tests bug67693.phpt, negative_index.phpt: popping the last int key
    // lowers the next index to it, other pops keep it.
    if let (Some(Key::Int(k)), Some(n)) = (&last, next)
        && k.checked_add(1) == Some(n)
    {
        d.set_next_index(*k);
    }
    d.reset();
    r.set(Value::Array(arr));
    Ok(v.map(|v| v.into_deref()).unwrap_or(Value::Null))
}

fn array_shift(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (r, arr) = ref_array(vm, a, 0, "array")?;
    if arr.is_empty() {
        return Ok(Value::Null);
    }
    drop(arr);
    let arr = take_ref_array(&r);
    let mut it = arr.iter();
    let first = it.next().map(|(_, v)| v.clone());
    let mut out = PhpArray::with_capacity(arr.len());
    for (k, v) in it {
        match k {
            KeyRef::Int(_) => {
                let _ = out.push(v.clone());
            }
            KeyRef::Str(s) => {
                out.insert(Key::Str(s.clone()), v.clone());
            }
        }
    }
    drop(arr);
    r.set(Value::Array(out));
    Ok(first.map(|v| v.into_deref()).unwrap_or(Value::Null))
}

fn array_unshift(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (r, arr) = ref_array(vm, a, 0, "array")?;
    drop(arr);
    let arr = take_ref_array(&r);
    let mut out = PhpArray::with_capacity(arr.len() + a.len() - 1);
    for i in 1..a.len() {
        let _ = out.push(a.val(i));
    }
    for (k, v) in arr.iter() {
        match k {
            KeyRef::Int(_) => {
                let _ = out.push(v.clone());
            }
            KeyRef::Str(s) => {
                out.insert(Key::Str(s.clone()), v.clone());
            }
        }
    }
    drop(arr);
    let n = out.len();
    r.set(Value::Array(out));
    Ok(Value::Int(n as i64))
}

fn array_reverse(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let arr = a.array(0);
    let preserve = a.bool_or(1, false);
    let items = entries(&arr);
    let mut out = PhpArray::with_capacity(items.len());
    for (k, v) in items.into_iter().rev() {
        let v = deref_shared(&v);
        match k {
            Key::Int(i) if preserve => {
                out.insert(Key::Int(i), v);
            }
            Key::Int(_) => {
                let _ = out.push(v);
            }
            Key::Str(s) => {
                out.insert(Key::Str(s), v);
            }
        }
    }
    Ok(Value::Array(out))
}

// ---------------------------------------------------------------- random

fn array_rand(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let arr = a.array(0);
    let num = a.int_or(1, 1);
    // Picks as Random\Randomizer::pickArrayKeys() does, from the global
    // Mt19937 whatever its mode (test ext/random/tests/01_functions/
    // array_rand_mt_rand_php.phpt).
    let picked = super::random::array_rand_positions(vm, &arr, num)?;
    let keys: Vec<Value> = arr.iter().map(|(k, _)| key_value(k)).collect();
    if num == 1 {
        return Ok(keys[picked[0]].clone());
    }
    Ok(Value::Array(list_of(
        picked.into_iter().map(|i| keys[i].clone()),
    )))
}

fn shuffle(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (r, arr) = ref_array(vm, a, 0, "array")?;
    let mut vals: Vec<Value> = arr.iter().map(|(_, v)| v.clone()).collect();
    drop(arr);
    super::random::legacy_shuffle(vm, &mut vals);
    r.set(Value::Array(list_of(vals)));
    Ok(Value::Bool(true))
}

// ---------------------------------------------------------------- internal pointer

fn pointer_target(vm: &mut Vm, a: &Args) -> R<Option<PhpRef>> {
    let r = match a.ref_at(0) {
        Some(r) => r,
        None => PhpRef::new(a.val(0)),
    };
    let is_obj = matches!(&*r.borrow(), Value::Object(_));
    if is_obj {
        vm.fdeprecated(
            a,
            &format!("Calling {}() on an object is deprecated", a.fname()),
        )?;
        return Ok(None);
    }
    Ok(Some(r))
}

/// Applies `f` to the property table of an object (the internal pointer
/// of objects with only dynamic properties persists there, as PHP keeps
/// it in the property table: test gh16905.phpt), returning `get`'s result.
fn with_obj_table<T>(vm: &mut Vm, a: &Args, f: impl FnOnce(&mut PhpArray) -> T) -> T {
    let Value::Object(o) = a.val(0) else {
        return f(&mut PhpArray::new());
    };
    let declared = !vm.class_of(&o).props.borrow().is_empty();
    if declared {
        let mut arr = vm.object_props_array(&o, true);
        return f(&mut arr);
    }
    let mut t = o.obj().props.borrow_mut();
    f(&mut t)
}

fn cur_value(arr: &PhpArray) -> Value {
    match arr.data().current() {
        Some(v) => v.deref(),
        None => Value::Bool(false),
    }
}

fn move_pointer(vm: &mut Vm, a: &mut Args, f: fn(&mut goro_runtime::ArrayData)) -> R<Value> {
    let Some(r) = pointer_target(vm, a)? else {
        return Ok(with_obj_table(vm, a, |arr| {
            if !arr.is_empty() {
                f(arr.make_mut());
            }
            cur_value(arr)
        }));
    };
    let mut b = r.borrow_mut();
    let Value::Array(arr) = &mut *b else {
        return Ok(Value::Bool(false));
    };
    if !arr.is_empty() {
        f(arr.make_mut());
    }
    Ok(cur_value(arr))
}

fn next(vm: &mut Vm, a: &mut Args) -> R<Value> {
    move_pointer(vm, a, |d| d.advance())
}
fn prev(vm: &mut Vm, a: &mut Args) -> R<Value> {
    move_pointer(vm, a, |d| d.retreat())
}
fn reset(vm: &mut Vm, a: &mut Args) -> R<Value> {
    move_pointer(vm, a, |d| d.reset())
}
fn end(vm: &mut Vm, a: &mut Args) -> R<Value> {
    move_pointer(vm, a, |d| d.end())
}

fn current(vm: &mut Vm, a: &mut Args) -> R<Value> {
    match a.val(0) {
        Value::Array(arr) => Ok(cur_value(&arr)),
        Value::Object(_) => {
            vm.fdeprecated(
                a,
                &format!("Calling {}() on an object is deprecated", a.fname()),
            )?;
            Ok(with_obj_table(vm, a, |arr| cur_value(arr)))
        }
        _ => Ok(Value::Bool(false)),
    }
}

fn key(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let k = |arr: &PhpArray| match arr.data().current_key() {
        Some(k) => key_value(k),
        None => Value::Null,
    };
    match a.val(0) {
        Value::Array(arr) => Ok(k(&arr)),
        Value::Object(_) => {
            vm.fdeprecated(a, "Calling key() on an object is deprecated")?;
            Ok(with_obj_table(vm, a, |arr| k(arr)))
        }
        _ => Ok(Value::Null),
    }
}
