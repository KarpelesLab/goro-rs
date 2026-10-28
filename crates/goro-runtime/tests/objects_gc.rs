//! Object handles, deterministic destruction and cycle collection.
//! Expected behaviour from the PHP manual (Constructors and Destructors,
//! Garbage Collection) and observations of PHP 8.5; each scenario quotes
//! the PHP script it mirrors.

use std::cell::RefCell;
use std::rc::Rc;

use goro_runtime::gc::collect_cycles;
use goro_runtime::object::{has_pending_destructors, live_object_count, pop_pending_destructor};
use goro_runtime::{ClassInfo, Key, ObjRef, PhpArray, PhpStr, Value};

struct Class {
    name: PhpStr,
    dtor: bool,
}

impl ClassInfo for Class {
    fn name(&self) -> &PhpStr {
        &self.name
    }
    fn has_destructor(&self) -> bool {
        self.dtor
    }
}

fn std_class() -> Rc<dyn ClassInfo> {
    thread_local! {
        static C: Rc<dyn ClassInfo> = Rc::new(Class { name: PhpStr::from("stdClass"), dtor: false });
    }
    C.with(|c| c.clone())
}

/// `class D { function __destruct() { echo "[dtor {$this->n}]"; } }`; slot 0
/// holds `n`, slot 1 a free property `s`.
fn d_class() -> Rc<dyn ClassInfo> {
    thread_local! {
        static C: Rc<dyn ClassInfo> = Rc::new(Class { name: PhpStr::from("D"), dtor: true });
    }
    C.with(|c| c.clone())
}

fn new_std() -> ObjRef {
    ObjRef::new(std_class(), vec![Value::Null])
}

fn new_d(n: i64) -> ObjRef {
    ObjRef::new(d_class(), vec![Value::Int(n), Value::Null])
}

fn set_prop(o: &ObjRef, v: Value) {
    let old = std::mem::replace(&mut o.obj().slots.borrow_mut()[o.slot_for_test()], v);
    drop(old);
}

trait SlotForTest {
    fn slot_for_test(&self) -> usize;
}
impl SlotForTest for ObjRef {
    fn slot_for_test(&self) -> usize {
        if self.class_name() == &"D" { 1 } else { 0 }
    }
}

/// The engine's part: run pending destructors, logging `[dtor n]`.
fn run_destructors(log: &RefCell<Vec<String>>) {
    while let Some(o) = pop_pending_destructor() {
        destruct(&o, log);
    }
}

fn destruct(o: &ObjRef, log: &RefCell<Vec<String>>) {
    let n = o.obj().slots.borrow()[0].as_int().unwrap();
    log.borrow_mut().push(format!("dtor {n}"));
}

#[test]
fn destructor_runs_on_last_release() {
    let log = RefCell::new(Vec::new());
    // $a = new D(1); $b = $a; unset($a); -> nothing; unset($b); -> dtor
    let a = new_d(1);
    let b = a.clone();
    drop(a);
    assert!(!has_pending_destructors());
    drop(b);
    assert!(has_pending_destructors());
    run_destructors(&log);
    assert_eq!(*log.borrow(), ["dtor 1"]);
    assert!(!has_pending_destructors());
    assert_eq!(live_object_count(), 0);
}

#[test]
fn destructor_order() {
    let log = RefCell::new(Vec::new());
    // $a = [new D(1), new D(2), new D(3)]; $a = null;  -> 1, 2, 3
    let mut a = Value::Array(PhpArray::from_values(
        (1..=3).map(|n| Value::Object(new_d(n))),
    ));
    a.assign(Value::Null);
    run_destructors(&log);
    assert_eq!(*log.borrow(), ["dtor 1", "dtor 2", "dtor 3"]);

    // $a = new D(1); $a = new D(2); echo "[x]"; -> [dtor 1][x][dtor 2]
    log.borrow_mut().clear();
    let mut a = Value::Object(new_d(1));
    let old = a.assign(Value::Object(new_d(2)));
    drop(old);
    run_destructors(&log);
    log.borrow_mut().push("x".into());
    drop(a);
    run_destructors(&log);
    assert_eq!(*log.borrow(), ["dtor 1", "x", "dtor 2"]);
}

#[test]
fn destructor_resurrection() {
    let log = RefCell::new(Vec::new());
    let keep: RefCell<Option<ObjRef>> = RefCell::new(None);
    drop(new_d(7));
    // The destructor stores $this somewhere: the object survives, and its
    // destructor is not called again.
    while let Some(o) = pop_pending_destructor() {
        destruct(&o, &log);
        *keep.borrow_mut() = Some(o);
    }
    let o = keep.borrow_mut().take().unwrap();
    assert!(o.destructor_called());
    assert_eq!(live_object_count(), 1);
    drop(o);
    assert!(!has_pending_destructors());
    assert_eq!(live_object_count(), 0);
    assert_eq!(*log.borrow(), ["dtor 7"]);
}

#[test]
fn object_ids_are_reused_last_freed_first() {
    // $a, $b, $c = new stdClass (1, 2, 3); unset($a); unset($b);
    // $d, $e, $f = new stdClass  -> 2, 1, 4   (observed on PHP 8.5)
    let a = new_std();
    let b = new_std();
    let c = new_std();
    assert_eq!((a.handle(), b.handle(), c.handle()), (1, 2, 3));
    drop(a);
    drop(b);
    let d = new_std();
    let e = new_std();
    let f = new_std();
    assert_eq!((d.handle(), e.handle(), f.handle()), (2, 1, 4));
}

#[test]
fn gc_array_reference_cycle() {
    // $a = []; $a[] = &$a; unset($a); gc_collect_cycles() === 1
    let mut a = Value::Array(PhpArray::new());
    let r = a.make_ref();
    {
        let mut arr = r.borrow_mut();
        arr.as_array_mut()
            .unwrap()
            .push(Value::Ref(r.clone()))
            .unwrap();
    }
    drop(r);
    drop(a);
    assert_eq!(collect_cycles(&mut |_| panic!("no destructors")), 1);
    assert_eq!(collect_cycles(&mut |_| {}), 0);
}

#[test]
fn gc_object_cycles() {
    // $o = new stdClass; $o->s = $o; unset($o); -> 1
    let o = new_std();
    let weak = o.downgrade();
    set_prop(&o, Value::Object(o.clone()));
    drop(o);
    assert!(weak.upgrade().is_some(), "cycle keeps it alive");
    assert_eq!(collect_cycles(&mut |_| {}), 1);
    assert!(weak.upgrade().is_none());
    assert_eq!(live_object_count(), 0);

    // $a->b = $b; $b->a = $a; unset($a, $b); -> 2
    let a = new_std();
    let b = new_std();
    set_prop(&a, Value::Object(b.clone()));
    set_prop(&b, Value::Object(a.clone()));
    drop(a);
    drop(b);
    assert_eq!(collect_cycles(&mut |_| {}), 2);
    assert_eq!(live_object_count(), 0);
}

#[test]
fn gc_counts_runtime_arrays_but_not_literals() {
    // $o->arr = [$o, [1, 2]]  (literal inner array) -> 2
    let o = new_std();
    let inner = PhpArray::from_values([Value::Int(1), Value::Int(2)]);
    inner.data().mark_literal();
    let arr = PhpArray::from_values([Value::Object(o.clone()), Value::Array(inner)]);
    o.obj()
        .props
        .borrow_mut()
        .insert(Key::from("arr"), Value::Array(arr));
    drop(o);
    assert_eq!(collect_cycles(&mut |_| {}), 2);

    // $x = [1]; $x[] = 2; $o->arr = [$o, $x]; unset($o, $x); -> 3
    let o = new_std();
    let mut x = PhpArray::from_values([Value::Int(1)]);
    x.push(Value::Int(2)).unwrap();
    let arr = PhpArray::from_values([Value::Object(o.clone()), Value::Array(x)]);
    o.obj()
        .props
        .borrow_mut()
        .insert(Key::from("arr"), Value::Array(arr));
    drop(o);
    assert_eq!(collect_cycles(&mut |_| {}), 3);
}

#[test]
fn gc_two_arrays_through_references() {
    // $a = ['x' => null]; $b = ['y' => null]; $a['x'] = &$b; $b['y'] = &$a;
    // unset($a, $b); -> 2
    let mut a = Value::Array(PhpArray::new());
    let mut b = Value::Array(PhpArray::new());
    let ra = a.make_ref();
    let rb = b.make_ref();
    ra.borrow_mut()
        .as_array_mut()
        .unwrap()
        .insert(Key::from("x"), Value::Ref(rb.clone()));
    rb.borrow_mut()
        .as_array_mut()
        .unwrap()
        .insert(Key::from("y"), Value::Ref(ra.clone()));
    drop((a, b, ra, rb));
    assert_eq!(collect_cycles(&mut |_| {}), 2);
}

#[test]
fn gc_runs_destructors_first() {
    // $o = new D(1); $o->s = $o; unset($o); gc_collect_cycles()
    //   -> [dtor 1], returns 1
    let log = RefCell::new(Vec::new());
    let o = new_d(1);
    set_prop(&o, Value::Object(o.clone()));
    drop(o);
    let n = collect_cycles(&mut |o| destruct(&o, &log));
    assert_eq!(n, 1);
    assert_eq!(*log.borrow(), ["dtor 1"]);
    assert_eq!(live_object_count(), 0);

    // A pair: both destructors, 2 freed.
    log.borrow_mut().clear();
    let a = new_d(1);
    let b = new_d(2);
    set_prop(&a, Value::Object(b.clone()));
    set_prop(&b, Value::Object(a.clone()));
    drop((a, b));
    assert_eq!(collect_cycles(&mut |o| destruct(&o, &log)), 2);
    let mut l = log.borrow().clone();
    l.sort();
    assert_eq!(l, ["dtor 1", "dtor 2"]);
}

#[test]
fn gc_respects_resurrection_and_live_roots() {
    // A cycle still referenced from outside is not garbage.
    let o = new_std();
    set_prop(&o, Value::Object(o.clone()));
    let extra = o.clone();
    drop(extra);
    assert_eq!(collect_cycles(&mut |_| {}), 0);
    assert_eq!(live_object_count(), 1);

    // A destructor that stores $this keeps the cycle alive.
    let keep: RefCell<Vec<ObjRef>> = RefCell::new(Vec::new());
    let d = new_d(5);
    set_prop(&d, Value::Object(d.clone()));
    drop(d);
    assert_eq!(collect_cycles(&mut |o| keep.borrow_mut().push(o)), 0);
    assert_eq!(keep.borrow().len(), 1);
    let d = keep.borrow_mut().pop().unwrap();
    assert!(d.destructor_called());
    // Released again: collected without a second destructor call.
    drop(d);
    assert_eq!(
        collect_cycles(&mut |_| panic!("destructor called twice")),
        1
    );

    drop(o);
    assert_eq!(collect_cycles(&mut |_| {}), 1);
    assert_eq!(live_object_count(), 0);
}

#[test]
fn gc_cycle_reachable_only_through_shared_array() {
    // $o = new stdClass; $x = [$o]; $o->a = $x; unset($o);
    // collect (alive through $x); unset($x); collect -> freed.
    let o = new_std();
    let x = PhpArray::from_values([Value::Object(o.clone())]);
    set_prop(&o, Value::Array(x.clone()));
    drop(o);
    assert_eq!(collect_cycles(&mut |_| {}), 0);
    drop(x);
    assert_eq!(collect_cycles(&mut |_| {}), 2);
    assert_eq!(live_object_count(), 0);
}

#[test]
fn scalar_arrays_are_not_buffered() {
    let a = PhpArray::from_values((0..10).map(Value::Int));
    for _ in 0..100 {
        let b = a.clone();
        drop(b);
    }
    assert_eq!(goro_runtime::gc::root_count(), 0);
}
