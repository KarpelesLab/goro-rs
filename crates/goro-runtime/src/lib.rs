//! PHP values: strings, numbers, ordered arrays, objects, conversions.
//!
//! This crate is PHP 8.5's value model, independent of the compiler and VM:
//!
//! - [`Value`]: a 16-byte tagged value (null, bool, int, float, string,
//!   array, object, resource, reference); scalars never allocate.
//! - [`PhpStr`]: refcounted copy-on-write byte strings, with interning.
//! - [`PhpArray`]: copy-on-write ordered maps with a packed list form.
//! - [`PhpRef`]: `&` reference cells.
//! - [`ObjRef`] / [`ResRef`]: object and resource handles, with deferred
//!   destructors ([`object`]) and a cycle collector ([`gc`]).
//! - Conversions ([`convert`]), numeric strings ([`numeric`]), float
//!   formatting ([`float_fmt`]), comparison ([`compare`]) and operators
//!   ([`ops`]).
//!
//! Operations report warnings and deprecations through [`Ctx::notice`] and
//! return [`PhpError`] for what PHP throws; the engine turns both into PHP
//! errors and exceptions.
//!
//! Behaviour follows the PHP manual and RFCs, the expected output of
//! php-src's `.phpt` tests, and observations of PHP 8.5 (the test fixture in
//! `tests/fixtures` was generated from the reference binary).

pub mod array;
pub mod compare;
pub mod convert;
pub mod ctx;
pub mod error;
pub mod float_fmt;
pub mod gc;
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
