//! `IntlIterator` and `IntlPartsIterator` (PHP manual): iterators over
//! the time zone enumerations and the boundaries and parts of break
//! iterators. The items are computed when the iterator is made.

use goro_runtime::{ObjRef, Value};

use super::{new_obj, with_obj};
use crate::class::NativeHooks;
use crate::func::Args;
use crate::registry::Module;
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "intl",
    classes: &[
        ("intliterator", ITER_CLASS),
        ("intlpartsiterator", PARTS_CLASS),
    ],
    methods: &[
        ("intliterator::current", it_current),
        ("intliterator::key", it_key),
        ("intliterator::next", it_next),
        ("intliterator::rewind", it_rewind),
        ("intliterator::valid", it_valid),
        ("intlpartsiterator::getbreakiterator", it_break_iterator),
        ("intlpartsiterator::getrulestatus", it_rule_status),
    ],
    hooks: &[("intliterator", it_hooks)],
    ..Module::EMPTY
};

const ITER_CLASS: &str = r#"<?php class IntlIterator implements Iterator {
    public function current(): mixed {}
    public function key(): mixed {}
    public function next(): void {}
    public function rewind(): void {}
    public function valid(): bool {}
}"#;

const PARTS_CLASS: &str = r#"<?php class IntlPartsIterator extends IntlIterator {
    public const int KEY_SEQUENTIAL = 0;
    public const int KEY_LEFT = 1;
    public const int KEY_RIGHT = 2;
    public function getBreakIterator(): IntlBreakIterator {}
    public function getRuleStatus(): int {}
}"#;

/// An iterator's items (key, value) and position; parts iterators keep
/// their break iterator and the rule status of each part.
#[derive(Clone)]
pub(crate) struct IterData {
    pub items: Vec<(Value, Value)>,
    pub pos: usize,
    pub source: Option<ObjRef>,
    pub statuses: Vec<i64>,
    /// A parts iterator driving its break iterator: the key type and the
    /// current part `(index, left, right, status)`.
    pub parts: Option<(i64, Option<(usize, usize, usize, i64)>)>,
}

/// A new IntlIterator over `items` (keys 0, 1, ...).
pub(crate) fn new_string_iterator(vm: &mut Vm, items: Vec<String>) -> R<ObjRef> {
    let items = items
        .into_iter()
        .enumerate()
        .map(|(i, s)| (Value::Int(i as i64), Value::from(s.as_str())))
        .collect();
    new_iterator(vm, "IntlIterator", items, None, Vec::new())
}

pub(crate) fn new_iterator(
    vm: &mut Vm,
    class: &str,
    items: Vec<(Value, Value)>,
    source: Option<ObjRef>,
    statuses: Vec<i64>,
) -> R<ObjRef> {
    new_obj(
        vm,
        class,
        IterData {
            items,
            pos: 0,
            source,
            statuses,
            parts: None,
        },
    )
}

/// A parts iterator over the break iterator `bi` (PHP's parts iterators
/// move their break iterator: ext/intl/tests/cpbi_parts_iterator.phpt).
pub(crate) fn new_parts_iterator(vm: &mut Vm, bi: ObjRef, key_type: i64) -> R<ObjRef> {
    let o = new_iterator(vm, "IntlPartsIterator", Vec::new(), Some(bi.clone()), Vec::new())?;
    with_obj::<IterData, ()>(&o, |x| x.v.parts = Some((key_type, None)));
    Ok(o)
}

/// The parts mode: (key type, break iterator), when the iterator is one.
fn parts_of(o: &ObjRef) -> Option<(i64, ObjRef)> {
    with_obj::<IterData, Option<(i64, ObjRef)>>(o, |x| {
        x.v.parts.map(|(t, _)| (t, x.v.source.clone().unwrap()))
    })
    .flatten()
}

fn parts_set(o: &ObjRef, cur: Option<(usize, usize, usize, i64)>) {
    with_obj::<IterData, ()>(o, |x| {
        if let Some((_, c)) = &mut x.v.parts {
            *c = cur;
        }
    });
}

fn parts_cur(o: &ObjRef) -> Option<(usize, usize, usize, i64)> {
    with_obj::<IterData, Option<(usize, usize, usize, i64)>>(o, |x| x.v.parts.and_then(|(_, c)| c)).flatten()
}

fn state<U>(vm: &mut Vm, a: &Args, f: impl FnOnce(&mut IterData) -> U) -> R<U> {
    let o = a.this_obj().clone();
    match with_obj::<IterData, U>(&o, |x| f(&mut x.v)) {
        Some(u) => Ok(u),
        None => vm.throw("Error", "The IntlIterator is not properly constructed"),
    }
}

fn it_current(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    if let Some((_, bi)) = parts_of(&o) {
        return Ok(match parts_cur(&o) {
            Some((_, l, r, _)) => super::breakiter::text_slice(&bi, l, r),
            None => Value::Null,
        });
    }
    state(vm, a, |s| s.items.get(s.pos).map_or(Value::Null, |(_, v)| v.clone()))
}

fn it_key(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    if let Some((t, _)) = parts_of(&o) {
        return Ok(match parts_cur(&o) {
            Some((i, l, r, _)) => Value::Int(match t {
                1 => l as i64,
                2 => r as i64,
                _ => i as i64,
            }),
            None => Value::Null,
        });
    }
    state(vm, a, |s| s.items.get(s.pos).map_or(Value::Null, |(k, _)| k.clone()))
}

fn parts_advance(o: &ObjRef, bi: &ObjRef, first: bool) {
    let idx = if first { 0 } else { parts_cur(o).map_or(0, |c| c.0 + 1) };
    let next = super::breakiter::advance(bi, first).map(|(l, r, st)| (idx, l, r, st));
    parts_set(o, next);
}

fn it_next(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    if let Some((_, bi)) = parts_of(&o) {
        parts_advance(&o, &bi, false);
        return Ok(Value::Null);
    }
    state(vm, a, |s| {
        if s.pos < s.items.len() {
            s.pos += 1;
        }
    })?;
    Ok(Value::Null)
}

fn it_rewind(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    if let Some((_, bi)) = parts_of(&o) {
        parts_advance(&o, &bi, true);
        return Ok(Value::Null);
    }
    state(vm, a, |s| s.pos = 0)?;
    Ok(Value::Null)
}

fn it_valid(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    if parts_of(&o).is_some() {
        return Ok(Value::Bool(parts_cur(&o).is_some()));
    }
    state(vm, a, |s| Value::Bool(s.pos < s.items.len()))
}

fn it_break_iterator(vm: &mut Vm, a: &mut Args) -> R<Value> {
    state(vm, a, |s| s.source.clone().map_or(Value::Null, Value::Object))
}

fn it_rule_status(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    if parts_of(&o).is_some() {
        return Ok(Value::Int(parts_cur(&o).map_or(0, |c| c.3)));
    }
    state(vm, a, |s| {
        let i = s.pos.min(s.statuses.len().saturating_sub(1));
        Value::Int(s.statuses.get(i).copied().unwrap_or(0))
    })
}

fn it_hooks() -> NativeHooks {
    NativeHooks {
        clone: Some(|vm, _from, _to| {
            super::timezone::clone_error(
                vm,
                &format!(
                    "Trying to clone an uncloneable object of class {}",
                    vm.class_of(_from).name.to_string_lossy()
                ),
            );
        }),
        ..Default::default()
    }
}
