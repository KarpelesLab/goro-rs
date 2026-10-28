//! Array semantics: keys, order, next index, copy-on-write, references in
//! arrays, the internal pointer and foreach iteration. Expected values are
//! from the PHP manual (Arrays, References Explained, foreach) and
//! observations of PHP 8.5 (quoted scripts were run on the reference
//! binary).

use goro_runtime::array::canonical_int;
use goro_runtime::{ArrayData, Diagnostics, Key, KeyRef, PhpArray, PhpRef, PhpStr, Value};

fn int(v: &Value) -> i64 {
    match v.deref() {
        Value::Int(i) => i,
        v => panic!("not an int: {v:?}"),
    }
}

fn keys(a: &PhpArray) -> Vec<String> {
    a.iter()
        .map(|(k, _)| match k {
            KeyRef::Int(i) => i.to_string(),
            KeyRef::Str(s) => format!("{:?}", s.to_string_lossy()),
        })
        .collect()
}

fn vals(a: &PhpArray) -> Vec<i64> {
    a.iter().map(|(_, v)| int(v)).collect()
}

fn list(v: &[i64]) -> PhpArray {
    PhpArray::from_values(v.iter().map(|&i| Value::Int(i)))
}

fn s(x: &str) -> Key {
    Key::Str(PhpStr::from(x))
}

#[test]
fn key_normalisation() {
    // PHP manual (Arrays): "8" is stored as 8, "08" is not.
    assert_eq!(Key::from("8"), Key::Int(8));
    assert_eq!(Key::from("08"), s("08"));
    assert_eq!(Key::from("-5"), Key::Int(-5));
    // Observed on PHP 8.5: "-0", "+1", " 1" and out-of-range integers stay
    // strings; PHP_INT_MIN's digits become an int.
    assert_eq!(Key::from("-0"), s("-0"));
    assert_eq!(Key::from("+1"), s("+1"));
    assert_eq!(Key::from(" 1"), s(" 1"));
    assert_eq!(Key::from("1 "), s("1 "));
    assert_eq!(Key::from("9223372036854775807"), Key::Int(i64::MAX));
    assert_eq!(Key::from("9223372036854775808"), s("9223372036854775808"));
    assert_eq!(Key::from("-9223372036854775808"), Key::Int(i64::MIN));
    assert_eq!(Key::from("-9223372036854775809"), s("-9223372036854775809"));
    assert_eq!(canonical_int(b"0"), Some(0));
    assert_eq!(canonical_int(b""), None);
    assert_eq!(canonical_int(b"-"), None);
    assert_eq!(canonical_int(b"1e3"), None);
    assert_eq!(canonical_int(b"00"), None);

    let mut a = PhpArray::new();
    a.insert(Key::from("8"), Value::Int(1));
    assert_eq!(a.get_int(8).map(int), Some(1));
    assert_eq!(a.get_bytes(b"8").map(int), Some(1));
    assert!(a.get_bytes(b"08").is_none());
}

#[test]
fn key_casts_with_diagnostics() {
    use goro_runtime::convert::to_key;
    let mut ctx = Diagnostics::new();
    assert_eq!(
        to_key(&Value::Bool(true), "array", &mut ctx).unwrap(),
        Key::Int(1)
    );
    assert_eq!(
        to_key(&Value::Float(1.7), "array", &mut ctx).unwrap(),
        Key::Int(1)
    );
    let n = ctx.take();
    assert_eq!(n.len(), 1);
    assert_eq!(
        n[0].message,
        "Implicit conversion from float 1.7 to int loses precision"
    );
    assert_eq!(to_key(&Value::Null, "array", &mut ctx).unwrap(), s(""));
    assert_eq!(
        ctx.take()[0].message,
        "Using null as an array offset is deprecated, use an empty string instead"
    );
    let e = to_key(&Value::Array(PhpArray::new()), "array", &mut ctx).unwrap_err();
    assert_eq!(e.message, "Cannot access offset of type array on array");
}

#[test]
fn insertion_order_and_overwrite() {
    let mut a = PhpArray::new();
    a.insert(s("b"), Value::Int(1));
    a.insert(Key::Int(5), Value::Int(2));
    a.insert(s("a"), Value::Int(3));
    a.insert(s("b"), Value::Int(4)); // overwrite keeps the position
    assert_eq!(keys(&a), ["\"b\"", "5", "\"a\""]);
    assert_eq!(vals(&a), [4, 2, 3]);
    // Observed on PHP 8.5: unset then re-add moves the key to the end.
    a.remove(KeyRef::Str(&PhpStr::from("b")));
    a.insert(s("b"), Value::Int(9));
    assert_eq!(keys(&a), ["5", "\"a\"", "\"b\""]);
    assert_eq!(a.len(), 3);
}

#[test]
fn next_index_rules() {
    // Observed on PHP 8.5 (and RFC "Make unary negative array index
    // consistent", PHP 8.3): [-5 => 1] then [] uses -4.
    let mut a = PhpArray::new();
    a.insert(Key::Int(-5), Value::Int(1));
    a.push(Value::Int(2)).unwrap();
    a.insert(s("x"), Value::Int(3));
    a.push(Value::Int(4)).unwrap();
    assert_eq!(keys(&a), ["-5", "-4", "\"x\"", "-3"]);

    // [-3]=1, []=2, [-10]=3, []=4  ->  -3, -2, -10, -1
    let mut a = PhpArray::new();
    a.insert(Key::Int(-3), Value::Int(1));
    a.push(Value::Int(2)).unwrap();
    a.insert(Key::Int(-10), Value::Int(3));
    a.push(Value::Int(4)).unwrap();
    assert_eq!(keys(&a), ["-3", "-2", "-10", "-1"]);

    // unset does not lower the next index: [1,2,3]; unset($a[2]); $a[] = 4
    let mut a = list(&[1, 2, 3]);
    a.remove(KeyRef::Int(2));
    a.push(Value::Int(4)).unwrap();
    assert_eq!(keys(&a), ["0", "1", "3"]);

    // array_pop resets it: [0, 1, 2]
    let mut a = list(&[1, 2, 3]);
    assert_eq!(a.make_mut().pop().map(|v| int(&v)), Some(3));
    a.push(Value::Int(4)).unwrap();
    assert_eq!(keys(&a), ["0", "1", "2"]);

    // String keys only: next is 0.
    let mut a = PhpArray::new();
    a.insert(s("x"), Value::Int(1));
    a.push(Value::Int(2)).unwrap();
    assert_eq!(keys(&a), ["\"x\"", "0"]);

    // [5 => 1]; unset($a[5]); $a[] = 2  ->  6
    let mut a = PhpArray::new();
    a.insert(Key::Int(5), Value::Int(1));
    a.remove(KeyRef::Int(5));
    a.push(Value::Int(2)).unwrap();
    assert_eq!(keys(&a), ["6"]);

    // [PHP_INT_MAX => 1]; $a[] = 2 throws Error.
    let mut a = PhpArray::new();
    a.insert(Key::Int(i64::MAX), Value::Int(1));
    let e = a.push(Value::Int(2)).unwrap_err();
    assert_eq!(
        e.to_error().message,
        "Cannot add element to the array as the next element is already occupied"
    );
}

#[test]
fn packed_representation() {
    let mut a = PhpArray::new();
    for i in 0..100 {
        assert_eq!(a.push(Value::Int(i * 10)).unwrap(), i);
    }
    assert!(a.data().is_packed());
    assert_eq!(a.get_int(42).map(int), Some(420));
    assert!(a.get_int(100).is_none());
    assert!(a.get_int(-1).is_none());
    a.insert(Key::Int(3), Value::Int(-3));
    assert!(a.data().is_packed());
    // Removing the last element keeps it packed.
    a.remove(KeyRef::Int(99));
    assert!(a.data().is_packed());
    assert_eq!(a.len(), 99);
    // A hole converts it, keeping order and positions.
    a.remove(KeyRef::Int(50));
    assert!(!a.data().is_packed());
    assert_eq!(a.len(), 98);
    assert_eq!(a.get_int(51).map(int), Some(510));
    assert!(!a.data().is_list());
    let mut b = list(&[1, 2]);
    b.insert(s("k"), Value::Int(3));
    assert!(!b.data().is_packed());
    assert_eq!(keys(&b), ["0", "1", "\"k\""]);
    // Inserting at a gap converts too.
    let mut c = list(&[1]);
    c.insert(Key::Int(5), Value::Int(2));
    assert_eq!(keys(&c), ["0", "5"]);
    c.push(Value::Int(3)).unwrap();
    assert_eq!(keys(&c), ["0", "5", "6"]);
    // array_is_list
    assert!(list(&[1, 2, 3]).data().is_list());
    let mut d = PhpArray::new();
    d.insert(Key::Int(1), Value::Int(1));
    d.insert(Key::Int(0), Value::Int(1));
    assert!(!d.data().is_list());
    let mut e = PhpArray::new();
    e.insert(Key::Int(3), Value::Int(1));
    e.push(Value::Int(2)).unwrap();
    assert!(!e.data().is_list());
}

#[test]
fn copy_on_write() {
    let a = list(&[1, 2, 3]);
    let mut b = a.clone();
    assert!(PhpArray::ptr_eq(&a, &b));
    assert_eq!(a.refcount(), 2);
    b.insert(Key::Int(0), Value::Int(100));
    assert!(!PhpArray::ptr_eq(&a, &b));
    assert_eq!(vals(&a), [1, 2, 3]);
    assert_eq!(vals(&b), [100, 2, 3]);
    assert_eq!(a.refcount(), 1);
    // Reads and misses don't separate.
    let mut c = a.clone();
    assert!(c.get_mut(KeyRef::Int(10)).is_none());
    assert!(c.remove(KeyRef::Int(10)).is_none());
    assert!(PhpArray::ptr_eq(&a, &c));
    // The empty array doesn't allocate.
    let e = PhpArray::new();
    assert_eq!(e.refcount(), 0);
    assert_eq!(std::mem::size_of::<PhpArray>(), 8);
}

#[test]
fn references_inside_arrays() {
    // PHP manual (References Explained): references inside arrays are kept
    // when the array is copied. $x = 1; $a = [&$x]; $b = $a; $b[0] = 2;
    // gives $x == 2 and $a[0] == 2.
    let mut x = Value::Int(1);
    let r = x.make_ref();
    let mut a = PhpArray::new();
    a.push(Value::Ref(r.clone())).unwrap();
    let mut b = a.clone();
    b.assign(Key::Int(0), Value::Int(2));
    assert_eq!(int(&x), 2);
    assert_eq!(int(a.get_int(0).unwrap()), 2);

    // A reference held only by the array is dereferenced when a copy is
    // separated (observed on PHP 8.5): $a = [1]; $r = &$a[0]; unset($r);
    // $b = $a; $b[0] = 2;  ->  $a == [1].
    let mut a = list(&[1]);
    let r = a.entry(Key::Int(0)).make_ref();
    drop(r);
    let mut b = a.clone();
    b.assign(Key::Int(0), Value::Int(2));
    assert_eq!(int(a.get_int(0).unwrap()), 1);
    assert_eq!(int(b.get_int(0).unwrap()), 2);

    // With the reference still alive it is shared: [0] => &int(2).
    let mut a = list(&[1]);
    let r = a.entry(Key::Int(0)).make_ref();
    let mut b = a.clone();
    b.assign(Key::Int(0), Value::Int(2));
    assert_eq!(int(a.get_int(0).unwrap()), 2);
    assert_eq!(int(&r.get()), 2);
    assert!(a.get_int(0).unwrap().is_ref());
}

#[test]
fn internal_pointer() {
    // $a = [1]; next($a); $a[] = 2; current($a) === 2
    let mut a = list(&[1]);
    a.make_mut().advance();
    a.push(Value::Int(2)).unwrap();
    assert_eq!(a.data().current().map(int), Some(2));

    // $a = [1,2,3]; next($a); unset($a[1]); [current, key] === [3, 2]
    let mut a = list(&[1, 2, 3]);
    a.make_mut().advance();
    a.remove(KeyRef::Int(1));
    assert_eq!(a.data().current().map(int), Some(3));
    assert_eq!(a.data().current_key(), Some(KeyRef::Int(2)));

    // end($a); unset($a[2]); current() === false
    let mut a = list(&[1, 2, 3]);
    a.make_mut().end();
    a.remove(KeyRef::Int(2));
    assert!(a.data().current().is_none());

    // prev() at the start makes the pointer invalid
    let mut a = list(&[1, 2]);
    a.make_mut().retreat();
    assert!(a.data().current().is_none());
    a.make_mut().reset();
    assert_eq!(a.data().current().map(int), Some(1));

    // The pointer is part of the value: $b = $a copies it.
    let mut a = list(&[1, 2, 3]);
    a.make_mut().advance();
    let b = a.clone();
    assert_eq!(b.data().current().map(int), Some(2));

    // end(); next(); $a[] = 3; current() === 3
    let mut a = list(&[1, 2]);
    a.make_mut().end();
    a.make_mut().advance();
    a.push(Value::Int(3)).unwrap();
    assert_eq!(a.data().current().map(int), Some(3));

    // prev() from the second element of a hash with holes
    let mut a = PhpArray::new();
    for k in ["a", "b", "c", "d"] {
        a.insert(s(k), Value::Int(k.as_bytes()[0] as i64));
    }
    a.remove(KeyRef::Str(&PhpStr::from("b")));
    let d = a.make_mut();
    d.advance(); // -> c
    assert_eq!(d.current().map(int), Some(b'c' as i64));
    d.retreat(); // -> a
    assert_eq!(d.current().map(int), Some(b'a' as i64));
}

/// `foreach ($a as $v)` over a snapshot: the engine iterates a clone.
#[test]
fn foreach_by_value_iterates_a_snapshot() {
    // $a = [1,2]; foreach ($a as $k => $v) { $o[] = $v; if ($k == 0) $a[] = 3; }
    // gives $o == [1,2], $a == [1,2,3].
    let mut a = list(&[1, 2]);
    let snapshot = a.clone();
    let mut seen = Vec::new();
    let mut p = 0;
    while let Some((pos, k, v)) = snapshot.data().next_from(p) {
        p = pos + 1;
        seen.push(int(v));
        if k == KeyRef::Int(0) {
            a.push(Value::Int(3)).unwrap();
        }
    }
    assert_eq!(seen, [1, 2]);
    assert_eq!(vals(&a), [1, 2, 3]);
}

/// Drives a by-reference foreach over the array in `var`, calling `body`
/// with the variable, the key and the reference.
fn foreach_ref(var: &mut PhpArray, mut body: impl FnMut(&mut PhpArray, &Key, &PhpRef)) -> Vec<i64> {
    let id = PhpArray::new_iter_id();
    let mut seen = Vec::new();
    while let Some((k, r)) = var.make_mut().iter_next_ref(id) {
        seen.push(int(&r.get()));
        body(var, &k, &r);
    }
    var.make_mut().iter_release(id);
    seen
}

#[test]
fn foreach_by_reference() {
    // Appending during the loop: [1, 2, 3] visited.
    let mut a = list(&[1, 2]);
    let seen = foreach_ref(&mut a, |a, k, _| {
        if *k == Key::Int(0) {
            a.push(Value::Int(3)).unwrap();
        }
    });
    assert_eq!(seen, [1, 2, 3]);

    // Unsetting the next element: [1, 3].
    let mut a = list(&[1, 2, 3]);
    let seen = foreach_ref(&mut a, |a, k, _| {
        if *k == Key::Int(0) {
            a.remove(KeyRef::Int(1));
        }
    });
    assert_eq!(seen, [1, 3]);

    // Writing through the reference: [2, 4, 6].
    let mut a = list(&[1, 2, 3]);
    foreach_ref(&mut a, |_, _, r| {
        let v = int(&r.get());
        r.set(Value::Int(v * 2));
    });
    assert_eq!(vals(&a), [2, 4, 6]);

    // Replacing the array: iteration continues over the new one from its
    // start: [1, 7, 8, 9, 10].
    let mut a = list(&[1, 2, 3]);
    let mut done = false;
    let seen = foreach_ref(&mut a, |a, _, _| {
        if !done {
            done = true;
            *a = list(&[7, 8, 9, 10]);
        }
    });
    assert_eq!(seen, [1, 7, 8, 9, 10]);

    // Copying then appending (separation in the middle): [1, 2, 3, 4].
    let mut a = list(&[1, 2, 3]);
    let mut copy = None;
    let seen = foreach_ref(&mut a, |a, k, _| {
        if *k == Key::Int(0) {
            copy = Some(a.clone());
            a.push(Value::Int(4)).unwrap();
        }
    });
    assert_eq!(seen, [1, 2, 3, 4]);
    assert_eq!(vals(copy.as_ref().unwrap()), [1, 2, 3]);
}

#[test]
fn iteration_survives_compaction() {
    // Many deletions and insertions while a by-reference iterator and the
    // internal pointer are active: every element present when reached is
    // visited exactly once, in order.
    let mut a = PhpArray::new();
    for i in 0..64 {
        a.insert(Key::Int(i * 2), Value::Int(i));
    }
    a.make_mut().advance(); // internal pointer at key 2
    let id = PhpArray::new_iter_id();
    let mut seen = Vec::new();
    let mut extra = 1000;
    while let Some((k, r)) = a.make_mut().iter_next_ref(id) {
        let v = int(&r.get());
        seen.push(v);
        let Key::Int(ki) = k else { panic!() };
        // Delete the element after next, and add two at the end, forcing
        // tombstones and several rebuilds.
        a.remove(KeyRef::Int(ki + 4));
        if extra < 1100 {
            a.insert(Key::Int(extra * 2), Value::Int(extra));
            a.insert(Key::Int(extra * 2 + 1), Value::Int(-extra));
            extra += 1;
        }
    }
    a.make_mut().iter_release(id);
    // Order equals the final array's order (no element visited twice or
    // skipped), and the internal pointer still points at key 2.
    let order: Vec<i64> = a.iter().map(|(_, v)| int(v)).collect();
    assert_eq!(seen, order);
    assert_eq!(a.data().current_key(), Some(KeyRef::Int(2)));
}

/// Randomised comparison against a naive ordered map.
#[test]
fn randomized_against_model() {
    let mut seed: u64 = 0x1234_5678_9abc_def0;
    let mut rnd = move |n: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % n
    };
    for round in 0..20 {
        let mut a = PhpArray::new();
        let mut model: Vec<(Key, i64)> = Vec::new();
        let mut next: Option<i64> = None;
        let mut snapshots: Vec<(PhpArray, Vec<(Key, i64)>)> = Vec::new();
        for step in 0..2000 {
            let key = match rnd(4) {
                0 => Key::Int(rnd(64) as i64 - 8),
                1 => Key::Str(PhpStr::from(format!("k{}", rnd(64)))),
                2 => Key::Int(rnd(10_000) as i64),
                _ => Key::Int(model.len() as i64),
            };
            match rnd(10) {
                0..=4 => {
                    let v = (round * 100_000 + step) as i64;
                    if let Key::Int(i) = key {
                        next = Some(next.map_or(i + 1, |n: i64| n.max(i + 1)));
                    }
                    match model.iter_mut().find(|(k, _)| *k == key) {
                        Some(e) => e.1 = v,
                        None => model.push((key.clone(), v)),
                    }
                    a.insert(key, Value::Int(v));
                }
                5..=7 => {
                    model.retain(|(k, _)| *k != key);
                    a.remove(key.as_ref());
                }
                8 => {
                    let v = -(step as i64);
                    let k = next.unwrap_or(0);
                    next = Some(k + 1);
                    model.push((Key::Int(k), v));
                    assert_eq!(a.push(Value::Int(v)).unwrap(), k);
                }
                _ => snapshots.push((a.clone(), model.clone())),
            }
            if step % 97 == 0 {
                let got: Vec<(Key, i64)> = a.iter().map(|(k, v)| (k.to_owned(), int(v))).collect();
                assert_eq!(got, model, "round {round} step {step}");
                for (k, v) in &model {
                    assert_eq!(a.get(k.as_ref()).map(int), Some(*v));
                }
            }
        }
        // Snapshots were not affected by later writes (copy on write).
        for (snap, m) in snapshots {
            let got: Vec<(Key, i64)> = snap.iter().map(|(k, v)| (k.to_owned(), int(v))).collect();
            assert_eq!(got, m);
        }
    }
}

#[test]
fn entry_and_nested_writes() {
    // $a['x'][] = 1; $a['x'][] = 2;
    let mut a = PhpArray::new();
    for i in 1..=2 {
        let slot = a.entry(Key::from("x"));
        if slot.is_null() {
            *slot = Value::Array(PhpArray::new());
        }
        slot.as_array_mut().unwrap().push(Value::Int(i)).unwrap();
    }
    let inner = a.get_bytes(b"x").unwrap().as_array().unwrap();
    assert_eq!(vals(inner), [1, 2]);
    // $a[] = ...; on a packed array via push_entry
    let mut b = list(&[1]);
    *b.make_mut().push_entry().unwrap() = Value::Int(5);
    assert_eq!(vals(&b), [1, 5]);
}

#[test]
fn array_data_api() {
    let mut d = ArrayData::with_capacity(4);
    d.push(Value::Int(1)).unwrap();
    d.insert(Key::from("a"), Value::Int(2));
    assert_eq!(d.len(), 2);
    assert_eq!(d.first_pos(), Some(0));
    assert_eq!(d.last_pos(), Some(1));
    assert_eq!(d.key_at(1), Some(KeyRef::Str(&PhpStr::from("a"))));
    d.clear();
    assert!(d.is_empty());
    assert_eq!(d.next_index(), Some(0));
    let a = PhpArray::from_data(d);
    assert!(a.is_empty());
    let p = PhpArray::from_pairs([
        (Key::from("x"), Value::Int(1)),
        (Key::from("x"), Value::Int(2)),
    ]);
    assert_eq!(p.len(), 1);
    assert_eq!(int(p.get_bytes(b"x").unwrap()), 2);
}

/// Power-of-two strides and sequential runs must not degrade lookups (the
/// index folds high key bits into the slot position).
#[test]
fn strided_int_keys() {
    for shift in [0u32, 4, 10, 16, 20, 32, 40] {
        let mut a = PhpArray::new();
        a.insert(Key::from("first"), Value::Null);
        let n = 50_000i64;
        for i in 0..n {
            a.insert(Key::Int(i << shift), Value::Int(i));
        }
        for i in 0..n {
            assert_eq!(a.get_int(i << shift).map(int), Some(i), "shift {shift}");
        }
        assert!(a.get_int((n + 1) << shift).is_none());
        assert_eq!(a.len(), n as usize + 1);
    }
}

/// Converting a list to a hash keeps every element reachable.
#[test]
fn packed_to_hash_lookups() {
    for n in [1i64, 7, 8, 9, 100, 1000] {
        let mut a = list(&(0..n).collect::<Vec<_>>());
        a.insert(Key::from("s"), Value::Int(-1));
        for i in 0..n {
            assert_eq!(a.get_int(i).map(int), Some(i));
        }
        assert_eq!(a.get_bytes(b"s").map(int), Some(-1));
    }
}
