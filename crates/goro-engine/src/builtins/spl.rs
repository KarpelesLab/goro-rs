//! SPL (PHP manual: Standard PHP Library): exceptions, interfaces, data
//! structures (ArrayObject/ArrayIterator, SplFixedArray, doubly linked
//! lists, heaps, SplObjectStorage, MultipleIterator) and the iterators
//! (written in PHP, spl_iterators.php). Debug and serialization layouts
//! follow php85's output.

use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use goro_runtime::{Key, KeyRef, ObjRef, PhpArray, PhpRef, PhpStr, Value};

use crate::class::{Class, DisplayName, NativeHooks, TypeName, lower};
use crate::func::Args;
use crate::registry::Module;
use crate::vm::calls::Callee;
use crate::vm::{R, Vm};

const DECLS: &str = r#"<?php
class LogicException extends Exception {}
class BadFunctionCallException extends LogicException {}
class BadMethodCallException extends BadFunctionCallException {}
class DomainException extends LogicException {}
class InvalidArgumentException extends LogicException {}
class LengthException extends LogicException {}
class OutOfRangeException extends LogicException {}
class RuntimeException extends Exception {}
class OutOfBoundsException extends RuntimeException {}
class OverflowException extends RuntimeException {}
class RangeException extends RuntimeException {}
class UnderflowException extends RuntimeException {}
class UnexpectedValueException extends RuntimeException {}
interface OuterIterator extends Iterator { public function getInnerIterator(): ?Iterator; }
interface RecursiveIterator extends Iterator { public function hasChildren(): bool; public function getChildren(): ?RecursiveIterator; }
interface SeekableIterator extends Iterator { public function seek(int $offset): void; }
interface SplObserver { public function update(SplSubject $subject): void; }
interface SplSubject { public function attach(SplObserver $observer): void; public function detach(SplObserver $observer): void; public function notify(): void; }
class ArrayObject implements IteratorAggregate, ArrayAccess, Serializable, Countable {
    const STD_PROP_LIST = 1;
    const ARRAY_AS_PROPS = 2;
    public function __construct(array|object $array = [], int $flags = 0, string $iteratorClass = ArrayIterator::class) {}
    public function offsetExists(mixed $key): bool {}
    public function offsetGet(mixed $key): mixed {}
    public function offsetSet(mixed $key, mixed $value): void {}
    public function offsetUnset(mixed $key): void {}
    public function append(mixed $value): void {}
    public function getArrayCopy(): array {}
    public function count(): int {}
    public function getFlags(): int {}
    public function setFlags(int $flags): void {}
    public function asort(int $flags = SORT_REGULAR): true {}
    public function ksort(int $flags = SORT_REGULAR): true {}
    public function uasort(callable $callback): true {}
    public function uksort(callable $callback): true {}
    public function natsort(): true {}
    public function natcasesort(): true {}
    public function unserialize(string $data): void {}
    public function serialize(): string {}
    public function __serialize(): array {}
    public function __unserialize(array $data): void {}
    public function getIterator(): Iterator {}
    public function exchangeArray(array|object $array): array {}
    public function setIteratorClass(string $iteratorClass): void {}
    public function getIteratorClass(): string {}
    public function __debugInfo(): array {}
    public function __get($name) {}
    public function __set($name, $value) {}
    public function __isset($name) {}
    public function __unset($name) {}
}
class ArrayIterator implements SeekableIterator, ArrayAccess, Serializable, Countable {
    const STD_PROP_LIST = 1;
    const ARRAY_AS_PROPS = 2;
    public function __construct(array|object $array = [], int $flags = 0) {}
    public function offsetExists(mixed $key): bool {}
    public function offsetGet(mixed $key): mixed {}
    public function offsetSet(mixed $key, mixed $value): void {}
    public function offsetUnset(mixed $key): void {}
    public function append(mixed $value): void {}
    public function getArrayCopy(): array {}
    public function count(): int {}
    public function getFlags(): int {}
    public function setFlags(int $flags): void {}
    public function asort(int $flags = SORT_REGULAR): true {}
    public function ksort(int $flags = SORT_REGULAR): true {}
    public function uasort(callable $callback): true {}
    public function uksort(callable $callback): true {}
    public function natsort(): true {}
    public function natcasesort(): true {}
    public function unserialize(string $data): void {}
    public function serialize(): string {}
    public function __serialize(): array {}
    public function __unserialize(array $data): void {}
    public function rewind(): void {}
    public function current(): mixed {}
    public function key(): string|int|null {}
    public function next(): void {}
    public function valid(): bool {}
    public function seek(int $offset): void {}
    public function __debugInfo(): array {}
    public function __get($name) {}
    public function __set($name, $value) {}
    public function __isset($name) {}
    public function __unset($name) {}
}
class RecursiveArrayIterator extends ArrayIterator implements RecursiveIterator {
    const CHILD_ARRAYS_ONLY = 4;
    public function hasChildren(): bool {}
    public function getChildren(): ?RecursiveArrayIterator {}
}
class SplFixedArray implements IteratorAggregate, ArrayAccess, Countable, JsonSerializable {
    public function __construct(int $size = 0) {}
    public function __wakeup(): void {}
    public function __serialize(): array {}
    public function __unserialize(array $data): void {}
    public function count(): int {}
    public function toArray(): array {}
    public static function fromArray(array $array, bool $preserveKeys = true): SplFixedArray {}
    public function getSize(): int {}
    public function setSize(int $size): true {}
    public function offsetExists($index): bool {}
    public function offsetGet($index): mixed {}
    public function offsetSet($index, mixed $value): void {}
    public function offsetUnset($index): void {}
    public function getIterator(): Iterator {}
    public function jsonSerialize(): array {}
}
final class InternalIterator implements Iterator {
    private function __construct() {}
    public function current(): mixed {}
    public function key(): mixed {}
    public function next(): void {}
    public function valid(): bool {}
    public function rewind(): void {}
}
class SplDoublyLinkedList implements Iterator, Countable, ArrayAccess, Serializable {
    const IT_MODE_LIFO = 2;
    const IT_MODE_FIFO = 0;
    const IT_MODE_DELETE = 1;
    const IT_MODE_KEEP = 0;
    public function add(int $index, mixed $value): void {}
    public function pop(): mixed {}
    public function shift(): mixed {}
    public function push(mixed $value): void {}
    public function unshift(mixed $value): void {}
    public function top(): mixed {}
    public function bottom(): mixed {}
    public function __debugInfo(): array {}
    public function count(): int {}
    public function isEmpty(): bool {}
    public function setIteratorMode(int $mode): int {}
    public function getIteratorMode(): int {}
    public function offsetExists($index): bool {}
    public function offsetGet($index): mixed {}
    public function offsetSet($index, mixed $value): void {}
    public function offsetUnset($index): void {}
    public function rewind(): void {}
    public function current(): mixed {}
    public function key(): int {}
    public function prev(): void {}
    public function next(): void {}
    public function valid(): bool {}
    public function unserialize(string $data): void {}
    public function serialize(): string {}
    public function __serialize(): array {}
    public function __unserialize(array $data): void {}
}
class SplQueue extends SplDoublyLinkedList {
    public function enqueue(mixed $value): void {}
    public function dequeue(): mixed {}
}
class SplStack extends SplDoublyLinkedList {}
abstract class SplHeap implements Iterator, Countable {
    public function extract(): mixed {}
    public function insert(mixed $value): true {}
    public function top(): mixed {}
    public function count(): int {}
    public function isEmpty(): bool {}
    public function rewind(): void {}
    public function current(): mixed {}
    public function key(): int {}
    public function next(): void {}
    public function valid(): bool {}
    public function recoverFromCorruption(): true {}
    abstract protected function compare(mixed $value1, mixed $value2): int;
    public function isCorrupted(): bool {}
    public function __debugInfo(): array {}
    public function __serialize(): array {}
    public function __unserialize(array $data): void {}
}
class SplMinHeap extends SplHeap { protected function compare(mixed $value1, mixed $value2): int {} }
class SplMaxHeap extends SplHeap { protected function compare(mixed $value1, mixed $value2): int {} }
class SplPriorityQueue implements Iterator, Countable {
    const EXTR_BOTH = 3;
    const EXTR_PRIORITY = 2;
    const EXTR_DATA = 1;
    public function compare(mixed $priority1, mixed $priority2): int {}
    public function insert(mixed $value, mixed $priority) {}
    public function setExtractFlags(int $flags): int {}
    public function top(): mixed {}
    public function extract(): mixed {}
    public function count(): int {}
    public function isEmpty(): bool {}
    public function rewind(): void {}
    public function current(): mixed {}
    public function key(): int {}
    public function next(): void {}
    public function valid(): bool {}
    public function recoverFromCorruption(): true {}
    public function isCorrupted(): bool {}
    public function getExtractFlags(): int {}
    public function __debugInfo(): array {}
    public function __serialize(): array {}
    public function __unserialize(array $data): void {}
}
class SplObjectStorage implements Countable, SeekableIterator, Serializable, ArrayAccess {
    public function attach(object $object, mixed $info = null): void {}
    public function detach(object $object): void {}
    public function contains(object $object): bool {}
    public function addAll(SplObjectStorage $storage): int {}
    public function removeAll(SplObjectStorage $storage): int {}
    public function removeAllExcept(SplObjectStorage $storage): int {}
    public function getInfo(): mixed {}
    public function setInfo(mixed $info): void {}
    public function count(int $mode = COUNT_NORMAL): int {}
    public function rewind(): void {}
    public function valid(): bool {}
    public function key(): int {}
    public function current(): object {}
    public function next(): void {}
    public function seek(int $offset): void {}
    public function unserialize(string $data): void {}
    public function serialize(): string {}
    public function offsetExists($object): bool {}
    public function offsetGet($object): mixed {}
    public function offsetSet($object, mixed $info = null): void {}
    public function offsetUnset($object): void {}
    public function getHash(object $object): string {}
    public function __serialize(): array {}
    public function __unserialize(array $data): void {}
    public function __debugInfo(): array {}
}
class MultipleIterator implements Iterator {
    const MIT_NEED_ANY = 0;
    const MIT_NEED_ALL = 1;
    const MIT_KEYS_NUMERIC = 0;
    const MIT_KEYS_ASSOC = 2;
    public function __construct(int $flags = MultipleIterator::MIT_NEED_ALL | MultipleIterator::MIT_KEYS_NUMERIC) {}
    public function getFlags(): int {}
    public function setFlags(int $flags): void {}
    public function attachIterator(Iterator $iterator, string|int|null $info = null): void {}
    public function detachIterator(Iterator $iterator): void {}
    public function containsIterator(Iterator $iterator): bool {}
    public function countIterators(): int {}
    public function rewind(): void {}
    public function valid(): bool {}
    public function key(): array {}
    public function current(): array {}
    public function next(): void {}
    public function __debugInfo(): array {}
}
"#;

const ITERATORS: &str = include_str!("spl_iterators.php");
const FILES: &str = include_str!("spl_files.php");
const REGEX: &str = include_str!("spl_regex.php");

macro_rules! c {
    ($l:literal) => {
        ($l, DECLS)
    };
}
macro_rules! fl {
    ($l:literal) => {
        ($l, FILES)
    };
}
macro_rules! it {
    ($l:literal) => {
        ($l, ITERATORS)
    };
}

pub static MODULE: Module = Module {
    name: "SPL",
    functions: super::spl_autoload::FUNCTIONS,
    classes: &[
        c!("logicexception"),
        c!("badfunctioncallexception"),
        c!("badmethodcallexception"),
        c!("domainexception"),
        c!("invalidargumentexception"),
        c!("lengthexception"),
        c!("outofrangeexception"),
        c!("runtimeexception"),
        c!("outofboundsexception"),
        c!("overflowexception"),
        c!("rangeexception"),
        c!("underflowexception"),
        c!("unexpectedvalueexception"),
        c!("outeriterator"),
        c!("recursiveiterator"),
        c!("seekableiterator"),
        c!("splobserver"),
        c!("splsubject"),
        c!("arrayobject"),
        c!("arrayiterator"),
        c!("recursivearrayiterator"),
        c!("splfixedarray"),
        c!("internaliterator"),
        c!("spldoublylinkedlist"),
        c!("splqueue"),
        c!("splstack"),
        c!("splheap"),
        c!("splminheap"),
        c!("splmaxheap"),
        c!("splpriorityqueue"),
        c!("splobjectstorage"),
        c!("multipleiterator"),
        it!("iteratoriterator"),
        it!("filteriterator"),
        it!("callbackfilteriterator"),
        it!("recursivefilteriterator"),
        it!("parentiterator"),
        it!("recursivecallbackfilteriterator"),
        it!("limititerator"),
        it!("cachingiterator"),
        it!("recursivecachingiterator"),
        it!("norewinditerator"),
        it!("infiniteiterator"),
        it!("appenditerator"),
        it!("recursiveiteratoriterator"),
        it!("recursivetreeiterator"),
        it!("emptyiterator"),
        ("regexiterator", REGEX),
        ("recursiveregexiterator", REGEX),
        fl!("splfileinfo"),
        fl!("splfileobject"),
        fl!("spltempfileobject"),
        fl!("directoryiterator"),
        fl!("filesystemiterator"),
        fl!("recursivedirectoryiterator"),
        fl!("globiterator"),
    ],
    methods: &[
        // ArrayObject / ArrayIterator
        ("arrayobject::__construct", ao_construct),
        ("arrayobject::offsetexists", ao_offset_exists),
        ("arrayobject::offsetget", ao_offset_get),
        ("arrayobject::offsetset", ao_offset_set),
        ("arrayobject::offsetunset", ao_offset_unset),
        ("arrayobject::append", ao_append),
        ("arrayobject::getarraycopy", ao_copy),
        ("arrayobject::count", ao_count),
        ("arrayobject::getflags", ao_get_flags),
        ("arrayobject::setflags", ao_set_flags),
        ("arrayobject::asort", ao_asort),
        ("arrayobject::ksort", ao_ksort),
        ("arrayobject::uasort", ao_uasort),
        ("arrayobject::uksort", ao_uksort),
        ("arrayobject::natsort", ao_natsort),
        ("arrayobject::natcasesort", ao_natcasesort),
        ("arrayobject::unserialize", ao_unserialize_str),
        ("arrayobject::serialize", ao_serialize_str),
        ("arrayobject::__serialize", ao_serialize),
        ("arrayobject::__unserialize", ao_unserialize),
        ("arrayobject::getiterator", ao_get_iterator),
        ("arrayobject::exchangearray", ao_exchange),
        ("arrayobject::setiteratorclass", ao_set_iter_class),
        ("arrayobject::getiteratorclass", ao_get_iter_class),
        ("arrayobject::__debuginfo", ao_debug_info),
        ("arrayobject::__get", ao_magic_get),
        ("arrayobject::__set", ao_magic_set),
        ("arrayobject::__isset", ao_magic_isset),
        ("arrayobject::__unset", ao_magic_unset),
        ("arrayiterator::__construct", ai_construct),
        ("arrayiterator::offsetexists", ao_offset_exists),
        ("arrayiterator::offsetget", ao_offset_get),
        ("arrayiterator::offsetset", ao_offset_set),
        ("arrayiterator::offsetunset", ao_offset_unset),
        ("arrayiterator::append", ao_append),
        ("arrayiterator::getarraycopy", ao_copy),
        ("arrayiterator::count", ao_count),
        ("arrayiterator::getflags", ao_get_flags),
        ("arrayiterator::setflags", ao_set_flags),
        ("arrayiterator::asort", ao_asort),
        ("arrayiterator::ksort", ao_ksort),
        ("arrayiterator::uasort", ao_uasort),
        ("arrayiterator::uksort", ao_uksort),
        ("arrayiterator::natsort", ao_natsort),
        ("arrayiterator::natcasesort", ao_natcasesort),
        ("arrayiterator::unserialize", ao_unserialize_str),
        ("arrayiterator::serialize", ao_serialize_str),
        ("arrayiterator::__serialize", ao_serialize),
        ("arrayiterator::__unserialize", ao_unserialize),
        ("arrayiterator::rewind", ai_rewind),
        ("arrayiterator::current", ai_current),
        ("arrayiterator::key", ai_key),
        ("arrayiterator::next", ai_next),
        ("arrayiterator::valid", ai_valid),
        ("arrayiterator::seek", ai_seek),
        ("arrayiterator::__debuginfo", ao_debug_info),
        ("arrayiterator::__get", ao_magic_get),
        ("arrayiterator::__set", ao_magic_set),
        ("arrayiterator::__isset", ao_magic_isset),
        ("arrayiterator::__unset", ao_magic_unset),
        ("recursivearrayiterator::haschildren", rai_has_children),
        ("recursivearrayiterator::getchildren", rai_get_children),
        // SplFixedArray
        ("splfixedarray::__construct", fa_construct),
        ("splfixedarray::__wakeup", fa_wakeup),
        ("splfixedarray::__serialize", fa_serialize),
        ("splfixedarray::__unserialize", fa_unserialize),
        ("splfixedarray::count", fa_count),
        ("splfixedarray::toarray", fa_to_array),
        ("splfixedarray::fromarray", fa_from_array),
        ("splfixedarray::getsize", fa_count),
        ("splfixedarray::setsize", fa_set_size),
        ("splfixedarray::offsetexists", fa_offset_exists),
        ("splfixedarray::offsetget", fa_offset_get),
        ("splfixedarray::offsetset", fa_offset_set),
        ("splfixedarray::offsetunset", fa_offset_unset),
        ("splfixedarray::getiterator", fa_get_iterator),
        ("splfixedarray::jsonserialize", fa_to_array),
        ("internaliterator::current", ii_current),
        ("internaliterator::key", ii_key),
        ("internaliterator::next", ii_next),
        ("internaliterator::valid", ii_valid),
        ("internaliterator::rewind", ii_rewind),
        // SplDoublyLinkedList
        ("spldoublylinkedlist::add", dll_add),
        ("spldoublylinkedlist::pop", dll_pop),
        ("spldoublylinkedlist::shift", dll_shift),
        ("spldoublylinkedlist::push", dll_push),
        ("spldoublylinkedlist::unshift", dll_unshift),
        ("spldoublylinkedlist::top", dll_top),
        ("spldoublylinkedlist::bottom", dll_bottom),
        ("spldoublylinkedlist::__debuginfo", dll_debug_info),
        ("spldoublylinkedlist::count", dll_count),
        ("spldoublylinkedlist::isempty", dll_is_empty),
        ("spldoublylinkedlist::setiteratormode", dll_set_mode),
        ("spldoublylinkedlist::getiteratormode", dll_get_mode),
        ("spldoublylinkedlist::offsetexists", dll_offset_exists),
        ("spldoublylinkedlist::offsetget", dll_offset_get),
        ("spldoublylinkedlist::offsetset", dll_offset_set),
        ("spldoublylinkedlist::offsetunset", dll_offset_unset),
        ("spldoublylinkedlist::rewind", dll_rewind),
        ("spldoublylinkedlist::current", dll_current),
        ("spldoublylinkedlist::key", dll_key),
        ("spldoublylinkedlist::prev", dll_prev),
        ("spldoublylinkedlist::next", dll_next),
        ("spldoublylinkedlist::valid", dll_valid),
        ("spldoublylinkedlist::unserialize", dll_unserialize_str),
        ("spldoublylinkedlist::serialize", dll_serialize_str),
        ("spldoublylinkedlist::__serialize", dll_serialize),
        ("spldoublylinkedlist::__unserialize", dll_unserialize),
        ("splqueue::enqueue", dll_push),
        ("splqueue::dequeue", dll_shift),
        // heaps
        ("splheap::extract", heap_extract),
        ("splheap::insert", heap_insert),
        ("splheap::top", heap_top),
        ("splheap::count", heap_count),
        ("splheap::isempty", heap_is_empty),
        ("splheap::rewind", noop),
        ("splheap::current", heap_current),
        ("splheap::key", heap_key),
        ("splheap::next", heap_next),
        ("splheap::valid", heap_valid),
        ("splheap::recoverfromcorruption", heap_recover),
        ("splheap::iscorrupted", heap_is_corrupted),
        ("splheap::__debuginfo", heap_debug_info),
        ("splheap::__serialize", heap_serialize),
        ("splheap::__unserialize", heap_unserialize),
        ("splminheap::compare", minheap_compare),
        ("splmaxheap::compare", maxheap_compare),
        ("splpriorityqueue::compare", pq_compare),
        ("splpriorityqueue::insert", pq_insert),
        ("splpriorityqueue::setextractflags", pq_set_flags),
        ("splpriorityqueue::top", heap_top),
        ("splpriorityqueue::extract", heap_extract),
        ("splpriorityqueue::count", heap_count),
        ("splpriorityqueue::isempty", heap_is_empty),
        ("splpriorityqueue::rewind", noop),
        ("splpriorityqueue::current", heap_current),
        ("splpriorityqueue::key", heap_key),
        ("splpriorityqueue::next", heap_next),
        ("splpriorityqueue::valid", heap_valid),
        ("splpriorityqueue::recoverfromcorruption", heap_recover),
        ("splpriorityqueue::iscorrupted", heap_is_corrupted),
        ("splpriorityqueue::getextractflags", pq_get_flags),
        ("splpriorityqueue::__debuginfo", heap_debug_info),
        ("splpriorityqueue::__serialize", heap_serialize),
        ("splpriorityqueue::__unserialize", heap_unserialize),
        // SplObjectStorage
        ("splobjectstorage::attach", os_attach),
        ("splobjectstorage::detach", os_detach),
        ("splobjectstorage::contains", os_contains),
        ("splobjectstorage::addall", os_add_all),
        ("splobjectstorage::removeall", os_remove_all),
        ("splobjectstorage::removeallexcept", os_remove_all_except),
        ("splobjectstorage::getinfo", os_get_info),
        ("splobjectstorage::setinfo", os_set_info),
        ("splobjectstorage::count", os_count),
        ("splobjectstorage::rewind", os_rewind),
        ("splobjectstorage::valid", os_valid),
        ("splobjectstorage::key", os_key_m),
        ("splobjectstorage::current", os_current),
        ("splobjectstorage::next", os_next),
        ("splobjectstorage::seek", os_seek),
        ("splobjectstorage::unserialize", os_unserialize_str),
        ("splobjectstorage::serialize", os_serialize_str),
        ("splobjectstorage::offsetexists", os_offset_exists),
        ("splobjectstorage::offsetget", os_offset_get),
        ("splobjectstorage::offsetset", os_offset_set),
        ("splobjectstorage::offsetunset", os_offset_unset),
        ("splobjectstorage::gethash", os_get_hash),
        ("splobjectstorage::__serialize", os_serialize),
        ("splobjectstorage::__unserialize", os_unserialize),
        ("splobjectstorage::__debuginfo", os_debug_info),
        // MultipleIterator
        ("multipleiterator::__construct", mi_construct),
        ("multipleiterator::getflags", mi_get_flags),
        ("multipleiterator::setflags", mi_set_flags),
        ("multipleiterator::attachiterator", mi_attach),
        ("multipleiterator::detachiterator", mi_detach),
        ("multipleiterator::containsiterator", mi_contains),
        ("multipleiterator::countiterators", mi_count),
        ("multipleiterator::rewind", mi_rewind),
        ("multipleiterator::valid", mi_valid),
        ("multipleiterator::key", mi_key),
        ("multipleiterator::current", mi_current),
        ("multipleiterator::next", mi_next),
        ("multipleiterator::__debuginfo", os_debug_info),
    ],
    hooks: &[
        ("arrayobject", ao_hooks),
        ("arrayiterator", ao_hooks),
        ("splfixedarray", fa_hooks),
        ("spldoublylinkedlist", dll_hooks),
        ("splheap", heap_hooks),
        ("splpriorityqueue", heap_hooks),
        ("splobjectstorage", os_hooks),
        ("multipleiterator", os_hooks),
        ("splfileobject", sfo_hooks),
        ("globiterator", glob_hooks),
        ("recursiveiteratoriterator", rii_hooks),
    ],
    listed: true,
    ..Module::EMPTY
};

fn noop(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Null)
}

/// Runs `f` on the native data of type `T` of `o`.
fn with<T: 'static, X>(o: &ObjRef, f: impl FnOnce(&mut T) -> X) -> Option<X> {
    let mut n = o.obj().native.borrow_mut();
    let any: &mut dyn std::any::Any = &mut **n.as_mut()?;
    any.downcast_mut::<T>().map(f)
}

fn set_native<T: goro_runtime::NativeData>(o: &ObjRef, d: T) {
    *o.obj().native.borrow_mut() = Some(Box::new(d));
}

/// Properties of the object as `(array)` would give them without the
/// class's own table hook (for __debugInfo / __serialize members).
fn members(vm: &mut Vm, o: &ObjRef) -> PhpArray {
    vm.plain_props_array(o, true)
}

/// A mangled private name "\0Class\0name".
fn private_key(class: &str, name: &str) -> Key {
    let mut v = vec![0u8];
    v.extend_from_slice(class.as_bytes());
    v.push(0);
    v.extend_from_slice(name.as_bytes());
    Key::Str(PhpStr::from(v))
}

/// Restores serialized members (mangled names select private and protected
/// properties).
fn restore_members(vm: &mut Vm, o: &ObjRef, m: &PhpArray) -> R<()> {
    for (k, v) in m.iter() {
        vm.unser_set_prop(o, k.to_owned(), v.deref())?;
    }
    Ok(())
}

fn key_desc(k: &Key) -> String {
    match k {
        Key::Int(i) => i.to_string(),
        Key::Str(s) => format!("\"{}\"", s.to_string_lossy()),
    }
}

fn call_fn(vm: &mut Vm, name: &str, args: Vec<Value>) -> R<Value> {
    let Some(f) = vm.functions.get(&PhpStr::from(name)).cloned() else {
        // Removed by disable_functions (test ArrayObject/asort_disabled).
        return vm.throw(
            "Error",
            &format!("Cannot call method {name} when function {name} is disabled"),
        );
    };
    vm.call_callee(Callee::func(f), args)
}

/// The method `lname` of the object's class, if a user class overrides the
/// internal one.
fn user_override(vm: &Vm, o: &ObjRef, lname: &[u8]) -> Option<Rc<crate::func::Func>> {
    let cls = vm.class_of(o);
    cls.find_method(lname).filter(|f| !f.is_native())
}

// ================================================================ ArrayObject

pub struct AoData {
    /// An array, or the object whose properties are used (an ArrayObject
    /// or ArrayIterator: its storage).
    storage: Value,
    flags: i64,
    iter_class: PhpStr,
    /// Iteration position (hash position) for ArrayIterator.
    pos: u32,
    /// A sort is running: the data can't change (test
    /// ArrayObject_exchange_array_during_sorting).
    sorting: bool,
    /// A RecursiveArrayIterator child made from an array element: the
    /// parent and the element's position. The data is that element, so
    /// changes reach the parent (tests bug42654, bug42654_2); `storage`
    /// is the fallback once the element is gone.
    link: Option<(ObjRef, u32)>,
}

impl goro_runtime::NativeData for AoData {
    fn trace(&self, visit: &mut dyn FnMut(&Value)) {
        visit(&self.storage);
        if let Some((p, _)) = &self.link {
            visit(&Value::Object(p.clone()));
        }
    }
    fn clear(&mut self) {
        self.storage = Value::Array(PhpArray::new());
    }
}

fn ao_hooks() -> NativeHooks {
    NativeHooks {
        create: Some(|_, o| {
            set_native(
                o,
                AoData {
                    storage: Value::Array(PhpArray::new()),
                    flags: 0,
                    iter_class: PhpStr::from("ArrayIterator"),
                    pos: 0,
                    sorting: false,
                    link: None,
                },
            )
        }),
        clone: Some(|vm, from, to| {
            let shares = is_subclass_named(&vm.class_of(from), b"arrayiterator");
            // A cloned ArrayObject wrapping another object gets a copy of
            // its data (tests ArrayObject_clone_other_std_props,
            // arrayObject_clone_basic2).
            let copy = match Some(ao_own_storage(from)) {
                Some(Value::Object(x)) if !shares && !ObjRef::ptr_eq(&x, from) => {
                    Some(ao_array(vm, from))
                }
                _ => None,
            };
            let own = ao_own_storage(from);
            let d = with(from, |d: &mut AoData| AoData {
                storage: match (&own, copy) {
                    (_, Some(c)) => Value::Array(c),
                    (Value::Array(_), _) if shares => Value::Object(from.clone()),
                    // An object wrapping itself: the clone wraps the clone
                    // (test ArrayObject/array_022).
                    (Value::Object(x), _) if ObjRef::ptr_eq(x, from) => Value::Object(to.clone()),
                    (s, _) => s.clone(),
                },
                flags: d.flags,
                iter_class: d.iter_class.clone(),
                pos: d.pos,
                sorting: false,
                link: None,
            });
            if let Some(d) = d {
                set_native(to, d);
            }
        }),
        // STD_PROP_LIST: (array), var_export and json_encode see the
        // properties (observed on PHP 8.5).
        props_for: Some(|vm, o| {
            if ao_flags(o) & 1 != 0 {
                Some(vm.plain_props_array(o, true))
            } else {
                Some(ao_array(vm, o))
            }
        }),
        dim_storage: Some(|_, o| ao_storage_ptr(o)),
        compare: Some(ao_compare),
        count: Some(|vm, o| Ok(ao_len(vm, o))),
        ..Default::default()
    }
}

/// `==` of two ArrayObjects: the data, then the properties (test
/// bug61326).
fn ao_compare(vm: &mut Vm, x: &Value, y: &Value) -> Option<R<i32>> {
    let (Value::Object(a), Value::Object(b)) = (x, y) else {
        return None;
    };
    if !is_ao(a) || !is_ao(b) {
        return None;
    }
    let (da, db) = (ao_array(vm, a), ao_array(vm, b));
    match php_cmp(vm, &Value::Array(da), &Value::Array(db)) {
        Ok(0) => None,
        Ok(c) => Some(Ok(c as i32)),
        Err(e) => Some(Err(e)),
    }
}

fn is_ao(o: &ObjRef) -> bool {
    with(o, |_: &mut AoData| ()).is_some()
}

/// The innermost ArrayObject/ArrayIterator holding the data (storage
/// chains through wrapped ArrayObjects).
fn ao_owner(o: &ObjRef) -> ObjRef {
    let mut cur = o.clone();
    for _ in 0..1000 {
        let next = with(&cur, |d: &mut AoData| match &d.storage {
            Value::Object(x) => Some(x.clone()),
            _ => None,
        })
        .flatten();
        match next {
            Some(x) if is_ao(&x) => cur = x,
            _ => break,
        }
    }
    cur
}

/// The storage array's slot, when the data is an array.
fn ao_storage_ptr(o: &ObjRef) -> Option<*mut Value> {
    let owner = ao_owner(o);
    let link = with(&owner, |d: &mut AoData| d.link.clone()).flatten();
    if let Some((parent, pos)) = link
        && let Some(p) = ao_elem_ptr(&parent, pos)
    {
        return Some(p);
    }
    with(&owner, |d: &mut AoData| {
        if matches!(d.storage, Value::Array(_)) {
            Some(&mut d.storage as *mut Value)
        } else {
            None
        }
    })
    .flatten()
}

/// The slot of the array at position `pos` of `parent`'s data (see
/// [`AoData::link`]).
fn ao_elem_ptr(parent: &ObjRef, pos: u32) -> Option<*mut Value> {
    let pp = ao_storage_ptr(parent)?;
    // SAFETY: storage slots live in the owners' native data, which nothing
    // replaces while the pointer is used.
    let Value::Array(a) = (unsafe { &mut *pp }) else {
        return None;
    };
    let v = a.make_mut().value_at_mut(pos)? as *mut Value;
    match unsafe { &mut *v } {
        Value::Array(_) => Some(v),
        Value::Ref(r) => {
            let mut g = r.borrow_mut();
            let p = &mut *g as *mut Value;
            drop(g);
            matches!(unsafe { &*p }, Value::Array(_)).then_some(p)
        }
        _ => None,
    }
}

/// The object's own data (an array or an object), following a child's
/// link.
fn ao_own_storage(o: &ObjRef) -> Value {
    let link = with(o, |d: &mut AoData| d.link.clone()).flatten();
    if let Some((parent, pos)) = link
        && let Some(p) = ao_elem_ptr(&parent, pos)
    {
        return unsafe { (*p).clone() };
    }
    with(o, |d: &mut AoData| d.storage.clone()).unwrap_or_default()
}

/// The wrapped plain object, when the data is an object's properties.
fn ao_wrapped(o: &ObjRef) -> Option<ObjRef> {
    let owner = ao_owner(o);
    with(&owner, |d: &mut AoData| match &d.storage {
        Value::Object(x) => Some(x.clone()),
        _ => None,
    })
    .flatten()
}

/// The data as an array (a copy of the wrapped object's properties).
fn ao_array(vm: &mut Vm, o: &ObjRef) -> PhpArray {
    if let Some(p) = ao_storage_ptr(o)
        && let Value::Array(a) = unsafe { &*p }
    {
        return a.clone();
    }
    match ao_wrapped(o) {
        // The object's property table, private and protected names
        // mangled (observed on PHP 8.5).
        Some(w) => {
            // Lazy objects are initialized first (test
            // ArrayObject/gh15833_1); numeric names are integer keys
            // (test ArrayObject_proptable_canonicalization).
            let w = vm.realize_deferred(&w);
            let t = vm.plain_props_array(&w, true);
            if !t.iter().any(
                |(k, _)| matches!(k, KeyRef::Str(s) if matches!(Key::from_php_str(s), Key::Int(_))),
            ) {
                return t;
            }
            let mut out = PhpArray::new();
            for (k, v) in t.iter() {
                let k = match k {
                    KeyRef::Str(s) => Key::from_php_str(s),
                    k => k.to_owned(),
                };
                out.insert(k, v.clone());
            }
            out
        }
        None => PhpArray::new(),
    }
}

/// A mangled (private or protected) property name: skipped by iteration
/// and count() over an object (observed on PHP 8.5).
fn is_mangled(k: KeyRef) -> bool {
    matches!(k, KeyRef::Str(s) if s.first() == Some(&0))
}

/// count() of an ArrayObject.
fn ao_len(vm: &mut Vm, o: &ObjRef) -> i64 {
    let arr = ao_array(vm, o);
    if ao_wrapped(o).is_some() {
        return arr.iter().filter(|(k, _)| !is_mangled(*k)).count() as i64;
    }
    arr.len() as i64
}

/// The declared property of `w` whose table name is `name`.
fn raw_prop(vm: &Vm, w: &ObjRef, name: &[u8]) -> Option<Rc<crate::class::PropInfo>> {
    let cls = vm.class_of(w);
    let props = cls.props.borrow();
    props
        .iter()
        .find(|p| {
            if p.flags & (crate::class::mflags::VIRTUAL | crate::class::mflags::HIDDEN) != 0 {
                return false;
            }
            if p.flags & crate::class::mflags::PUBLIC != 0 {
                return p.name.as_bytes() == name;
            }
            let owner: &[u8] = if p.flags & crate::class::mflags::PROTECTED != 0 {
                b"*"
            } else {
                unsafe { &(*p.class).name }
            };
            name.len() == owner.len() + p.name.len() + 2
                && name[0] == 0
                && &name[1..owner.len() + 1] == owner
                && name[owner.len() + 1] == 0
                && &name[owner.len() + 2..] == p.name.as_bytes()
        })
        .cloned()
}

/// Writes the wrapped object's property table directly: no hooks, type
/// checks or dynamic-property deprecation (observed on PHP 8.5; test
/// ArrayObject/property_hooks).
fn raw_write(vm: &mut Vm, w: &ObjRef, k: &Key, v: Value) {
    let w = &vm.realize_deferred(w);
    let name = key_value(k);
    if let Some(p) = raw_prop(vm, w, &name) {
        let old = std::mem::replace(&mut w.obj().slots.borrow_mut()[p.slot as usize], v);
        drop(old);
        return;
    }
    let old = w.obj().props.borrow_mut().insert(Key::Str(name), v);
    drop(old);
}

fn raw_unset(vm: &mut Vm, w: &ObjRef, k: &Key) {
    let w = &vm.realize_deferred(w);
    let name = key_value(k);
    if let Some(p) = raw_prop(vm, w, &name) {
        let mark = vm.unset_value();
        let old = std::mem::replace(&mut w.obj().slots.borrow_mut()[p.slot as usize], mark);
        drop(old);
        return;
    }
    let old = w.obj().props.borrow_mut().remove(KeyRef::Str(&name));
    drop(old);
}

/// Serialized flag of an ArrayObject that wraps itself (observed on PHP
/// 8.5; test ArrayObject/bug74669).
const AO_IS_SELF: i64 = 0x0100_0000;

fn ao_flags(o: &ObjRef) -> i64 {
    with(o, |d: &mut AoData| d.flags).unwrap_or(0)
}

fn ao_class_name(vm: &Vm, a: &Args) -> &'static str {
    let cls = vm.class_of(a.this_obj());
    let mut c: Option<&Class> = Some(&cls);
    while let Some(k) = c {
        match k.lname.as_bytes() {
            b"arrayobject" => return "ArrayObject",
            b"arrayiterator" => return "ArrayIterator",
            _ => c = k.parent.as_deref(),
        }
    }
    "ArrayObject"
}

/// The new data of an ArrayObject from a constructor/exchangeArray
/// argument.
fn ao_input(vm: &mut Vm, a: &Args, v: Value, fname: &str) -> R<Value> {
    match v {
        Value::Array(_) => Ok(v),
        Value::Object(ref x) => {
            ao_deprecate_object(vm, a, fname)?;
            ao_check_object(vm, a, x)?;
            Ok(v)
        }
        _ => Ok(Value::Array(PhpArray::new())),
    }
}

/// PHP 8.5 deprecates every object backing, ArrayObjects included (test
/// ArrayObject/arrayObject_getFlags_basic1).
fn ao_deprecate_object(vm: &mut Vm, a: &Args, fname: &str) -> R<()> {
    vm.deprecated(&format!(
        "{}::{}(): Using an object as a backing array for {} is deprecated, as it allows violating class constraints and invariants",
        ao_class_name(vm, a),
        fname,
        ao_class_name(vm, a)
    ))
}

/// Refuses objects that can't back an ArrayObject.
fn ao_check_object(vm: &mut Vm, a: &Args, x: &ObjRef) -> R<()> {
    if is_ao(x) {
        return Ok(());
    }
    let cls = vm.class_of(x);
    if cls.is_enum() {
        // test ArrayObject/ArrayObject_enum
        return vm.throw(
            "InvalidArgumentException",
            &format!("Enums are not compatible with {}", ao_class_name(vm, a)),
        );
    }
    // Objects whose property table is computed (SplFixedArray, the date
    // classes, WeakMap) are refused (tests ArrayObject_overloaded_*;
    // observed on PHP 8.5).
    if cls.native.props_for.is_some() || cls.native.dyn_prop.is_some() {
        return vm.throw(
            "InvalidArgumentException",
            &format!(
                "Overloaded object of type {} is not compatible with {}",
                cls.name.to_string_lossy(),
                ao_class_name(vm, a)
            ),
        );
    }
    Ok(())
}

fn ao_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let v = if a.has(0) {
        a.val(0)
    } else {
        Value::Array(PhpArray::new())
    };
    let flags = a.int_or(1, 0);
    let ic = if a.has(2) {
        a.str(2)
    } else {
        PhpStr::from("ArrayIterator")
    };
    // The iterator class is checked first (test
    // arrayObject___construct_error1: no deprecation then).
    let ic = check_iter_class(vm, &ic, "__construct(): Argument #3 ($iteratorClass)")?;
    let storage = ao_input(vm, a, v, "__construct")?;
    let flags = ao_inherit_flags(a, &storage, flags);
    with(&o, |d: &mut AoData| {
        d.link = None;
        d.storage = storage;
        d.flags = flags;
        d.iter_class = ic;
        d.pos = 0;
    });
    Ok(Value::Null)
}

/// Without a flags argument, an ArrayObject built on another one takes its
/// flags (test ArrayObject/arrayObject_getFlags_basic2).
fn ao_inherit_flags(a: &Args, storage: &Value, flags: i64) -> i64 {
    match storage {
        Value::Object(x) if !a.has(1) && is_ao(x) => ao_flags(x),
        _ => flags,
    }
}

fn check_iter_class(vm: &mut Vm, name: &PhpStr, what: &str) -> R<PhpStr> {
    let c = vm.find_class(name, true)?;
    match c {
        Some(c) if is_subclass_named(&c, b"arrayiterator") => Ok(c.name.clone()),
        _ => vm.type_error(&format!(
            "ArrayObject::{} must be a class name derived from ArrayIterator, {} given",
            what,
            name.to_string_lossy()
        )),
    }
}

fn is_subclass_named(c: &Class, lname: &[u8]) -> bool {
    let mut k: Option<&Class> = Some(c);
    while let Some(x) = k {
        if x.lname.as_bytes() == lname {
            return true;
        }
        k = x.parent.as_deref();
    }
    false
}

fn ai_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let v = if a.has(0) {
        a.val(0)
    } else {
        Value::Array(PhpArray::new())
    };
    let storage = ao_input(vm, a, v, "__construct")?;
    let flags = ao_inherit_flags(a, &storage, a.int_or(1, 0));
    with(&o, |d: &mut AoData| {
        d.link = None;
        d.storage = storage;
        d.flags = flags;
        d.pos = 0;
    });
    Ok(Value::Null)
}

fn ao_key(vm: &mut Vm, a: &Args, k: &Value) -> R<Key> {
    match k {
        Value::Int(i) => Ok(Key::Int(*i)),
        Value::Str(s) => Ok(Key::from_php_str(s)),
        Value::Null => Ok(Key::Str(PhpStr::empty())),
        Value::Bool(b) => Ok(Key::Int(*b as i64)),
        Value::Array(_) | Value::Object(_) => vm.type_error(&format!(
            "Cannot access offset of type {} on {}",
            k.tname(),
            vm.class_of(a.this_obj()).name.dname()
        )),
        _ => vm.to_key(k, "array"),
    }
}

fn key_value(k: &Key) -> PhpStr {
    match k {
        Key::Int(i) => PhpStr::from(i.to_string()),
        Key::Str(s) => s.clone(),
    }
}

fn ao_offset_exists(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let k = ao_key(vm, a, &a.val(0))?;
    if let Some(p) = ao_storage_ptr(&o)
        && let Value::Array(arr) = unsafe { &*p }
    {
        return Ok(Value::Bool(arr.get(k.as_ref()).is_some()));
    }
    let arr = ao_array(vm, &o);
    Ok(Value::Bool(arr.get(k.as_ref()).is_some()))
}

/// property_exists() on an ArrayObject with ARRAY_AS_PROPS: its elements
/// count (test bug53515).
pub(crate) fn ao_has_prop(vm: &mut Vm, o: &ObjRef, name: &PhpStr) -> bool {
    if !is_ao(o) || ao_flags(o) & 2 == 0 {
        return false;
    }
    ao_array(vm, o)
        .get(Key::from_php_str(name).as_ref())
        .is_some()
}

/// isset($o[$k]) on an internal ArrayObject: exists and is not null.
pub(crate) fn ao_isset(vm: &mut Vm, o: &ObjRef, k: &Value) -> R<Option<Value>> {
    let key = match k {
        Value::Int(i) => Key::Int(*i),
        Value::Str(s) => Key::from_php_str(s),
        Value::Null => Key::Str(PhpStr::empty()),
        Value::Bool(b) => Key::Int(*b as i64),
        Value::Array(_) | Value::Object(_) => {
            // test ArrayObject_illegal_offset
            return vm.type_error(&format!(
                "Cannot access offset of type {} in isset or empty",
                k.tname()
            ));
        }
        _ => vm.to_key(k, "array")?,
    };
    let arr = ao_array(vm, o);
    Ok(arr.get(key.as_ref()).map(|v| v.deref()))
}

fn ao_offset_get(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    if a.val(0).is_null() {
        // test bug70852
        vm.deprecated("Using null as an array offset is deprecated, use an empty string instead")?;
    }
    let k = ao_key(vm, a, &a.val(0))?;
    let arr = ao_array(vm, &o);
    match arr.get(k.as_ref()) {
        Some(v) => Ok(v.deref()),
        None => {
            vm.warn(&format!("Undefined array key {}", key_desc(&k)))?;
            Ok(Value::Null)
        }
    }
}

fn ao_offset_set(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    ao_modifiable(vm, &o)?;
    let kv = a.val(0);
    let v = a.val(1);
    if kv.is_null() && a.has(0) {
        return ao_push(vm, &o, v);
    }
    let k = ao_key(vm, a, &kv)?;
    if let Some(p) = ao_storage_ptr(&o) {
        if let Value::Array(arr) = unsafe { &mut *p } {
            let old = arr.assign(k, v);
            drop(old);
        }
        return Ok(Value::Null);
    }
    if let Some(w) = ao_wrapped(&o) {
        raw_write(vm, &w, &k, v);
    }
    Ok(Value::Null)
}

fn ao_push(vm: &mut Vm, o: &ObjRef, v: Value) -> R<Value> {
    ao_modifiable(vm, o)?;
    if let Some(p) = ao_storage_ptr(o) {
        if let Value::Array(arr) = unsafe { &mut *p }
            && arr.push(v).is_err()
        {
            return vm
                .warn("Cannot add element to the array as the next element is already occupied")
                .map(|_| Value::Null);
        }
        return Ok(Value::Null);
    }
    let cn = vm.class_of(o).name.dname();
    vm.throw(
        "Error",
        &format!(
            "Cannot append properties to objects, use {}::offsetSet() instead",
            cn
        ),
    )
}

fn ao_offset_unset(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    ao_modifiable(vm, &o)?;
    let kv = a.val(0);
    if matches!(kv, Value::Array(_) | Value::Object(_)) {
        // test ArrayObject_illegal_offset
        return vm.type_error(&format!(
            "Cannot unset offset of type {} on {}",
            kv.tname(),
            vm.class_of(&o).name.dname()
        ));
    }
    let k = ao_key(vm, a, &kv)?;
    if let Some(p) = ao_storage_ptr(&o) {
        let old = if let Value::Array(arr) = unsafe { &mut *p } {
            arr.remove(k.as_ref())
        } else {
            None
        };
        drop(old);
        return Ok(Value::Null);
    }
    if let Some(w) = ao_wrapped(&o) {
        raw_unset(vm, &w, &k);
    }
    Ok(Value::Null)
}

fn ao_append(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    ao_push(vm, &o, a.val(0))
}

fn ao_copy(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let mut arr = ao_array(vm, &o);
    // A copy without the references' sharing of the internal array.
    arr.make_mut();
    Ok(Value::Array(arr))
}

fn ao_count(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    Ok(Value::Int(ao_len(vm, &o)))
}

fn ao_get_flags(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Int(ao_flags(a.this_obj())))
}

fn ao_set_flags(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.int(0);
    with(a.this_obj(), |d: &mut AoData| d.flags = f);
    Ok(Value::Null)
}

/// Runs an array sorting function on the data.
/// Refuses changes while a sort runs.
fn ao_modifiable(vm: &mut Vm, o: &ObjRef) -> R<()> {
    let owner = ao_owner(o);
    if with(&owner, |d: &mut AoData| d.sorting).unwrap_or(false)
        || with(o, |d: &mut AoData| d.sorting).unwrap_or(false)
    {
        return vm.throw(
            "Error",
            "Modification of ArrayObject during sorting is prohibited",
        );
    }
    Ok(())
}

fn ao_sort(vm: &mut Vm, a: &mut Args, fname: &str, extra: Option<Value>) -> R<Value> {
    let o = a.this_obj().clone();
    ao_modifiable(vm, &o)?;
    let arr = ao_array(vm, &o);
    let r = PhpRef::new(Value::Array(arr));
    let mut args = vec![Value::Ref(r.clone())];
    if let Some(e) = extra {
        args.push(e);
    }
    let owner = ao_owner(&o);
    with(&owner, |d: &mut AoData| d.sorting = true);
    let res = call_fn(vm, fname, args);
    with(&owner, |d: &mut AoData| d.sorting = false);
    res?;
    let sorted = r.get();
    if let Some(p) = ao_storage_ptr(&o) {
        let old = unsafe { std::mem::replace(&mut *p, sorted) };
        drop(old);
    } else if let Some(w) = ao_wrapped(&o)
        && let Value::Array(sa) = sorted
    {
        // Reorder the wrapped object's property table (declared
        // properties keep their slots).
        for (k, _) in sa.iter() {
            raw_unset(vm, &w, &k.to_owned());
        }
        for (k, v) in sa.iter() {
            raw_write(vm, &w, &k.to_owned(), v.clone());
        }
    }
    Ok(Value::Bool(true))
}

fn ao_asort(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.has(0).then(|| a.val(0));
    ao_sort(vm, a, "asort", f)
}
fn ao_ksort(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.has(0).then(|| a.val(0));
    ao_sort(vm, a, "ksort", f)
}
fn ao_uasort(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.val(0);
    ao_sort(vm, a, "uasort", Some(f))
}
fn ao_uksort(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.val(0);
    ao_sort(vm, a, "uksort", Some(f))
}
fn ao_natsort(vm: &mut Vm, a: &mut Args) -> R<Value> {
    ao_sort(vm, a, "natsort", None)
}
fn ao_natcasesort(vm: &mut Vm, a: &mut Args) -> R<Value> {
    ao_sort(vm, a, "natcasesort", None)
}

fn is_iterator_class(vm: &Vm, a: &Args) -> bool {
    ao_class_name(vm, a) == "ArrayIterator"
}

fn ao_serialize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let storage = ao_own_storage(&o);
    let (flags, ic) = with(&o, |d: &mut AoData| (d.flags, d.iter_class.clone())).unwrap();
    // An object wrapping itself is flagged instead of stored (test
    // ArrayObject/bug74669).
    let (flags, storage) = match &storage {
        Value::Object(x) if ObjRef::ptr_eq(x, &o) => (flags | AO_IS_SELF, Value::Null),
        _ => (flags, storage),
    };
    let mut out = PhpArray::new();
    let _ = out.push(Value::Int(flags));
    let _ = out.push(storage);
    let _ = out.push(Value::Array(members(vm, &o)));
    if is_iterator_class(vm, a) || ic.as_bytes() == b"ArrayIterator" {
        let _ = out.push(Value::Null);
    } else {
        let _ = out.push(Value::Str(ic));
    }
    Ok(Value::Array(out))
}

fn ao_unserialize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let data = a.array(0);
    let flags = data.get_int(0).map(|v| v.deref());
    let storage = data.get_int(1).map(|v| v.deref());
    let members_v = data.get_int(2).map(|v| v.deref());
    let (Some(Value::Int(flags)), Some(storage), Some(Value::Array(m))) =
        (flags, storage, members_v)
    else {
        return vm.throw(
            "UnexpectedValueException",
            "Incomplete or ill-typed serialization data",
        );
    };
    // The iterator class (ArrayObject only) is a string or null (test
    // unserialize_errors).
    let ic = if is_iterator_class(vm, a) {
        None
    } else {
        match data.get_int(3).map(|v| v.deref()) {
            None | Some(Value::Null) => None,
            Some(Value::Str(s)) => Some(s),
            Some(_) => {
                return vm.throw(
                    "UnexpectedValueException",
                    "Incomplete or ill-typed serialization data",
                );
            }
        }
    };
    if flags & AO_IS_SELF != 0 {
        with(&o, |d: &mut AoData| {
            d.flags = flags & !AO_IS_SELF;
            d.link = None;
            d.storage = Value::Object(o.clone());
        });
        restore_members(vm, &o, &m)?;
        return Ok(Value::Null);
    }
    if !matches!(storage, Value::Array(_) | Value::Object(_)) {
        return vm.throw(
            "InvalidArgumentException",
            "Passed variable is not an array or object",
        );
    }
    let storage = if matches!(storage, Value::Object(_)) {
        // test ArrayObject/array_025
        ao_input(vm, a, storage, "__unserialize")?
    } else {
        storage
    };
    with(&o, |d: &mut AoData| {
        d.flags = flags;
        d.link = None;
        d.storage = storage;
    });
    restore_members(vm, &o, &m)?;
    if let Some(ic) = ic {
        let cname = ao_class_name(vm, a);
        let Some(c) = vm.find_class(&ic, true)? else {
            return vm.throw(
                "UnexpectedValueException",
                &format!(
                    "Cannot deserialize {} with iterator class '{}'; no such class exists",
                    cname,
                    ic.to_string_lossy()
                ),
            );
        };
        let iter = vm.core.iterator.clone();
        if !iter.is_some_and(|i| c.is_subclass_of(&i)) {
            return vm.throw(
                "UnexpectedValueException",
                &format!(
                    "Cannot deserialize {} with iterator class '{}'; this class does not implement the Iterator interface",
                    cname,
                    ic.to_string_lossy()
                ),
            );
        }
        with(&o, |d: &mut AoData| d.iter_class = c.name.clone());
    }
    Ok(Value::Null)
}

fn ao_serialize_str(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let storage = ao_own_storage(&o);
    let flags = with(&o, |d: &mut AoData| d.flags).unwrap();
    // One numbering, where the flags are value 1 (test bug45826).
    vm.with_ser_ctx(|vm| {
        let mut out = b"x:".to_vec();
        out.extend_from_slice(&vm.serialize_value(&Value::Int(flags))?);
        out.extend_from_slice(&vm.serialize_value(&storage)?);
        out.extend_from_slice(b";m:");
        let m = Value::Array(members(vm, &o));
        out.extend_from_slice(&vm.serialize_value(&m)?);
        Ok(Value::Str(PhpStr::from(out)))
    })
}

fn ao_unserialize_str(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    // test bug67539
    ao_modifiable(vm, &o)?;
    let s = a.str(0);
    if s.is_empty() {
        return Ok(Value::Null);
    }
    let bad = |vm: &mut Vm, pos: usize, len: usize| -> R<Value> {
        vm.throw(
            "UnexpectedValueException",
            &format!("Error at offset {} of {} bytes", pos, len),
        )
    };
    let b = s.as_bytes();
    // `x:` flags storage `;m:` members, read in the value table of the
    // unserialize() in progress, where the flags take a number (tests
    // bug70166, bug73029). Error offsets observed on PHP 8.5: a value
    // that does not parse fails where the parser stopped, one of the
    // wrong type at its start (storage) or end (flags, members).
    let parsed = vm.with_unser_ctx(|vm| -> R<Result<(i64, Value, Value), usize>> {
        if !b.starts_with(b"x:") {
            return Ok(Err(0));
        }
        let mut cur = 2;
        let flags = match vm.unser_part(&b[cur..], "ArrayObject::unserialize()")? {
            Ok((Value::Int(f), n)) => {
                cur += n;
                f
            }
            Ok((_, n)) => return Ok(Err(cur + n)),
            Err(_) => return Ok(Err(cur)),
        };
        let storage = match vm.unser_part(&b[cur..], "ArrayObject::unserialize()")? {
            Ok((v @ (Value::Array(_) | Value::Object(_)), n)) => {
                cur += n;
                v
            }
            Ok(_) => return Ok(Err(cur)),
            Err(off) => return Ok(Err(cur + off)),
        };
        if b.get(cur) != Some(&b';') {
            return Ok(Err(cur));
        }
        cur += 1;
        if !b[cur..].starts_with(b"m:") {
            return Ok(Err(cur));
        }
        cur += 2;
        let m = match vm.unser_part(&b[cur..], "ArrayObject::unserialize()")? {
            Ok((v @ Value::Array(_), _)) => v,
            Ok((_, n)) => return Ok(Err(cur + n)),
            Err(off) => return Ok(Err(cur + off)),
        };
        Ok(Ok((flags, storage, m)))
    })?;
    let (flags, storage, m) = match parsed {
        Ok(x) => x,
        Err(at) => return bad(vm, at, b.len()),
    };
    // The deprecation, the members, then the object check (test
    // ArrayObject/bug70155).
    if matches!(storage, Value::Object(_)) {
        ao_deprecate_object(vm, a, "unserialize")?;
    }
    if let Value::Array(m) = m {
        restore_members(vm, &o, &m)?;
    }
    if let Value::Object(x) = &storage {
        ao_check_object(vm, a, x)?;
    }
    with(&o, |d: &mut AoData| {
        d.flags = flags;
        d.link = None;
        d.storage = storage;
    });
    Ok(Value::Null)
}

fn ao_get_iterator(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let (ic, flags) = with(&o, |d: &mut AoData| (d.iter_class.clone(), d.flags)).unwrap();
    let Some(cls) = vm.find_class(&ic, true)? else {
        return vm.throw(
            "Error",
            &format!("Class \"{}\" not found", ic.to_string_lossy()),
        );
    };
    let it = vm.instantiate(&cls)?;
    with(&it, |d: &mut AoData| {
        d.link = None;
        d.storage = Value::Object(o.clone());
        d.flags = flags;
        d.pos = 0;
    });
    Ok(Value::Object(it))
}

fn ao_exchange(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    ao_modifiable(vm, &o)?;
    let old = ao_array(vm, &o);
    let v = a.val(0);
    let storage = ao_input(vm, a, v, "exchangeArray")?;
    with(&o, |d: &mut AoData| {
        d.link = None;
        d.storage = storage;
    });
    Ok(Value::Array(old))
}

fn ao_set_iter_class(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = a.str(0);
    let ic = check_iter_class(vm, &n, "setIteratorClass(): Argument #1 ($iteratorClass)")?;
    with(a.this_obj(), |d: &mut AoData| d.iter_class = ic);
    Ok(Value::Null)
}

fn ao_get_iter_class(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Str(
        with(a.this_obj(), |d: &mut AoData| d.iter_class.clone()).unwrap_or_default(),
    ))
}

fn ao_debug_info(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let mut out = members(vm, &o);
    let storage = ao_own_storage(&o);
    if matches!(&storage, Value::Object(x) if ObjRef::ptr_eq(x, &o)) {
        // test ArrayObject/array_022
        return Ok(Value::Array(out));
    }
    let cn = ao_class_name(vm, a);
    out.insert(private_key(cn, "storage"), storage);
    Ok(Value::Array(out))
}

/// ARRAY_AS_PROPS goes through a user override of the offset method
/// (test bug74058).
fn ao_user_offset(vm: &mut Vm, o: &ObjRef, m: &[u8], args: Vec<Value>) -> R<Option<Value>> {
    if user_override(vm, o, m).is_none() {
        return Ok(None);
    }
    vm.call_method(o, m, args).map(Some)
}

fn ao_magic_get(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let n = a.str(0);
    if ao_flags(&o) & 2 != 0
        && let Some(v) = ao_user_offset(vm, &o, b"offsetget", vec![Value::Str(n.clone())])?
    {
        return Ok(v);
    }
    if ao_flags(&o) & 2 != 0 {
        let arr = ao_array(vm, &o);
        return match arr.get(Key::from_php_str(&n).as_ref()) {
            Some(v) => Ok(v.deref()),
            None => {
                vm.warn(&format!(
                    "Undefined array key {}",
                    key_desc(&Key::from_php_str(&n))
                ))?;
                Ok(Value::Null)
            }
        };
    }
    vm.read_prop(&o, &n, false, None)
}

fn ao_magic_set(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let n = a.str(0);
    let v = a.val(1);
    if ao_flags(&o) & 2 != 0
        && ao_user_offset(vm, &o, b"offsetset", vec![Value::Str(n.clone()), v.clone()])?.is_some()
    {
        return Ok(Value::Null);
    }
    if ao_flags(&o) & 2 != 0 {
        if let Some(p) = ao_storage_ptr(&o) {
            if let Value::Array(arr) = unsafe { &mut *p } {
                let old = arr.assign(Key::from_php_str(&n), v);
                drop(old);
            }
            return Ok(Value::Null);
        }
        if let Some(w) = ao_wrapped(&o) {
            raw_write(vm, &w, &Key::from_php_str(&n), v);
        }
        return Ok(Value::Null);
    }
    vm.write_prop(&o, &n, v, None)?;
    Ok(Value::Null)
}

fn ao_magic_isset(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let n = a.str(0);
    if ao_flags(&o) & 2 != 0 {
        let arr = ao_array(vm, &o);
        return Ok(Value::Bool(
            arr.get(Key::from_php_str(&n).as_ref())
                .is_some_and(|v| !v.deref().is_null()),
        ));
    }
    Ok(Value::Bool(vm.prop_isset(&o, &n, false)?))
}

fn ao_magic_unset(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let n = a.str(0);
    if ao_flags(&o) & 2 != 0 {
        if let Some(p) = ao_storage_ptr(&o) {
            let old = if let Value::Array(arr) = unsafe { &mut *p } {
                arr.remove(Key::from_php_str(&n).as_ref())
            } else {
                None
            };
            drop(old);
            return Ok(Value::Null);
        }
        if let Some(w) = ao_wrapped(&o) {
            raw_unset(vm, &w, &Key::from_php_str(&n));
        }
        return Ok(Value::Null);
    }
    vm.unset_prop(&o, &n)?;
    Ok(Value::Null)
}

// ---------------------------------------------------------------- ArrayIterator iteration

fn ai_pos(o: &ObjRef) -> u32 {
    with(o, |d: &mut AoData| d.pos).unwrap_or(0)
}

fn ai_set_pos(o: &ObjRef, p: u32) {
    with(o, |d: &mut AoData| d.pos = p);
}

/// The element at or after the iteration position: (position, key, value).
fn ai_entry(vm: &mut Vm, o: &ObjRef) -> Option<(u32, Value, Value)> {
    let mut pos = ai_pos(o);
    let arr = ao_array(vm, o);
    let obj = ao_wrapped(o).is_some();
    loop {
        let (p, k, v) = arr.data().next_from(pos)?;
        if obj && is_mangled(k) {
            pos = p + 1;
            continue;
        }
        return Some((p, k.to_value(), v.deref()));
    }
}

/// Whether `foreach` by reference can go over `o`: an ArrayIterator
/// whose current() is the internal one (observed on PHP 8.5: an
/// overriding current() makes it "An iterator cannot be used with foreach
/// by reference").
pub(crate) fn ai_iterates_by_ref(vm: &mut Vm, o: &ObjRef) -> bool {
    let cls = vm.class_of(o);
    is_ao(o)
        && is_subclass_named(&cls, b"arrayiterator")
        && cls
            .find_method(b"current")
            .is_some_and(|f| f.code().is_none())
}

/// The current element of an ArrayIterator, by reference into its storage
/// (a property of a wrapped object, by name).
pub(crate) fn ai_current_ref(vm: &mut Vm, o: &ObjRef) -> R<Option<Value>> {
    let Some((p, k, _)) = ai_entry(vm, o) else {
        return Ok(None);
    };
    if let Some(ptr) = ao_storage_ptr(o) {
        // SAFETY: the storage slot lives in the owner's native data, which
        // nothing replaces while this runs.
        if let Value::Array(a) = unsafe { &mut *ptr }
            && let Some(v) = a.make_mut().value_at_mut(p)
        {
            return Ok(Some(Value::Ref(v.make_ref())));
        }
        return Ok(None);
    }
    let Some(w) = ao_wrapped(o) else {
        return Ok(None);
    };
    let name = match k {
        Value::Str(s) => s,
        Value::Int(i) => PhpStr::from(i.to_string()),
        _ => return Ok(None),
    };
    let r = vm.prop_action(&w, &name, 0, crate::bytecode::PathAction::MakeRef, None)?;
    Ok(Some(r))
}

fn ai_rewind(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    ai_set_pos(a.this_obj(), 0);
    Ok(Value::Null)
}
fn ai_valid(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    Ok(Value::Bool(ai_entry(vm, &o).is_some()))
}
fn ai_current(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    Ok(ai_entry(vm, &o).map(|e| e.2).unwrap_or_default())
}
fn ai_key(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    Ok(ai_entry(vm, &o).map(|e| e.1).unwrap_or_default())
}
fn ai_next(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    if let Some((p, _, _)) = ai_entry(vm, &o) {
        ai_set_pos(&o, p + 1);
    }
    Ok(Value::Null)
}
fn ai_seek(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let n = a.int(0);
    let arr = ao_array(vm, &o);
    if n >= 0 {
        let mut pos = 0u32;
        let mut i = 0i64;
        while let Some((p, _, _)) = arr.data().next_from(pos) {
            if i == n {
                ai_set_pos(&o, p);
                return Ok(Value::Null);
            }
            i += 1;
            pos = p + 1;
        }
    }
    vm.throw(
        "OutOfBoundsException",
        &format!("Seek position {} is out of range", n),
    )
}

fn rai_has_children(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let flags = ao_flags(&o);
    Ok(Value::Bool(match ai_entry(vm, &o).map(|e| e.2) {
        Some(Value::Array(_)) => true,
        Some(Value::Object(_)) => flags & 4 == 0,
        _ => false,
    }))
}

fn rai_get_children(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let flags = ao_flags(&o);
    let entry = ai_entry(vm, &o);
    let Some((pos, _, cur)) = entry else {
        return Ok(Value::Null);
    };
    match &cur {
        Value::Array(_) => {}
        Value::Object(x) if flags & 4 == 0 => {
            // An ArrayIterator child is returned as is.
            if is_subclass_named(&vm.class_of(x), b"recursivearrayiterator") {
                return Ok(cur);
            }
        }
        // The child is constructed from the value (test
        // spl_iterator_getcallchildren).
        other => {
            return vm.type_error(&format!(
                "ArrayIterator::__construct(): Argument #1 ($array) must be of type array, {} given",
                Vm::type_name_given(other)
            ));
        }
    }
    let cls = vm.class_of(&o);
    let child = vm.instantiate(&cls)?;
    if let Some(f) = user_override(vm, &child, b"__construct") {
        let c = Callee {
            func: f.clone(),
            this: Some(child.clone()),
            static_class: Rc::as_ptr(&cls),
            scope: f.scope.get(),
            closure: None,
            magic_name: None,
        };
        vm.call_callee(c, vec![cur, Value::Int(flags)])?;
    } else {
        if matches!(cur, Value::Object(_)) {
            // The child is constructed from the object (test
            // ArrayObject/bug38618).
            vm.deprecated("ArrayIterator::__construct(): Using an object as a backing array for ArrayIterator is deprecated, as it allows violating class constraints and invariants")?;
        }
        let is_arr = matches!(cur, Value::Array(_));
        with(&child, |d: &mut AoData| {
            d.link = None;
            d.storage = cur;
            d.flags = flags;
            d.pos = 0;
            // An array element is shared with the parent (tests
            // bug42654, bug42654_2).
            if is_arr {
                d.link = Some((o.clone(), pos));
            }
        });
    }
    Ok(Value::Object(child))
}

// ================================================================ SplFixedArray

pub struct FaData {
    items: Vec<Value>,
    /// setSize() is releasing elements: a setSize() from their
    /// destructors waits for it (test bug81992b).
    resizing: bool,
    pending: Option<usize>,
}

impl goro_runtime::NativeData for FaData {
    fn trace(&self, visit: &mut dyn FnMut(&Value)) {
        for v in &self.items {
            visit(v);
        }
    }
    fn clear(&mut self) {
        self.items.clear();
    }
}

fn fa_hooks() -> NativeHooks {
    fn table(vm: &mut Vm, o: &ObjRef) -> Option<PhpArray> {
        // The elements, then the properties (test
        // SplFixedArray_get_properties_for).
        let mut out = PhpArray::new();
        let items = with(o, |d: &mut FaData| d.items.clone()).unwrap_or_default();
        for (i, v) in items.into_iter().enumerate() {
            out.insert(Key::Int(i as i64), v);
        }
        for (k, v) in vm.plain_props_array(o, true).iter() {
            out.insert(k.to_owned(), v.clone());
        }
        Some(out)
    }
    NativeHooks {
        create: Some(|_, o| {
            set_native(
                o,
                FaData {
                    items: Vec::new(),
                    resizing: false,
                    pending: None,
                },
            )
        }),
        clone: Some(|_, from, to| {
            let items = with(from, |d: &mut FaData| d.items.clone()).unwrap_or_default();
            set_native(
                to,
                FaData {
                    items,
                    resizing: false,
                    pending: None,
                },
            );
        }),
        debug_props: Some(table),
        props_for: Some(table),
        count: Some(|_, o| Ok(with(o, |d: &mut FaData| d.items.len() as i64).unwrap_or(0))),
        ..Default::default()
    }
}

fn fa_items(o: &ObjRef) -> Vec<Value> {
    with(o, |d: &mut FaData| d.items.clone()).unwrap_or_default()
}

fn fa_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = a.int_or(0, 0);
    if n < 0 {
        return vm.value_error(
            "SplFixedArray::__construct(): Argument #1 ($size) must be greater than or equal to 0",
        );
    }
    fa_alloc_check(vm, n)?;
    let o = a.this_obj().clone();
    with(&o, |d: &mut FaData| {
        if d.items.is_empty() {
            d.items = vec![Value::Null; n as usize];
        }
    });
    Ok(Value::Null)
}

fn fa_wakeup(vm: &mut Vm, a: &mut Args) -> R<Value> {
    // Properties holding the elements (old serialization format) become
    // the elements.
    let o = a.this_obj().clone();
    let props: Vec<(Value, Value)> = vm.visible_props(&o);
    if fa_items(&o).is_empty() && !props.is_empty() {
        let mut items = Vec::new();
        for (k, v) in props {
            items.push(v);
            if let Value::Str(s) = &k {
                vm.unset_prop(&o, s)?;
            } else if let Value::Int(i) = &k {
                vm.unset_prop(&o, &PhpStr::from(i.to_string()))?;
            }
        }
        with(&o, |d: &mut FaData| d.items = items);
    }
    Ok(Value::Null)
}

fn fa_serialize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let mut out = PhpArray::new();
    for v in fa_items(&o) {
        let _ = out.push(v);
    }
    let m = vm.plain_props_array(&o, true);
    for (k, v) in m.iter() {
        out.insert(k.to_owned(), v.clone());
    }
    Ok(Value::Array(out))
}

fn fa_unserialize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let data = a.array(0);
    // A no-op on a non-empty array (test SplFixedArray_serialize).
    if with(&o, |d: &mut FaData| !d.items.is_empty()).unwrap_or(false) {
        return Ok(Value::Null);
    }
    let mut items = Vec::new();
    for (k, v) in data.iter() {
        match k {
            KeyRef::Int(_) => items.push(v.deref()),
            KeyRef::Str(s) => vm.write_prop(&o, s, v.deref(), None)?,
        }
    }
    with(&o, |d: &mut FaData| d.items = items);
    Ok(Value::Null)
}

fn fa_count(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Int(
        with(a.this_obj(), |d: &mut FaData| d.items.len() as i64).unwrap_or(0),
    ))
}

fn fa_to_array(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Array(PhpArray::from_values(fa_items(a.this_obj()))))
}

fn fa_from_array(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let arr = a.array(0);
    let keep = a.bool_or(1, true);
    let items = if keep && !arr.is_empty() {
        let mut max: i64 = -1;
        for (k, _) in arr.iter() {
            match k {
                KeyRef::Int(i) if i >= 0 => max = max.max(i),
                // observed on PHP 8.5
                _ => {
                    return vm.throw(
                        "InvalidArgumentException",
                        "array must contain only positive integer keys",
                    );
                }
            }
        }
        if max == i64::MAX {
            // test SplArray_fromArray
            return vm.throw("InvalidArgumentException", "integer overflow detected");
        }
        fa_alloc_check(vm, max + 1)?;
        let mut items = vec![Value::Null; (max + 1) as usize];
        for (k, v) in arr.iter() {
            if let KeyRef::Int(i) = k {
                items[i as usize] = v.deref();
            }
        }
        items
    } else {
        arr.iter().map(|(_, v)| v.deref()).collect()
    };
    let cls = vm.find_class(b"SplFixedArray", false)?.unwrap();
    let o = vm.instantiate(&cls)?;
    with(&o, |d: &mut FaData| d.items = items);
    Ok(Value::Object(o))
}

/// Refuses sizes whose allocation can't be made (test bug67247: "Possible
/// integer overflow in memory allocation (N * 16 + 0)"; then
/// memory_limit).
fn fa_alloc_check(vm: &mut Vm, n: i64) -> R<()> {
    if (n as u64)
        .checked_mul(16)
        .is_none_or(|b| b > isize::MAX as u64)
    {
        return Err(vm.fatal(&format!(
            "Possible integer overflow in memory allocation ({} * 16 + 0)",
            n
        )));
    }
    super::string::alloc_check(vm, n as u128 * 16)
}

fn fa_set_size(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = a.int(0);
    if n < 0 {
        return vm.value_error(
            "SplFixedArray::setSize(): Argument #1 ($size) must be greater than or equal to 0",
        );
    }
    fa_alloc_check(vm, n)?;
    let o = a.this_obj().clone();
    if with(&o, |d: &mut FaData| {
        if d.resizing {
            d.pending = Some(n as usize);
        }
        d.resizing
    })
    .unwrap_or(false)
    {
        return Ok(Value::Bool(true));
    }
    let mut size = n as usize;
    loop {
        let removed = with(&o, |d: &mut FaData| {
            d.resizing = true;
            if size < d.items.len() {
                d.items.split_off(size)
            } else {
                d.items.resize(size, Value::Null);
                Vec::new()
            }
        })
        .unwrap_or_default();
        // Released in order; their destructors see the new size.
        for v in removed {
            drop(v);
            if let Err(e) = vm.run_pending_destructors() {
                with(&o, |d: &mut FaData| d.resizing = false);
                return Err(e);
            }
        }
        let next = with(&o, |d: &mut FaData| {
            let p = d.pending.take();
            if p.is_none() {
                d.resizing = false;
            }
            p
        })
        .flatten();
        match next {
            Some(p) => size = p,
            None => break,
        }
    }
    Ok(Value::Bool(true))
}

/// The index of an SplFixedArray offset.
fn fa_index(vm: &mut Vm, k: &Value) -> R<i64> {
    match k {
        Value::Int(i) => Ok(*i),
        Value::Bool(b) => Ok(*b as i64),
        Value::Float(f) => {
            let i = *f as i64;
            if i as f64 != *f {
                vm.deprecated(&format!(
                    "Implicit conversion from float {} to int loses precision",
                    goro_runtime::float_fmt::to_php_string(*f, 17)
                ))?;
            }
            Ok(i)
        }
        // Only integer strings are indexes (test fixedarray_003).
        Value::Str(s) => match Key::from_php_str(s) {
            Key::Int(i) => Ok(i),
            Key::Str(_) => vm.type_error("Cannot access offset of type string on SplFixedArray"),
        },
        Value::Resource(r) => {
            vm.warn(&format!(
                "Resource ID#{} used as offset, casting to integer ({})",
                r.id(),
                r.id()
            ))?;
            Ok(r.id())
        }
        other => vm.type_error(&format!(
            "Cannot access offset of type {} on SplFixedArray",
            other.tname(),
        )),
    }
}

/// isset()/empty() on an SplFixedArray whose offsetExists()/offsetGet()
/// are the internal ones: the index is converted once (test
/// fixedarray_003). `None` when the class overrides them.
pub(crate) fn fa_isset(vm: &mut Vm, o: &ObjRef, k: &Value, empty: bool) -> R<Option<bool>> {
    if with(o, |_: &mut FaData| ()).is_none() {
        return Ok(None);
    }
    let cls = vm.class_of(o);
    // A user offsetExists() decides; empty() then looks at the element
    // itself, without offsetGet() (test fixedarray_002).
    if cls
        .find_method(b"offsetexists")
        .is_some_and(|f| !f.is_native())
    {
        let r = vm.call_method(o, b"offsetexists", vec![k.clone()])?;
        let exists = vm.to_bool(&r);
        if !empty || !exists {
            return Ok(Some(if empty { !exists } else { exists }));
        }
    }
    let i = fa_index(vm, k)?;
    let v = fa_items(o)
        .get(usize::try_from(i).unwrap_or(usize::MAX))
        .cloned();
    Ok(Some(match v {
        None | Some(Value::Null) => empty,
        Some(v) if empty => !vm.to_bool(&v),
        Some(_) => true,
    }))
}

fn fa_offset_exists(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let i = fa_index(vm, &a.val(0))?;
    let items = fa_items(a.this_obj());
    Ok(Value::Bool(
        i >= 0 && (i as usize) < items.len() && !items[i as usize].is_null(),
    ))
}

fn fa_offset_get(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let k = a.val(0);
    if k.is_null() && a.has(0) {
        return vm.throw("Error", "[] operator not supported for SplFixedArray");
    }
    let i = fa_index(vm, &k)?;
    let v = with(a.this_obj(), |d: &mut FaData| {
        if i >= 0 && (i as usize) < d.items.len() {
            Some(d.items[i as usize].clone())
        } else {
            None
        }
    })
    .flatten();
    match v {
        Some(v) => Ok(v),
        None => vm.throw("OutOfBoundsException", "Index invalid or out of range"),
    }
}

fn fa_offset_set(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let k = a.val(0);
    if k.is_null() {
        return vm.throw("Error", "[] operator not supported for SplFixedArray");
    }
    let i = fa_index(vm, &k)?;
    let v = a.val(1);
    let old = with(a.this_obj(), |d: &mut FaData| {
        if i >= 0 && (i as usize) < d.items.len() {
            Ok(std::mem::replace(&mut d.items[i as usize], v))
        } else {
            Err(())
        }
    })
    .unwrap_or(Err(()));
    match old {
        Ok(old) => {
            drop(old);
            Ok(Value::Null)
        }
        Err(()) => vm.throw("OutOfBoundsException", "Index invalid or out of range"),
    }
}

fn fa_offset_unset(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let i = fa_index(vm, &a.val(0))?;
    let old = with(a.this_obj(), |d: &mut FaData| {
        if i >= 0 && (i as usize) < d.items.len() {
            Ok(std::mem::replace(&mut d.items[i as usize], Value::Null))
        } else {
            Err(())
        }
    })
    .unwrap_or(Err(()));
    match old {
        Ok(old) => {
            drop(old);
            Ok(Value::Null)
        }
        Err(()) => vm.throw("OutOfBoundsException", "Index invalid or out of range"),
    }
}

/// InternalIterator: iteration over an SplFixedArray.
pub struct IiData {
    target: ObjRef,
    pos: usize,
    /// Over an SplDoublyLinkedList (else an SplFixedArray).
    dll: bool,
    /// The list's iterator mode when foreach started: changing it in the
    /// loop doesn't change the direction (test
    /// SplDoublyLinkedList_consistent_iterator_mode).
    mode: i64,
}

impl goro_runtime::NativeData for IiData {
    fn trace(&self, visit: &mut dyn FnMut(&Value)) {
        visit(&Value::Object(self.target.clone()));
    }
}

fn fa_get_iterator(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let cls = vm.find_class(b"InternalIterator", false)?.unwrap();
    let o = vm.instantiate_raw(&cls)?;
    set_native(
        &o,
        IiData {
            target: a.this_obj().clone(),
            pos: 0,
            dll: false,
            mode: 0,
        },
    );
    Ok(Value::Object(o))
}

/// An InternalIterator over another extension's collection (ext/dom's
/// node lists): `step` answers valid/current/key for a position.
pub struct ExtIter {
    pub target: ObjRef,
    pub pos: usize,
    pub step: fn(&mut Vm, &ObjRef, usize, ExtIterOp) -> R<Value>,
}

/// What [`ExtIter::step`] is asked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExtIterOp {
    Valid,
    Current,
    Key,
}

impl goro_runtime::NativeData for ExtIter {
    fn trace(&self, visit: &mut dyn FnMut(&Value)) {
        visit(&Value::Object(self.target.clone()));
    }
}

/// A new InternalIterator over `target`.
pub fn new_ext_iter(
    vm: &mut Vm,
    target: &ObjRef,
    step: fn(&mut Vm, &ObjRef, usize, ExtIterOp) -> R<Value>,
) -> R<ObjRef> {
    let cls = vm.find_class(b"InternalIterator", false)?.unwrap();
    let o = vm.instantiate_raw(&cls)?;
    set_native(
        &o,
        ExtIter {
            target: target.clone(),
            pos: 0,
            step,
        },
    );
    Ok(o)
}

/// The InternalIterator methods for an [`ExtIter`] (None: not one).
fn ext_iter(vm: &mut Vm, a: &Args, op: u8) -> Option<R<Value>> {
    let o = a.this_obj();
    let (t, pos, step) = with(o, |d: &mut ExtIter| (d.target.clone(), d.pos, d.step))?;
    Some(match op {
        0 => step(vm, &t, pos, ExtIterOp::Valid),
        1 => step(vm, &t, pos, ExtIterOp::Current),
        2 => step(vm, &t, pos, ExtIterOp::Key),
        3 => {
            with(o, |d: &mut ExtIter| d.pos += 1);
            Ok(Value::Null)
        }
        _ => {
            if pos > 0 {
                return Some(vm.throw("Error", "Iterator does not support rewinding"));
            }
            Ok(Value::Null)
        }
    })
}

fn ii(o: &ObjRef) -> Option<(ObjRef, usize, bool)> {
    with(o, |d: &mut IiData| (d.target.clone(), d.pos, d.dll))
}

/// (mode flags, length) of the linked list an internal iterator walks.
fn ii_dll(it: &ObjRef, t: &ObjRef) -> (i64, usize) {
    let mode = with(it, |d: &mut IiData| d.mode).unwrap_or(0);
    (mode, with(t, |d: &mut DllData| d.items.len()).unwrap_or(0))
}

fn ii_valid(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if let Some(r) = ext_iter(vm, a, 0) {
        return r;
    }
    let _vm = vm;
    let Some((t, p, dll)) = ii(a.this_obj()) else {
        return Ok(Value::Bool(false));
    };
    if dll {
        return Ok(Value::Bool(p < ii_dll(a.this_obj(), &t).1));
    }
    Ok(Value::Bool(p < fa_items(&t).len()))
}
fn ii_current(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if let Some(r) = ext_iter(vm, a, 1) {
        return r;
    }
    let _vm = vm;
    let Some((t, p, dll)) = ii(a.this_obj()) else {
        return Ok(Value::Null);
    };
    if dll {
        return Ok(with(&t, |d: &mut DllData| d.items.get(p).cloned())
            .flatten()
            .unwrap_or_default());
    }
    Ok(fa_items(&t).get(p).cloned().unwrap_or_default())
}
fn ii_key(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if let Some(r) = ext_iter(vm, a, 2) {
        return r;
    }
    let _vm = vm;
    let Some((t, p, dll)) = ii(a.this_obj()) else {
        return Ok(Value::Null);
    };
    if dll {
        return Ok(Value::Int(if p == usize::MAX { -1 } else { p as i64 }));
    }
    if p < fa_items(&t).len() {
        return Ok(Value::Int(p as i64));
    }
    Ok(Value::Null)
}
fn ii_next(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if let Some(r) = ext_iter(vm, a, 3) {
        return r;
    }
    let _vm = vm;
    let Some((t, p, dll)) = ii(a.this_obj()) else {
        return Ok(Value::Null);
    };
    if dll {
        let (flags, len) = ii_dll(a.this_obj(), &t);
        let lifo = flags & 2 != 0;
        if flags & 1 != 0 {
            // IT_MODE_DELETE: the current element goes away.
            let old = with(&t, |d: &mut DllData| {
                if lifo {
                    d.items.pop_back()
                } else {
                    d.items.pop_front()
                }
            });
            drop(old);
            let np = if lifo {
                len.saturating_sub(1).wrapping_sub(1)
            } else {
                0
            };
            with(a.this_obj(), |d: &mut IiData| d.pos = np);
        } else {
            let np = if lifo { p.wrapping_sub(1) } else { p + 1 };
            with(a.this_obj(), |d: &mut IiData| d.pos = np);
        }
        return Ok(Value::Null);
    }
    with(a.this_obj(), |d: &mut IiData| d.pos += 1);
    Ok(Value::Null)
}
fn ii_rewind(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if let Some(r) = ext_iter(vm, a, 4) {
        return r;
    }
    let _vm = vm;
    let Some((t, _, dll)) = ii(a.this_obj()) else {
        return Ok(Value::Null);
    };
    let p = if dll {
        let (flags, len) = ii_dll(a.this_obj(), &t);
        if flags & 2 != 0 {
            len.wrapping_sub(1)
        } else {
            0
        }
    } else {
        0
    };
    with(a.this_obj(), |d: &mut IiData| d.pos = p);
    Ok(Value::Null)
}

// ================================================================ SplDoublyLinkedList

pub struct DllData {
    items: VecDeque<Value>,
    flags: i64,
    /// Iteration index (-1 or len: invalid).
    pos: i64,
}

impl goro_runtime::NativeData for DllData {
    fn trace(&self, visit: &mut dyn FnMut(&Value)) {
        for v in &self.items {
            visit(v);
        }
    }
    fn clear(&mut self) {
        self.items.clear();
    }
}

/// Mode bit of SplQueue/SplStack: the LIFO/FIFO direction is fixed.
const DLL_FIX: i64 = 4;

fn dll_hooks() -> NativeHooks {
    NativeHooks {
        create: Some(|vm, o| {
            let cls = vm.class_of(o);
            let flags = if is_subclass_named(&cls, b"splstack") {
                2 | DLL_FIX
            } else if is_subclass_named(&cls, b"splqueue") {
                DLL_FIX
            } else {
                0
            };
            set_native(
                o,
                DllData {
                    items: VecDeque::new(),
                    flags,
                    pos: 0,
                },
            )
        }),
        clone: Some(|_, from, to| {
            let d = with(from, |d: &mut DllData| DllData {
                items: d.items.clone(),
                flags: d.flags,
                pos: d.pos,
            });
            if let Some(d) = d {
                set_native(to, d);
            }
        }),
        count: Some(|_, o| Ok(with(o, |d: &mut DllData| d.items.len() as i64).unwrap_or(0))),
        get_iterator: Some(|vm, o, by_ref| {
            if by_ref {
                return vm.throw(
                    "Error",
                    "An iterator cannot be used with foreach by reference",
                );
            }
            let cls = vm.find_class(b"InternalIterator", false)?.unwrap();
            let it = vm.instantiate_raw(&cls)?;
            set_native(
                &it,
                IiData {
                    target: o.clone(),
                    pos: 0,
                    dll: true,
                    mode: with(o, |d: &mut DllData| d.flags).unwrap_or(0),
                },
            );
            Ok(it)
        }),
        ..Default::default()
    }
}

fn dll<X>(a: &Args, f: impl FnOnce(&mut DllData) -> X) -> X {
    with(a.this_obj(), f).expect("SplDoublyLinkedList data")
}

fn dll_index(vm: &mut Vm, a: &Args, v: &Value, method: &str) -> R<i64> {
    match v {
        Value::Int(i) => Ok(*i),
        Value::Str(s) => match goro_runtime::numeric::numeric_value(s) {
            Some(goro_runtime::numeric::Number::Int(i)) => Ok(i),
            Some(goro_runtime::numeric::Number::Float(f)) => Ok(f as i64),
            None => vm.type_error(&format!(
                "SplDoublyLinkedList::{}(): Argument #1 ($index) must be of type int, string given",
                method
            )),
        },
        Value::Bool(b) => Ok(*b as i64),
        Value::Float(f) => Ok(*f as i64),
        Value::Null => Ok(0),
        other => {
            let _ = a;
            vm.type_error(&format!(
                "SplDoublyLinkedList::{}(): Argument #1 ($index) must be of type int, {} given",
                method,
                crate::vm::Vm::type_name_given(other)
            ))
        }
    }
}

fn out_of_range<T>(vm: &mut Vm, method: &str) -> R<T> {
    vm.throw(
        "OutOfRangeException",
        &format!(
            "SplDoublyLinkedList::{}(): Argument #1 ($index) is out of range",
            method
        ),
    )
}

fn dll_add(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let i = dll_index(vm, a, &a.val(0), "add")?;
    let v = a.val(1);
    let len = dll(a, |d| d.items.len() as i64);
    if i < 0 || i > len {
        return out_of_range(vm, "add");
    }
    dll(a, |d| d.items.insert(i as usize, v));
    Ok(Value::Null)
}
fn dll_pop(vm: &mut Vm, a: &mut Args) -> R<Value> {
    match dll(a, |d| d.items.pop_back()) {
        Some(v) => Ok(v),
        None => vm.throw("RuntimeException", "Can't pop from an empty datastructure"),
    }
}
fn dll_shift(vm: &mut Vm, a: &mut Args) -> R<Value> {
    match dll(a, |d| d.items.pop_front()) {
        Some(v) => Ok(v),
        None => vm.throw(
            "RuntimeException",
            "Can't shift from an empty datastructure",
        ),
    }
}
fn dll_push(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let v = a.val(0);
    dll(a, |d| d.items.push_back(v));
    Ok(Value::Null)
}
fn dll_unshift(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let v = a.val(0);
    dll(a, |d| d.items.push_front(v));
    Ok(Value::Null)
}
fn dll_top(vm: &mut Vm, a: &mut Args) -> R<Value> {
    match dll(a, |d| d.items.back().cloned()) {
        Some(v) => Ok(v),
        None => vm.throw("RuntimeException", "Can't peek at an empty datastructure"),
    }
}
fn dll_bottom(vm: &mut Vm, a: &mut Args) -> R<Value> {
    match dll(a, |d| d.items.front().cloned()) {
        Some(v) => Ok(v),
        None => vm.throw("RuntimeException", "Can't peek at an empty datastructure"),
    }
}
fn dll_debug_info(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let mut out = members(vm, &o);
    let (flags, items) = dll(a, |d| (d.flags, d.items.clone()));
    out.insert(
        private_key("SplDoublyLinkedList", "flags"),
        Value::Int(flags),
    );
    out.insert(
        private_key("SplDoublyLinkedList", "dllist"),
        Value::Array(PhpArray::from_values(items)),
    );
    Ok(Value::Array(out))
}
fn dll_count(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Int(dll(a, |d| d.items.len() as i64)))
}
fn dll_is_empty(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Bool(dll(a, |d| d.items.is_empty())))
}
fn dll_set_mode(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let m = a.int(0);
    let flags = dll(a, |d| d.flags);
    if flags & DLL_FIX != 0 && (flags & 2) != (m & 2) {
        return vm.throw(
            "RuntimeException",
            "Iterators' LIFO/FIFO modes for SplStack/SplQueue objects are frozen",
        );
    }
    let nf = (m & 3) | (flags & DLL_FIX);
    dll(a, |d| d.flags = nf);
    Ok(Value::Int(nf))
}
fn dll_get_mode(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Int(dll(a, |d| d.flags)))
}
fn dll_offset_exists(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let i = dll_index(vm, a, &a.val(0), "offsetExists")?;
    Ok(Value::Bool(dll(a, |d| {
        i >= 0 && (i as usize) < d.items.len()
    })))
}
fn dll_offset_get(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let i = dll_index(vm, a, &a.val(0), "offsetGet")?;
    match dll(a, |d| {
        if i >= 0 {
            d.items.get(i as usize).cloned()
        } else {
            None
        }
    }) {
        Some(v) => Ok(v),
        None => out_of_range(vm, "offsetGet"),
    }
}
fn dll_offset_set(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let k = a.val(0);
    let v = a.val(1);
    if k.is_null() {
        dll(a, |d| d.items.push_back(v));
        return Ok(Value::Null);
    }
    let i = dll_index(vm, a, &k, "offsetSet")?;
    let r = dll(a, |d| {
        if i >= 0 && (i as usize) < d.items.len() {
            Some(std::mem::replace(&mut d.items[i as usize], v))
        } else {
            None
        }
    });
    match r {
        Some(old) => {
            drop(old);
            Ok(Value::Null)
        }
        None => out_of_range(vm, "offsetSet"),
    }
}
fn dll_offset_unset(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let i = dll_index(vm, a, &a.val(0), "offsetUnset")?;
    let r = dll(a, |d| {
        if i >= 0 {
            d.items.remove(i as usize)
        } else {
            None
        }
    });
    match r {
        Some(old) => {
            drop(old);
            Ok(Value::Null)
        }
        None => out_of_range(vm, "offsetUnset"),
    }
}
fn dll_rewind(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    dll(a, |d| {
        d.pos = if d.flags & 2 != 0 {
            d.items.len() as i64 - 1
        } else {
            0
        }
    });
    Ok(Value::Null)
}
fn dll_valid(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Bool(dll(a, |d| {
        d.pos >= 0 && (d.pos as usize) < d.items.len()
    })))
}
fn dll_current(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(dll(a, |d| {
        if d.pos >= 0 {
            d.items.get(d.pos as usize).cloned()
        } else {
            None
        }
    })
    .unwrap_or_default())
}
fn dll_key(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Int(dll(a, |d| d.pos)))
}
fn dll_next(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let old = dll(a, |d| {
        let lifo = d.flags & 2 != 0;
        if d.flags & 1 != 0 {
            // IT_MODE_DELETE: the current element goes away.
            if d.pos >= 0 && (d.pos as usize) < d.items.len() {
                let v = d.items.remove(d.pos as usize);
                if lifo {
                    d.pos = d.items.len() as i64 - 1;
                }
                return v;
            }
            return None;
        }
        d.pos += if lifo { -1 } else { 1 };
        None
    });
    drop(old);
    Ok(Value::Null)
}
fn dll_prev(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    dll(a, |d| d.pos += if d.flags & 2 != 0 { 1 } else { -1 });
    Ok(Value::Null)
}
fn dll_serialize_str(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let flags = dll(a, |d| d.flags);
    vm.with_ser_ctx(|vm| {
        let mut out = vm.serialize_value(&Value::Int(flags))?;
        // The live list: serializing an element may change it (test
        // gh16589).
        let mut i = 0;
        while let Some(v) = dll(a, |d| d.items.get(i).cloned()) {
            i += 1;
            out.push(b':');
            out.extend_from_slice(&vm.serialize_value(&v)?);
        }
        Ok(Value::Str(PhpStr::from(out)))
    })
}
fn dll_unserialize_str(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.str(0);
    let b = s.as_bytes();
    if b.is_empty() {
        return Ok(Value::Null);
    }
    // The flags, then `:` and a value for each element, read in the value
    // table of the unserialize() in progress (test bug70169; error offsets
    // observed on PHP 8.5).
    let parsed = vm.with_unser_ctx(|vm| -> R<Result<(i64, VecDeque<Value>), usize>> {
        let (flags, mut cur) = match vm.unser_part(b, "SplDoublyLinkedList::unserialize()")? {
            Ok((Value::Int(f), n)) => (f, n),
            Ok((_, n)) => return Ok(Err(n)),
            Err(_) => return Ok(Err(0)),
        };
        let mut items = VecDeque::new();
        while cur < b.len() {
            if b[cur] != b':' {
                return Ok(Err(cur));
            }
            cur += 1;
            match vm.unser_part(&b[cur..], "SplDoublyLinkedList::unserialize()")? {
                Ok((v, n)) => {
                    items.push_back(v);
                    cur += n;
                }
                Err(off) => return Ok(Err(cur + off)),
            }
        }
        Ok(Ok((flags, items)))
    })?;
    let (flags, items) = match parsed {
        Ok(x) => x,
        Err(at) => {
            return vm.throw(
                "UnexpectedValueException",
                &format!("Error at offset {} of {} bytes", at, b.len()),
            );
        }
    };
    dll(a, |d| {
        d.flags = flags;
        d.items = items;
    });
    Ok(Value::Null)
}
fn dll_serialize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let (flags, items) = dll(a, |d| (d.flags, d.items.clone()));
    let mut out = PhpArray::new();
    let _ = out.push(Value::Int(flags));
    let _ = out.push(Value::Array(PhpArray::from_values(items)));
    let _ = out.push(Value::Array(members(vm, &o)));
    Ok(Value::Array(out))
}
fn dll_unserialize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let data = a.array(0);
    let f = data.get_int(0).map(|v| v.deref());
    let it = data.get_int(1).map(|v| v.deref());
    let m = data.get_int(2).map(|v| v.deref());
    let (Some(Value::Int(f)), Some(Value::Array(it)), Some(Value::Array(m))) = (f, it, m) else {
        return vm.throw(
            "UnexpectedValueException",
            "Incomplete or ill-typed serialization data",
        );
    };
    dll(a, |d| {
        d.flags = f;
        d.items = it.iter().map(|(_, v)| v.deref()).collect();
    });
    restore_members(vm, &o, &m)?;
    Ok(Value::Null)
}

// ================================================================ heaps

pub struct HeapData {
    /// Elements; for SplPriorityQueue `[data, priority]` pairs.
    heap: Vec<Value>,
    flags: i64,
    corrupted: bool,
    pq: bool,
    /// The heap is being modified (a user compare() is running).
    busy: bool,
}

impl goro_runtime::NativeData for HeapData {
    fn trace(&self, visit: &mut dyn FnMut(&Value)) {
        for v in &self.heap {
            visit(v);
        }
    }
    fn clear(&mut self) {
        self.heap.clear();
    }
}

fn heap_hooks() -> NativeHooks {
    NativeHooks {
        create: Some(|vm, o| {
            let pq = is_subclass_named(&vm.class_of(o), b"splpriorityqueue");
            set_native(
                o,
                HeapData {
                    heap: Vec::new(),
                    flags: if pq { 1 } else { 0 },
                    corrupted: false,
                    pq,
                    busy: false,
                },
            )
        }),
        clone: Some(|_, from, to| {
            let d = with(from, |d: &mut HeapData| HeapData {
                heap: d.heap.clone(),
                flags: d.flags,
                corrupted: d.corrupted,
                pq: d.pq,
                busy: false,
            });
            if let Some(d) = d {
                set_native(to, d);
            }
        }),
        count: Some(|_, o| Ok(with(o, |d: &mut HeapData| d.heap.len() as i64).unwrap_or(0))),
        ..Default::default()
    }
}

fn heap<X>(o: &ObjRef, f: impl FnOnce(&mut HeapData) -> X) -> X {
    with(o, f).expect("SplHeap data")
}

fn php_cmp(vm: &mut Vm, x: &Value, y: &Value) -> R<i64> {
    goro_runtime::compare::compare(x, y, vm)
        .map(|c| c as i64)
        .map_err(|e| vm.php_error(e))
}

/// Whether `x` goes above `y` (positive), per the class's compare().
fn heap_cmp(
    vm: &mut Vm,
    o: &ObjRef,
    user: &Option<Rc<crate::func::Func>>,
    pq: bool,
    x: &Value,
    y: &Value,
) -> R<i64> {
    let (x, y) = if pq {
        (pq_priority(x), pq_priority(y))
    } else {
        (x.clone(), y.clone())
    };
    match user {
        Some(f) => {
            let cls = vm.class_of(o);
            let c = Callee {
                func: f.clone(),
                this: Some(o.clone()),
                static_class: Rc::as_ptr(&cls),
                scope: f.scope.get(),
                closure: None,
                magic_name: None,
            };
            let r = vm.call_callee(c, vec![x, y])?;
            vm.to_int(&r)
        }
        None => {
            let min = !pq && is_subclass_named(&vm.class_of(o), b"splminheap");
            if min {
                php_cmp(vm, &y, &x)
            } else {
                php_cmp(vm, &x, &y)
            }
        }
    }
}

fn pq_priority(v: &Value) -> Value {
    match v {
        Value::Array(a) => a.get_int(1).cloned().unwrap_or_default(),
        _ => Value::Null,
    }
}

fn heap_check(vm: &mut Vm, o: &ObjRef) -> R<()> {
    let (c, busy) = heap(o, |d| (d.corrupted, d.busy));
    if busy {
        return vm.throw(
            "RuntimeException",
            "Heap cannot be changed when it is already being modified.",
        );
    }
    if c {
        return vm.throw(
            "RuntimeException",
            "Heap is corrupted, heap properties are no longer ensured.",
        );
    }
    Ok(())
}

fn compare_override(vm: &Vm, o: &ObjRef) -> Option<Rc<crate::func::Func>> {
    user_override(vm, o, b"compare")
}

/// Runs a heap operation that compares elements, marking the heap
/// corrupted when compare() throws.
/// The elements stay in place during the operation: compare() may look
/// at the heap (top(), count(); test gh16337).
fn heap_op<X>(vm: &mut Vm, o: &ObjRef, f: impl FnOnce(&mut Vm) -> R<X>) -> R<X> {
    heap_check(vm, o)?;
    heap(o, |d| d.busy = true);
    let r = f(vm);
    heap(o, |d| {
        d.busy = false;
        if r.is_err() {
            d.corrupted = true;
        }
    });
    r
}

fn heap_at(o: &ObjRef, i: usize) -> Value {
    heap(o, |d| d.heap.get(i).cloned()).unwrap_or_default()
}

fn heap_swap(o: &ObjRef, i: usize, j: usize) {
    heap(o, |d| {
        if i < d.heap.len() && j < d.heap.len() {
            d.heap.swap(i, j)
        }
    })
}

fn sift_up(vm: &mut Vm, o: &ObjRef, mut i: usize) -> R<()> {
    let user = compare_override(vm, o);
    let pq = heap(o, |d| d.pq);
    while i > 0 {
        let parent = (i - 1) / 2;
        let (x, y) = (heap_at(o, parent), heap_at(o, i));
        if heap_cmp(vm, o, &user, pq, &x, &y)? < 0 {
            heap_swap(o, parent, i);
            i = parent;
        } else {
            break;
        }
    }
    Ok(())
}

fn delete_top(vm: &mut Vm, o: &ObjRef) -> R<Value> {
    let user = compare_override(vm, o);
    let pq = heap(o, |d| d.pq);
    let top = heap(o, |d| d.heap.swap_remove(0));
    let n = heap(o, |d| d.heap.len());
    if n > 1 {
        let mut i = 0;
        loop {
            let mut j = 2 * i + 1;
            if j >= n {
                break;
            }
            if j + 1 < n {
                let (x, y) = (heap_at(o, j + 1), heap_at(o, j));
                if heap_cmp(vm, o, &user, pq, &x, &y)? > 0 {
                    j += 1;
                }
            }
            let (x, y) = (heap_at(o, i), heap_at(o, j));
            if heap_cmp(vm, o, &user, pq, &x, &y)? < 0 {
                heap_swap(o, i, j);
                i = j;
            } else {
                break;
            }
        }
    }
    Ok(top)
}

fn heap_insert(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let v = a.val(0);
    heap_op(vm, &o, |vm| {
        let n = heap(&o, |d| {
            d.heap.push(v);
            d.heap.len() - 1
        });
        sift_up(vm, &o, n)
    })?;
    Ok(Value::Bool(true))
}

fn pq_insert(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let v = Value::Array(PhpArray::from_values([a.val(0), a.val(1)]));
    heap_op(vm, &o, |vm| {
        let n = heap(&o, |d| {
            d.heap.push(v);
            d.heap.len() - 1
        });
        sift_up(vm, &o, n)
    })?;
    Ok(Value::Bool(true))
}

/// An element as returned to PHP (SplPriorityQueue: per extract flags).
fn heap_out(o: &ObjRef, v: Value) -> Value {
    let (pq, flags) = heap(o, |d| (d.pq, d.flags));
    if !pq {
        return v;
    }
    let Value::Array(a) = &v else { return v };
    let data = a.get_int(0).cloned().unwrap_or_default();
    let prio = a.get_int(1).cloned().unwrap_or_default();
    match flags & 3 {
        1 => data,
        2 => prio,
        _ => {
            let mut out = PhpArray::new();
            out.insert(Key::from("data"), data);
            out.insert(Key::from("priority"), prio);
            Value::Array(out)
        }
    }
}

fn heap_extract(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    heap_check(vm, &o)?;
    if heap(&o, |d| d.heap.is_empty()) {
        return vm.throw("RuntimeException", "Can't extract from an empty heap");
    }
    let v = heap_op(vm, &o, |vm| delete_top(vm, &o))?;
    Ok(heap_out(&o, v))
}

fn heap_top(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    // Reading is fine while the heap is being modified (test gh16337).
    if heap(&o, |d| d.corrupted) {
        return vm.throw(
            "RuntimeException",
            "Heap is corrupted, heap properties are no longer ensured.",
        );
    }
    match heap(&o, |d| d.heap.first().cloned()) {
        Some(v) => Ok(heap_out(&o, v)),
        None => vm.throw("RuntimeException", "Can't peek at an empty heap"),
    }
}

fn heap_count(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Int(heap(a.this_obj(), |d| d.heap.len() as i64)))
}
fn heap_is_empty(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Bool(heap(a.this_obj(), |d| d.heap.is_empty())))
}
fn heap_current(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    match heap(&o, |d| d.heap.first().cloned()) {
        Some(v) => Ok(heap_out(&o, v)),
        None => Ok(Value::Null),
    }
}
fn heap_key(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Int(heap(a.this_obj(), |d| d.heap.len() as i64 - 1)))
}
fn heap_next(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    if heap(&o, |d| d.heap.is_empty()) {
        return Ok(Value::Null);
    }
    let v = heap_op(vm, &o, |vm| delete_top(vm, &o))?;
    drop(v);
    Ok(Value::Null)
}
fn heap_valid(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Bool(heap(a.this_obj(), |d| !d.heap.is_empty())))
}
fn heap_recover(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    heap(a.this_obj(), |d| d.corrupted = false);
    Ok(Value::Bool(true))
}
fn heap_is_corrupted(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Bool(heap(a.this_obj(), |d| d.corrupted)))
}
fn heap_elements(o: &ObjRef) -> Vec<Value> {
    let (pq, h) = heap(o, |d| (d.pq, d.heap.clone()));
    h.into_iter()
        .map(|v| {
            if pq && let Value::Array(a) = &v {
                let mut out = PhpArray::new();
                out.insert(Key::from("data"), a.get_int(0).cloned().unwrap_or_default());
                out.insert(
                    Key::from("priority"),
                    a.get_int(1).cloned().unwrap_or_default(),
                );
                return Value::Array(out);
            }
            v
        })
        .collect()
}
fn heap_debug_info(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let mut out = members(vm, &o);
    let (pq, flags, c) = heap(&o, |d| (d.pq, d.flags, d.corrupted));
    let cn = if pq { "SplPriorityQueue" } else { "SplHeap" };
    out.insert(private_key(cn, "flags"), Value::Int(flags));
    out.insert(private_key(cn, "isCorrupted"), Value::Bool(c));
    out.insert(
        private_key(cn, "heap"),
        Value::Array(PhpArray::from_values(heap_elements(&o))),
    );
    Ok(Value::Array(out))
}
fn heap_serialize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    // test SplHeap_serialize_corrupted
    if heap(&o, |d| d.corrupted) {
        return vm.throw(
            "RuntimeException",
            "Heap is corrupted, heap properties are no longer ensured.",
        );
    }
    let flags = heap(&o, |d| d.flags);
    let mut out = PhpArray::new();
    let _ = out.push(Value::Array(members(vm, &o)));
    let mut st = PhpArray::new();
    st.insert(Key::from("flags"), Value::Int(flags));
    st.insert(
        Key::from("heap_elements"),
        Value::Array(PhpArray::from_values(heap_elements(&o))),
    );
    let _ = out.push(Value::Array(st));
    Ok(Value::Array(out))
}
fn heap_unserialize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    heap_check(vm, &o)?;
    let data = a.array(0);
    let pq = heap(&o, |d| d.pq);
    // Malformed data is refused as a whole (tests
    // SplHeap_serialize_error_handling,
    // SplPriorityQueue_unserialize_invalid_flags).
    let parsed = (|| {
        if data.len() != 2 {
            return None;
        }
        let (Some(Value::Array(m)), Some(Value::Array(st))) = (
            data.get_int(0).map(|v| v.deref()),
            data.get_int(1).map(|v| v.deref()),
        ) else {
            return None;
        };
        let Some(Value::Int(mut flags)) = st.get_str(&PhpStr::from("flags")).map(|v| v.deref())
        else {
            return None;
        };
        if pq {
            flags &= 3;
            if flags == 0 {
                return None;
            }
        }
        let Some(Value::Array(h)) = st
            .get_str(&PhpStr::from("heap_elements"))
            .map(|v| v.deref())
        else {
            return None;
        };
        let mut elems = Vec::new();
        for (_, v) in h.iter() {
            let v = v.deref();
            if pq {
                let Value::Array(p) = &v else { return None };
                let d = p.get_str(&PhpStr::from("data"))?.deref();
                let pr = p.get_str(&PhpStr::from("priority"))?.deref();
                elems.push(Value::Array(PhpArray::from_values([d, pr])));
            } else {
                elems.push(v);
            }
        }
        Some((m, flags, elems))
    })();
    let Some((m, flags, elems)) = parsed else {
        let cls = vm.class_of(&o);
        return vm.throw(
            "UnexpectedValueException",
            &format!(
                "Invalid serialization data for {} object",
                cls.name.to_string_lossy()
            ),
        );
    };
    restore_members(vm, &o, &m)?;
    heap(&o, |d| {
        d.flags = flags;
        d.heap = elems;
    });
    Ok(Value::Null)
}
fn minheap_compare(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (x, y) = (a.val(0), a.val(1));
    Ok(Value::Int(php_cmp(vm, &y, &x)?))
}
fn maxheap_compare(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (x, y) = (a.val(0), a.val(1));
    Ok(Value::Int(php_cmp(vm, &x, &y)?))
}
fn pq_compare(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (x, y) = (a.val(0), a.val(1));
    Ok(Value::Int(php_cmp(vm, &x, &y)?))
}
fn pq_set_flags(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.int(0) & 3;
    if f == 0 {
        return vm.throw("RuntimeException", "Must specify at least one extract flag");
    }
    heap(a.this_obj(), |d| d.flags = f);
    Ok(Value::Int(f))
}
fn pq_get_flags(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Int(heap(a.this_obj(), |d| d.flags)))
}

// ================================================================ SplObjectStorage

#[derive(Clone, PartialEq, Eq, Hash)]
enum OsKey {
    Handle(u32),
    Hash(PhpStr),
}

pub struct OsData {
    entries: Vec<Option<(ObjRef, Value)>>,
    index: HashMap<OsKey, usize>,
    live: usize,
    /// Iteration: index into entries, and the ordinal (key()).
    pos: usize,
    ord: i64,
    /// MultipleIterator flags.
    flags: i64,
}

impl goro_runtime::NativeData for OsData {
    fn trace(&self, visit: &mut dyn FnMut(&Value)) {
        for (o, v) in self.entries.iter().flatten() {
            visit(&Value::Object(o.clone()));
            visit(v);
        }
    }
    fn clear(&mut self) {
        self.entries.clear();
        self.index.clear();
        self.live = 0;
    }
}

impl OsData {
    fn new() -> OsData {
        OsData {
            entries: Vec::new(),
            index: HashMap::new(),
            live: 0,
            pos: 0,
            ord: 0,
            flags: 1,
        }
    }
    fn live_entries(&self) -> Vec<(ObjRef, Value)> {
        self.entries.iter().flatten().cloned().collect()
    }
    fn compact(&mut self) {
        if self.entries.len() > 16 && self.live * 2 < self.entries.len() {
            let cur = self.pos;
            let mut new_pos = 0;
            let mut out = Vec::with_capacity(self.live);
            for (i, e) in self.entries.drain(..).enumerate() {
                if i == cur {
                    new_pos = out.len();
                }
                if e.is_some() {
                    out.push(e);
                }
            }
            if cur > out.len() {
                new_pos = out.len();
            }
            self.entries = out;
            self.pos = new_pos;
            self.index.clear();
            // Keys are rebuilt by the caller (needs the VM for getHash).
        }
    }
}

fn os_hooks() -> NativeHooks {
    NativeHooks {
        create: Some(|vm, o| {
            let mut d = OsData::new();
            if !is_subclass_named(&vm.class_of(o), b"multipleiterator") {
                d.flags = 0;
            }
            set_native(o, d)
        }),
        clone: Some(|_, from, to| {
            let d = with(from, |d: &mut OsData| OsData {
                entries: d.entries.clone(),
                index: d.index.clone(),
                live: d.live,
                pos: d.pos,
                ord: d.ord,
                flags: d.flags,
            });
            if let Some(d) = d {
                set_native(to, d);
            }
        }),
        count: Some(|_, o| Ok(with(o, |d: &mut OsData| d.live as i64).unwrap_or(0))),
        compare: Some(os_compare),
        ..Default::default()
    }
}

/// `==` of two storages: the same objects with equal data, then the
/// properties (test SplObjectStorage/observer_007).
fn os_compare(vm: &mut Vm, x: &Value, y: &Value) -> Option<R<i32>> {
    let (Value::Object(a), Value::Object(b)) = (x, y) else {
        return None;
    };
    let ea = with(a, |d: &mut OsData| d.live_entries())?;
    let eb = with(b, |d: &mut OsData| d.live_entries())?;
    if ea.len() != eb.len() {
        return Some(Ok(1));
    }
    for (o, inf) in &ea {
        let Some((_, other)) = eb.iter().find(|(p, _)| ObjRef::ptr_eq(p, o)) else {
            return Some(Ok(1));
        };
        match php_cmp(vm, inf, other) {
            Ok(0) => {}
            Ok(c) => return Some(Ok(c as i32)),
            Err(e) => return Some(Err(e)),
        }
    }
    None
}

fn os<X>(o: &ObjRef, f: impl FnOnce(&mut OsData) -> X) -> X {
    with(o, f).expect("SplObjectStorage data")
}

/// The key of `obj` in storage `s` (user getHash() when overridden).
fn os_key(vm: &mut Vm, s: &ObjRef, obj: &ObjRef) -> R<OsKey> {
    if let Some(f) = user_override(vm, s, b"gethash") {
        let cls = vm.class_of(s);
        let c = Callee {
            func: f.clone(),
            this: Some(s.clone()),
            static_class: Rc::as_ptr(&cls),
            scope: f.scope.get(),
            closure: None,
            magic_name: None,
        };
        let r = vm.call_callee(c, vec![Value::Object(obj.clone())])?;
        return match r {
            Value::Str(h) => Ok(OsKey::Hash(h)),
            // test SplObjectStorage_getHash
            other => vm.type_error(&format!(
                "{}::getHash(): Return value must be of type string, {} returned",
                cls.name.dname(),
                crate::class::TypeName::tname(&other)
            )),
        };
    }
    Ok(OsKey::Handle(obj.handle()))
}

fn os_reindex(vm: &mut Vm, s: &ObjRef) -> R<()> {
    let entries: Vec<(usize, ObjRef)> = os(s, |d| {
        d.entries
            .iter()
            .enumerate()
            .filter_map(|(i, e)| e.as_ref().map(|(o, _)| (i, o.clone())))
            .collect()
    });
    let mut idx = HashMap::new();
    for (i, o) in entries {
        idx.insert(os_key(vm, s, &o)?, i);
    }
    os(s, |d| d.index = idx);
    Ok(())
}

fn os_put(vm: &mut Vm, s: &ObjRef, obj: ObjRef, inf: Value) -> R<()> {
    let k = os_key(vm, s, &obj)?;
    let old = os(s, |d| match d.index.get(&k) {
        Some(&i) => d.entries[i]
            .as_mut()
            .map(|e| std::mem::replace(&mut e.1, inf)),
        None => {
            d.entries.push(Some((obj, inf)));
            d.index.insert(k, d.entries.len() - 1);
            d.live += 1;
            None
        }
    });
    drop(old);
    Ok(())
}

fn os_remove(vm: &mut Vm, s: &ObjRef, obj: &ObjRef) -> R<bool> {
    let k = os_key(vm, s, obj)?;
    let (removed, needs_compact) = os(s, |d| match d.index.remove(&k) {
        Some(i) => {
            let e = d.entries[i].take();
            d.live -= 1;
            (e, d.entries.len() > 16 && d.live * 2 < d.entries.len())
        }
        None => (None, false),
    });
    let found = removed.is_some();
    drop(removed);
    if needs_compact {
        os(s, |d| d.compact());
        os_reindex(vm, s)?;
    }
    Ok(found)
}

fn os_find(vm: &mut Vm, s: &ObjRef, obj: &ObjRef) -> R<Option<Value>> {
    let k = os_key(vm, s, obj)?;
    Ok(os(s, |d| {
        d.index
            .get(&k)
            .and_then(|&i| d.entries[i].as_ref().map(|e| e.1.clone()))
    }))
}

fn os_deprecated(vm: &mut Vm, m: &str, instead: &str) -> R<()> {
    vm.deprecated(&format!(
        "Method SplObjectStorage::{}() is deprecated since 8.5, use method SplObjectStorage::{}() instead",
        m, instead
    ))
}

fn obj_arg(vm: &mut Vm, a: &Args, i: usize, method: &str) -> R<ObjRef> {
    match a.val(i) {
        Value::Object(o) => Ok(o),
        other => vm.type_error(&format!(
            "SplObjectStorage::{}(): Argument #{} ($object) must be of type object, {} given",
            method,
            i + 1,
            crate::vm::Vm::type_name_given(&other)
        )),
    }
}

fn os_attach(vm: &mut Vm, a: &mut Args) -> R<Value> {
    os_deprecated(vm, "attach", "offsetSet")?;
    let s = a.this_obj().clone();
    let o = obj_arg(vm, a, 0, "attach")?;
    let inf = if a.has(1) { a.val(1) } else { Value::Null };
    os_put(vm, &s, o, inf)?;
    Ok(Value::Null)
}
fn os_detach(vm: &mut Vm, a: &mut Args) -> R<Value> {
    os_deprecated(vm, "detach", "offsetUnset")?;
    let s = a.this_obj().clone();
    let o = obj_arg(vm, a, 0, "detach")?;
    os_remove(vm, &s, &o)?;
    Ok(Value::Null)
}
fn os_contains(vm: &mut Vm, a: &mut Args) -> R<Value> {
    os_deprecated(vm, "contains", "offsetExists")?;
    let s = a.this_obj().clone();
    let o = obj_arg(vm, a, 0, "contains")?;
    Ok(Value::Bool(os_find(vm, &s, &o)?.is_some()))
}
fn other_storage(vm: &mut Vm, a: &Args, method: &str) -> R<ObjRef> {
    match a.obj(0) {
        Some(o) if with(&o, |_: &mut OsData| ()).is_some() => Ok(o),
        _ => vm.type_error(&format!(
            "SplObjectStorage::{}(): Argument #1 ($storage) must be of type SplObjectStorage, {} given",
            method,
            crate::vm::Vm::type_name_given(&a.val(0))
        )),
    }
}
fn os_add_all(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.this_obj().clone();
    let other = other_storage(vm, a, "addAll")?;
    for (o, inf) in os(&other, |d| d.live_entries()) {
        os_put(vm, &s, o, inf)?;
    }
    Ok(Value::Int(os(&s, |d| d.live as i64)))
}
fn os_remove_all(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.this_obj().clone();
    let other = other_storage(vm, a, "removeAll")?;
    for (o, _) in os(&other, |d| d.live_entries()) {
        os_remove(vm, &s, &o)?;
    }
    Ok(Value::Int(os(&s, |d| d.live as i64)))
}
fn os_remove_all_except(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.this_obj().clone();
    let other = other_storage(vm, a, "removeAllExcept")?;
    for (o, _) in os(&s, |d| d.live_entries()) {
        if os_find(vm, &other, &o)?.is_none() {
            os_remove(vm, &s, &o)?;
        }
    }
    Ok(Value::Int(os(&s, |d| d.live as i64)))
}
/// Moves the iteration index to the next live entry at or after it.
fn os_settle(d: &mut OsData) {
    while d.pos < d.entries.len() && d.entries[d.pos].is_none() {
        d.pos += 1;
    }
}
fn os_get_info(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(os(a.this_obj(), |d| {
        os_settle(d);
        d.entries
            .get(d.pos)
            .and_then(|e| e.as_ref().map(|e| e.1.clone()))
    })
    .unwrap_or_default())
}
fn os_set_info(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let v = a.val(0);
    let old = os(a.this_obj(), |d| {
        os_settle(d);
        let pos = d.pos;
        d.entries
            .get_mut(pos)
            .and_then(|e| e.as_mut().map(|e| std::mem::replace(&mut e.1, v)))
    });
    drop(old);
    Ok(Value::Null)
}
fn os_count(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.this_obj().clone();
    let mode = a.int_or(0, 0);
    let n = os(&s, |d| d.live as i64);
    if mode == 1 {
        let mut total = n;
        for (_, inf) in os(&s, |d| d.live_entries()) {
            total += vm.count_value(&inf, true)?;
        }
        return Ok(Value::Int(total));
    }
    Ok(Value::Int(n))
}
fn os_rewind(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    os(a.this_obj(), |d| {
        d.pos = 0;
        d.ord = 0;
        os_settle(d);
    });
    Ok(Value::Null)
}
fn os_valid(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Bool(os(a.this_obj(), |d| {
        os_settle(d);
        d.pos < d.entries.len()
    })))
}
fn os_key_m(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Int(os(a.this_obj(), |d| d.ord)))
}
fn os_current(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = os(a.this_obj(), |d| {
        os_settle(d);
        d.entries
            .get(d.pos)
            .and_then(|e| e.as_ref().map(|e| e.0.clone()))
    });
    match c {
        Some(o) => Ok(Value::Object(o)),
        None => vm.throw("RuntimeException", "Called current() on invalid iterator"),
    }
}
fn os_next(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    os(a.this_obj(), |d| {
        os_settle(d);
        if d.pos < d.entries.len() {
            d.pos += 1;
            d.ord += 1;
        }
        os_settle(d);
    });
    Ok(Value::Null)
}
fn os_seek(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = a.int(0);
    let ok = os(a.this_obj(), |d| {
        if n < 0 || n as usize >= d.live {
            return false;
        }
        d.pos = 0;
        d.ord = 0;
        os_settle(d);
        while d.ord < n {
            d.pos += 1;
            d.ord += 1;
            os_settle(d);
        }
        true
    });
    if !ok {
        return vm.throw(
            "OutOfBoundsException",
            &format!("Seek position {} is out of range", n),
        );
    }
    Ok(Value::Null)
}
fn os_offset_exists(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.this_obj().clone();
    // test SplObjectStorage_coalesce
    let o = obj_arg(vm, a, 0, "offsetExists")?;
    Ok(Value::Bool(os_find(vm, &s, &o)?.is_some()))
}
fn os_offset_get(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.this_obj().clone();
    let o = obj_arg(vm, a, 0, "offsetGet")?;
    match os_find(vm, &s, &o)? {
        Some(v) => Ok(v),
        None => vm.throw("UnexpectedValueException", "Object not found"),
    }
}
fn os_offset_set(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.this_obj().clone();
    let o = obj_arg(vm, a, 0, "offsetSet")?;
    let inf = if a.has(1) { a.val(1) } else { Value::Null };
    os_put(vm, &s, o, inf)?;
    Ok(Value::Null)
}
fn os_offset_unset(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.this_obj().clone();
    let o = obj_arg(vm, a, 0, "offsetUnset")?;
    os_remove(vm, &s, &o)?;
    Ok(Value::Null)
}
fn os_get_hash(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let h = a.obj(0).map(|o| o.handle()).unwrap_or(0);
    Ok(Value::Str(PhpStr::from(format!("{:032x}", h))))
}
fn os_serialize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.this_obj().clone();
    let mut flat = PhpArray::new();
    for (o, inf) in os(&s, |d| d.live_entries()) {
        let _ = flat.push(Value::Object(o));
        let _ = flat.push(inf);
    }
    let mut out = PhpArray::new();
    let _ = out.push(Value::Array(flat));
    let _ = out.push(Value::Array(members(vm, &s)));
    Ok(Value::Array(out))
}
fn os_unserialize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.this_obj().clone();
    let data = a.array(0);
    let (Some(Value::Array(flat)), Some(Value::Array(m))) = (
        data.get_int(0).map(|v| v.deref()),
        data.get_int(1).map(|v| v.deref()),
    ) else {
        return vm.throw(
            "UnexpectedValueException",
            "Incomplete or ill-typed serialization data",
        );
    };
    if flat.len() % 2 != 0 {
        return vm.throw("UnexpectedValueException", "Odd number of elements");
    }
    let vals: Vec<Value> = flat.iter().map(|(_, v)| v.deref()).collect();
    for pair in vals.chunks(2) {
        let Value::Object(o) = &pair[0] else {
            return vm.throw("UnexpectedValueException", "Non-object key");
        };
        os_put(vm, &s, o.clone(), pair[1].clone())?;
    }
    restore_members(vm, &s, &m)?;
    Ok(Value::Null)
}
fn os_serialize_str(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.this_obj().clone();
    let entries = os(&s, |d| d.live_entries());
    vm.with_ser_ctx(|vm| {
        let mut out = b"x:".to_vec();
        out.extend_from_slice(&vm.serialize_value(&Value::Int(entries.len() as i64))?);
        for (i, (o, inf)) in entries.into_iter().enumerate() {
            if i > 0 {
                out.push(b';');
            }
            out.extend_from_slice(&vm.serialize_value(&Value::Object(o))?);
            out.push(b',');
            out.extend_from_slice(&vm.serialize_value(&inf)?);
        }
        out.extend_from_slice(b";m:");
        let m = Value::Array(members(vm, &s));
        out.extend_from_slice(&vm.serialize_value(&m)?);
        Ok(Value::Str(PhpStr::from(out)))
    })
}
fn os_unserialize_str(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.this_obj().clone();
    let data = a.str(0);
    let b = data.as_bytes();
    if b.is_empty() {
        return Ok(Value::Null);
    }
    // `x:` count, then each element as object [`,` info] `;`, then `m:`
    // members, read in the value table of the unserialize() in progress.
    // Error offsets observed on PHP 8.5 (test
    // SplObjectStorage_unserialize_bad): an object that is not one, or
    // comes from `R:`, fails at its start.
    let parsed = vm.with_unser_ctx(|vm| -> R<Result<Value, usize>> {
        if !b.starts_with(b"x:") {
            return Ok(Err(0));
        }
        let mut cur = 2;
        let n = match vm.unser_part(&b[cur..], "SplObjectStorage::unserialize()")? {
            Ok((Value::Int(n), len)) if n >= 0 => {
                cur += len;
                n
            }
            // A negative count fails before its `;`.
            Ok((Value::Int(_), len)) => return Ok(Err(cur + len - 1)),
            Ok((_, len)) => return Ok(Err(cur + len)),
            Err(_) => return Ok(Err(cur)),
        };
        for _ in 0..n {
            let obj = match vm.unser_part(&b[cur..], "SplObjectStorage::unserialize()")? {
                Ok((Value::Object(o), len)) if b[cur] != b'R' => {
                    cur += len;
                    o
                }
                Ok(_) => return Ok(Err(cur)),
                Err(off) => return Ok(Err(cur + off)),
            };
            let mut inf = Value::Null;
            if b.get(cur) == Some(&b',') {
                cur += 1;
                match vm.unser_part(&b[cur..], "SplObjectStorage::unserialize()")? {
                    Ok((v, len)) => {
                        cur += len;
                        inf = v;
                    }
                    Err(off) => return Ok(Err(cur + off)),
                }
            }
            if b.get(cur) != Some(&b';') {
                return Ok(Err(cur));
            }
            cur += 1;
            os_put(vm, &s, obj, inf)?;
        }
        if !b[cur..].starts_with(b"m:") {
            return Ok(Err(cur));
        }
        cur += 2;
        match vm.unser_part(&b[cur..], "SplObjectStorage::unserialize()")? {
            Ok((v @ Value::Array(_), _)) => Ok(Ok(v)),
            Ok((_, len)) => Ok(Err(cur + len)),
            Err(off) => Ok(Err(cur + off)),
        }
    })?;
    match parsed {
        Ok(Value::Array(m)) => restore_members(vm, &s, &m)?,
        Ok(_) => {}
        Err(at) => {
            return vm.throw(
                "UnexpectedValueException",
                &format!("Error at offset {} of {} bytes", at, b.len()),
            );
        }
    }
    Ok(Value::Null)
}
fn os_debug_info(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.this_obj().clone();
    let mut out = members(vm, &s);
    let mut st = PhpArray::new();
    for (o, inf) in os(&s, |d| d.live_entries()) {
        let mut e = PhpArray::new();
        e.insert(Key::from("obj"), Value::Object(o));
        e.insert(Key::from("inf"), inf);
        let _ = st.push(Value::Array(e));
    }
    out.insert(private_key("SplObjectStorage", "storage"), Value::Array(st));
    Ok(Value::Array(out))
}

// ================================================================ SplFileObject

/// SplFileObject (written in PHP, spl_files.php) can't be cloned (test
/// SplFileObject/bug72888: "Trying to clone an uncloneable object of
/// class SplFileObject").
fn sfo_hooks() -> NativeHooks {
    NativeHooks {
        create: Some(|vm, o| {
            let cls = vm.class_of(o);
            cls.flags
                .set(cls.flags.get() | crate::class::cflags::NOT_CLONEABLE);
            mark_check_init(&cls);
        }),
        ..Default::default()
    }
}

fn glob_hooks() -> NativeHooks {
    NativeHooks {
        create: Some(|vm, o| mark_check_init(&vm.class_of(o))),
        ..Default::default()
    }
}

/// A user subclass of SplFileObject, GlobIterator or
/// RecursiveIteratorIterator checks, before each of its own methods, that
/// the parent constructor ran (observed on PHP 8.5; tests gh8318,
/// bug41828).
fn mark_check_init(cls: &Class) {
    if !cls.has_flag(crate::class::cflags::INTERNAL) {
        cls.flags
            .set(cls.flags.get() | crate::class::cflags::CHECK_INIT);
    }
}

/// See [`mark_check_init`]: `f` is about to run on `o`.
pub(crate) fn check_init(vm: &mut Vm, o: &ObjRef, f: &crate::func::Func) -> R<()> {
    // Only methods the user wrote (the internal ones check for
    // themselves), and not the constructor.
    let scope = f.scope.get();
    if scope.is_null()
        || unsafe { &*scope }.has_flag(crate::class::cflags::INTERNAL)
        || f.name.eq_ignore_ascii_case(b"__construct")
    {
        return Ok(());
    }
    let cls = vm.class_of(o);
    if is_subclass_named(&cls, b"recursiveiteratoriterator") {
        if vm.get_prop_raw(o, b"__its").is_null() {
            return vm.throw(
                "Error",
                &format!(
                    "The {} instance wasn't initialized properly",
                    cls.name.dname()
                ),
            );
        }
        return Ok(());
    }
    let prop: &[u8] = if is_subclass_named(&cls, b"splfileobject") {
        b"__fp"
    } else {
        b"__dir"
    };
    if vm.get_prop_raw(o, prop).is_null() {
        return vm.throw(
            "Error",
            "The parent constructor was not called: the object is in an invalid state",
        );
    }
    Ok(())
}

/// RecursiveIteratorIterator (spl_iterators.php): foreach over one whose
/// constructor didn't run fails before rewind() (tests
/// RecursiveIteratorIterator_not_initialized, bug54281).
fn rii_hooks() -> NativeHooks {
    NativeHooks {
        create: Some(|vm, o| mark_check_init(&vm.class_of(o))),
        get_iterator: Some(|vm, o, _| {
            if vm.get_prop_raw(o, b"__its").is_null() {
                return vm.throw("Error", "Object is not initialized");
            }
            Ok(o.clone())
        }),
        ..Default::default()
    }
}

// ================================================================ MultipleIterator

fn mi_construct(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.int_or(0, 1);
    os(a.this_obj(), |d| d.flags = f);
    Ok(Value::Null)
}
fn mi_get_flags(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Int(os(a.this_obj(), |d| d.flags)))
}
fn mi_set_flags(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.int(0);
    os(a.this_obj(), |d| d.flags = f);
    Ok(Value::Null)
}
fn mi_attach(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.this_obj().clone();
    let it = a.obj(0).unwrap();
    let inf = if a.has(1) { a.val(1) } else { Value::Null };
    if os(&s, |d| d.flags) & 2 != 0 {
        match &inf {
            Value::Int(_) | Value::Str(_) => {}
            Value::Null => {
                return vm.type_error("MultipleIterator::attachIterator(): Argument #2 ($info) must be of type string|int, null given");
            }
            _ => {}
        }
        // The info must be unique.
        for (_, other) in os(&s, |d| d.live_entries()) {
            if goro_runtime::compare::strict_eq(&other, &inf) {
                return vm.throw("InvalidArgumentException", "Key duplication error");
            }
        }
    }
    os_put(vm, &s, it, inf)?;
    Ok(Value::Null)
}
fn mi_detach(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.this_obj().clone();
    let it = a.obj(0).unwrap();
    os_remove(vm, &s, &it)?;
    Ok(Value::Null)
}
fn mi_contains(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.this_obj().clone();
    let it = a.obj(0).unwrap();
    Ok(Value::Bool(os_find(vm, &s, &it)?.is_some()))
}
fn mi_count(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Int(os(a.this_obj(), |d| d.live as i64)))
}
fn mi_rewind(vm: &mut Vm, a: &mut Args) -> R<Value> {
    for (it, _) in os(a.this_obj(), |d| d.live_entries()) {
        vm.call_method(&it, b"rewind", Vec::new())?;
    }
    Ok(Value::Null)
}
fn mi_next(vm: &mut Vm, a: &mut Args) -> R<Value> {
    for (it, _) in os(a.this_obj(), |d| d.live_entries()) {
        vm.call_method(&it, b"next", Vec::new())?;
    }
    Ok(Value::Null)
}
fn mi_valid(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (entries, flags) = os(a.this_obj(), |d| (d.live_entries(), d.flags));
    if entries.is_empty() {
        return Ok(Value::Bool(false));
    }
    let all = flags & 1 != 0;
    for (it, _) in entries {
        let v = vm.call_method(&it, b"valid", Vec::new())?;
        let v = vm.to_bool(&v);
        if all && !v {
            return Ok(Value::Bool(false));
        }
        if !all && v {
            return Ok(Value::Bool(true));
        }
    }
    Ok(Value::Bool(all))
}
fn mi_values(vm: &mut Vm, a: &Args, current: bool) -> R<Value> {
    let (entries, flags) = os(a.this_obj(), |d| (d.live_entries(), d.flags));
    if entries.is_empty() {
        // test multiple_iterator_001
        return vm.throw(
            "RuntimeException",
            &format!(
                "Called {}() on an invalid iterator",
                if current { "current" } else { "key" }
            ),
        );
    }
    let mut out = PhpArray::new();
    for (it, inf) in entries {
        let valid = vm.call_method(&it, b"valid", Vec::new())?;
        let v = if vm.to_bool(&valid) {
            vm.call_method(&it, if current { b"current" } else { b"key" }, Vec::new())?
        } else {
            if flags & 1 != 0 {
                let what = if current { "value" } else { "key" };
                return vm
                    .throw(
                        "RuntimeException",
                        &format!(
                            "Called {}() with non valid sub iterator",
                            if current { "current" } else { "key" }
                        ),
                    )
                    .map(|_: Value| {
                        let _ = what;
                        Value::Null
                    });
            }
            Value::Null
        };
        if flags & 2 != 0 {
            let k = match &inf {
                Value::Int(i) => Key::Int(*i),
                Value::Str(s) => Key::from_php_str(s),
                _ => {
                    return vm.throw(
                        "InvalidArgumentException",
                        "Sub-Iterator is associated with NULL",
                    );
                }
            };
            out.insert(k, v);
        } else {
            let _ = out.push(v);
        }
    }
    Ok(Value::Array(out))
}
fn mi_key(vm: &mut Vm, a: &mut Args) -> R<Value> {
    mi_values(vm, a, false)
}
fn mi_current(vm: &mut Vm, a: &mut Args) -> R<Value> {
    mi_values(vm, a, true)
}

#[allow(dead_code)]
fn unused_lower(s: &[u8]) -> PhpStr {
    lower(s)
}
