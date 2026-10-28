//! PHP values: strings, numbers, ordered arrays, objects, conversions.
//!
//! This crate is PHP 8.5's value model, independent of the compiler and VM.
//! Behaviour follows the PHP manual and RFCs, the expected output of
//! php-src's `.phpt` tests, and observations of PHP 8.5 (the fixture in
//! `tests/fixtures` was generated once from the reference binary).
//!
//! # Representation
//!
//! | type | size | notes |
//! |---|---|---|
//! | [`Value`] | 16 bytes | `#[repr(u64)]` tag + 8-byte payload; `Option<Value>` and `PhpResult<Value>` are 16 bytes too |
//! | [`PhpStr`] | 8 bytes | thin pointer to one block: refcount, cached hash, len, cap, bytes |
//! | [`PhpArray`] | 8 bytes | `Option<Rc<ArrayData>>`; the empty array is `None` |
//! | [`Key`] | 16 bytes | `Int(i64)` or `Str(PhpStr)` |
//! | [`PhpRef`] / [`ObjRef`] / [`ResRef`] | 8 bytes | `Rc` handles |
//!
//! Null, bools, ints and floats live inline and never allocate. Everything
//! else is one non-atomic refcounted pointer, so cloning a value costs at
//! most one increment. The runtime is single-threaded (values are `!Send`),
//! like a PHP request.
//!
//! Strings are byte strings with copy-on-write: [`PhpStr::make_mut`] and
//! [`PhpStr::push_bytes`] copy only when shared and append in place
//! otherwise. The empty string, the 256 one-byte strings and
//! [`PhpStr::intern`]ed literals are immortal (no refcount traffic).
//!
//! Arrays ([`mod@array`]) are copy-on-write ordered maps. A list whose keys are
//! exactly `0..n` is *packed* (a plain `Vec<Value>`); anything else is a
//! hash: an insertion-ordered vector of 32-byte buckets (value, stored
//! hash, optional string key) plus an open-addressing index of `(tag,
//! entry)` slots, no index at all for up to 8 entries. Deletion leaves a
//! tombstone so positions stay stable; positions (the internal pointer and
//! the iterators of by-reference `foreach`) are remapped when the bucket
//! vector is compacted.
//!
//! # Ownership, destructors and cycles
//!
//! Every container is reference counted with `Rc`, so values are freed
//! deterministically, the moment the last reference goes, as PHP does.
//!
//! - **Destructors** ([`object`]): when the last [`ObjRef`] to an object whose
//!   class has a destructor is dropped, the drop keeps the object alive by
//!   moving it to a thread-local queue and marks its destructor as called.
//!   The engine drains the queue at its next safe point (right after the
//!   instruction that released the value) with
//!   [`object::pop_pending_destructor`]; [`object::has_pending_destructors`]
//!   is a cheap check. The destructor may resurrect the object; dropping the
//!   handle afterwards frees it. Objects without destructors are freed by
//!   the last drop.
//! - **Cycles** ([`gc`]): dropping a handle that leaves a reference cell, an
//!   object, or an array that has held a container alive records it as a
//!   *possible root* (PHP manual, Garbage Collection). [`gc::collect_cycles`]
//!   scans the subgraph reachable from the roots, counts the references each
//!   node receives from inside it, keeps whatever is referenced from outside
//!   (and what that reaches), runs destructors of the garbage through the
//!   engine's callback (rescanning, since they may resurrect), then breaks
//!   the cycles by clearing garbage reference cells and objects. The engine
//!   runs it for `gc_collect_cycles()` and when [`gc::root_count`] reaches
//!   [`gc::DEFAULT_THRESHOLD`].
//! - **References** ([`PhpRef`]) are shared cells; a slot "is a reference"
//!   when it holds `Value::Ref`. Copying an array dereferences elements whose
//!   reference is held by that array only, as PHP does.
//!
//! # Diagnostics
//!
//! Operations take `&mut dyn` [`Ctx`]. Warnings, notices and deprecations
//! are reported through [`Ctx::notice`] in PHP's order; what PHP throws is
//! returned as `Err(`[`PhpError`]`)` with its class ([`ErrorKind`]) and
//! message. The [`Ctx`] also supplies ini `precision` and object behaviour
//! that needs user code (`__toString`, comparison handlers, operator
//! overloading); [`Diagnostics`] is a ready-made context with PHP's default
//! object behaviour.
//!
//! # Engine API
//!
//! - Conversions: [`convert::to_bool`], [`convert::to_int`],
//!   [`convert::to_float`], [`convert::to_str`] / [`convert::append_str`],
//!   [`convert::to_array`], [`convert::to_key`] (array offsets),
//!   [`convert::float_to_string`] (`echo`, precision) and
//!   [`float_fmt::format_php_repr`] (`var_dump`/`var_export`/`json_encode`,
//!   serialize_precision).
//! - Numeric strings: [`numeric::parse_numeric`], [`numeric::is_numeric_str`].
//! - Operators: [`ops::binary_op`] or the individual functions
//!   ([`ops::add`], [`ops::concat`], [`ops::concat_assign`], ...),
//!   [`ops::neg`], [`ops::bit_not`], [`ops::not`], [`ops::increment`],
//!   [`ops::decrement`], [`ops::intdiv`].
//! - Comparison: [`compare::loose_eq`], [`compare::strict_eq`],
//!   [`compare::compare`] (`<=>`), [`compare::less`], [`compare::greater`]
//!   and friends (`>` is evaluated as a swapped `<`, as PHP does).
//! - Arrays: [`PhpArray::get_int`] / [`PhpArray::get_str`] (normalised keys)
//!   / [`PhpArray::get_bytes`], [`PhpArray::insert`], [`PhpArray::assign`]
//!   (writes through references), [`PhpArray::push`], [`PhpArray::entry`]
//!   (nested writes), [`PhpArray::remove`]; `foreach` by value iterates a
//!   clone with [`ArrayData::next_from`]; `foreach` by reference uses
//!   [`PhpArray::new_iter_id`] and [`ArrayData::iter_next_ref`]; the internal
//!   pointer is [`ArrayData::current`], [`ArrayData::advance`], ...
//! - Slots: [`Value::assign`] (assignment through references, returns the old
//!   value so the caller decides when it is released), [`Value::make_ref`]
//!   (`=&`), [`Value::deref`].
//! - Objects: [`ObjRef::new`] with the engine's [`ClassInfo`], property
//!   storage in [`Object::slots`] / [`Object::props`], native data via
//!   [`NativeData`] (which must trace its values for the collector).

pub mod array;
pub mod compare;
pub mod convert;
pub mod ctx;
pub mod error;
pub mod float_fmt;
pub mod gc;
pub mod mem;
pub mod numeric;
pub mod object;
pub mod ops;
pub mod reference;
pub mod string;
pub mod value;

pub use array::{ArrayData, Key, KeyRef, PhpArray};
pub use ctx::{Ctx, Diagnostics};
pub use error::{ErrorKind, Level, Notice, PhpError, PhpResult};
pub use numeric::Number;
pub use object::{ClassInfo, NativeData, ObjRef, Object, ResRef, WeakObj};
pub use reference::PhpRef;
pub use string::PhpStr;
pub use value::{Type, Value};
