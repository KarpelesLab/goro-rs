//! goro-sqlite: a pure-Rust port of the SQLite database engine.
//!
//! This is a module-by-module port of SQLite 3.54.1 (public domain).  The
//! Rust modules correspond to the C source files of the same name
//! (`main.c` is `mainapi`, `where.c` is `where_`) and keep the C names of
//! functions, structures and fields so that the port can be checked and
//! maintained against upstream.  See PORTING.md for the conventions.

#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]
#![allow(clippy::all)]

/// `mprintf!(db, "fmt", args...)`: sqlite3MPrintf() returning a ZStr.
/// Arguments are converted with `PfArg::from` (integers, f64, byte
/// strings, `&Token`, `ExprId` for %#T, `SrcItemId` for %S).
#[macro_export]
macro_rules! mprintf {
    ($db:expr, $fmt:expr $(, $a:expr)* $(,)?) => {
        $crate::printf::sqlite3MPrintf($db, $fmt.as_ref(), &[$($crate::printf::PfArg::from($a)),*])
    };
}

/// `errmsg!(db, "fmt", args...)`: sqlite3ErrorMsg() on the current parse.
#[macro_export]
macro_rules! errmsg {
    ($db:expr, $fmt:expr $(, $a:expr)* $(,)?) => {
        $crate::util::sqlite3ErrorMsg($db, $fmt.as_ref(), &[$($crate::printf::PfArg::from($a)),*])
    };
}

pub mod arena;
pub mod consts;
pub mod hash;
pub mod int;

pub mod alter;
pub mod analyze;
pub mod attach;
pub mod auth;
pub mod backup;
pub mod bitvec;
pub mod btree;
pub mod build;
pub mod callback;
pub mod complete;
pub mod date;
pub mod delete;
pub mod expr;
pub mod fkey;
pub mod func;
pub mod global;
pub mod insert;
pub mod json;
pub mod legacy;
pub mod loadext;
pub mod mainapi;
pub mod memjournal;
pub mod os;
pub mod os_unix;
pub mod pager;
pub mod parse;
pub mod pcache;
pub mod pcache1;
pub mod pragma;
pub mod prepare;
pub mod printf;
pub mod random;
pub mod resolve;
pub mod rowset;
pub mod select;
pub mod status;
pub mod tokenize;
pub mod trigger;
pub mod update;
pub mod upsert;
pub mod utf;
pub mod util;
pub mod vacuum;
pub mod vdbe;
pub mod vdbeapi;
pub mod vdbeaux;
pub mod vdbeblob;
pub mod vdbemem;
pub mod vdbesort;
pub mod vdbetrace;
pub mod vtab;
pub mod wal;
pub mod walker;
pub mod where_;
pub mod wherecode;
pub mod whereexpr;
pub mod window;

/// The SQLite version this port implements.
pub const SQLITE_VERSION: &str = "3.54.1";
pub const SQLITE_SOURCE_ID: &str =
    "2026-10-16 14:45:20 555e07ef33f69a0957d24d8e555b702464a131d6f1607f4c3c1e4551e8e6ca76";
