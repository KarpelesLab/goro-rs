//! PHP's zlib extension on bytes (PHP manual: Zlib; RFC 1950, 1951, 1952),
//! with a deflate encoder whose output is byte-identical to PHP 8.5's
//! (which links zlib 1.3.1) at every level, strategy, memory level, window
//! size and flush pattern.
//!
//! Behaviour sources: the PHP manual, the zlib manual's API description and
//! published per-level parameters, RFCs 1950-1952, the expected output of
//! php-src's `ext/zlib/tests/*.phpt`, and observations of PHP 8.5 recorded
//! once in `tests/fixtures/*_php85.txt` by the drivers next to them.
//!
//! # Engine wiring
//!
//! Functions take and return bytes and plain integers after the engine's
//! usual argument coercion. Failures come back as [`ZlibError`]: a
//! `ValueError`/`TypeError` to throw with the full message, or a warning
//! (message without the `func(): ` prefix) after which the function returns
//! `false`.
//!
//! - `gzcompress`, `gzdeflate`, `gzencode`, `zlib_encode`: [`gzcompress`],
//!   [`gzdeflate`], [`gzencode`], [`zlib_encode`].
//! - `gzuncompress`, `gzinflate`, `gzdecode`, `zlib_decode`:
//!   [`gzuncompress`], [`gzinflate`], [`gzdecode`], [`zlib_decode`].
//! - `deflate_init`/`deflate_add`: [`deflate_init`] with [`DeflateOptions`]
//!   (the engine reads `level`, `memory`, `window`, `strategy` with PHP's
//!   `(int)` conversion, and `dictionary` as a [`DictOption`]; unknown keys
//!   are ignored), giving a [`DeflateContext`] to store in the
//!   `DeflateContext` object; [`DeflateContext::add`] is `deflate_add`.
//! - `inflate_init`/`inflate_add`/`inflate_get_status`/
//!   `inflate_get_read_len`: [`inflate_init`] with [`InflateOptions`],
//!   [`InflateContext::add`], [`InflateContext::status`],
//!   [`InflateContext::read_len`].
//! - `gzopen` and the `compress.zlib://` wrapper: [`gzfile::parse_mode`],
//!   then [`gzfile::GzReader`] over a `Read + Seek` source or
//!   [`gzfile::GzWriter`] over a `Write` sink; the PHP stream layer
//!   (buffering, `fgets`, `feof`, positions) stays in the engine. PHP's
//!   `gzfile()` cuts each line at its first NUL byte (observed on PHP 8.5).
//! - The `zlib.deflate` and `zlib.inflate` stream filters:
//!   [`filter::deflate_filter`] and [`filter::inflate_filter`].
//! - Constants: [`consts::ALL`].
//!
//! The codecs are also usable directly: [`Deflater`] (one zlib
//! `deflate()` call at a time with an output budget, or unbounded) and
//! [`Inflater`].

pub mod api;
mod checksum;
pub mod deflate;
pub mod filter;
pub mod gzfile;
pub mod inflate;
mod trees;

pub use api::*;
pub use deflate::{DeflateParams, DeflateStatus, Deflater, Flush, Strategy, Wrap};
pub use inflate::{InflateFlush, InflateStatus, InflateWrap, Inflater};

/// The zlib extension's constants (values observed on PHP 8.5).
pub mod consts {
    /// A constant's value.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ConstValue {
        Int(i64),
        Str(&'static str),
    }

    /// Every constant with its value, for registration by the engine.
    pub const ALL: [(&str, ConstValue); 27] = [
        ("FORCE_GZIP", ConstValue::Int(31)),
        ("FORCE_DEFLATE", ConstValue::Int(15)),
        ("ZLIB_ENCODING_RAW", ConstValue::Int(-15)),
        ("ZLIB_ENCODING_GZIP", ConstValue::Int(31)),
        ("ZLIB_ENCODING_DEFLATE", ConstValue::Int(15)),
        ("ZLIB_NO_FLUSH", ConstValue::Int(0)),
        ("ZLIB_PARTIAL_FLUSH", ConstValue::Int(1)),
        ("ZLIB_SYNC_FLUSH", ConstValue::Int(2)),
        ("ZLIB_FULL_FLUSH", ConstValue::Int(3)),
        ("ZLIB_BLOCK", ConstValue::Int(5)),
        ("ZLIB_FINISH", ConstValue::Int(4)),
        ("ZLIB_FILTERED", ConstValue::Int(1)),
        ("ZLIB_HUFFMAN_ONLY", ConstValue::Int(2)),
        ("ZLIB_RLE", ConstValue::Int(3)),
        ("ZLIB_FIXED", ConstValue::Int(4)),
        ("ZLIB_DEFAULT_STRATEGY", ConstValue::Int(0)),
        ("ZLIB_VERSION", ConstValue::Str("1.3.1")),
        ("ZLIB_VERNUM", ConstValue::Int(4880)),
        ("ZLIB_OK", ConstValue::Int(0)),
        ("ZLIB_STREAM_END", ConstValue::Int(1)),
        ("ZLIB_NEED_DICT", ConstValue::Int(2)),
        ("ZLIB_ERRNO", ConstValue::Int(-1)),
        ("ZLIB_STREAM_ERROR", ConstValue::Int(-2)),
        ("ZLIB_DATA_ERROR", ConstValue::Int(-3)),
        ("ZLIB_MEM_ERROR", ConstValue::Int(-4)),
        ("ZLIB_BUF_ERROR", ConstValue::Int(-5)),
        ("ZLIB_VERSION_ERROR", ConstValue::Int(-6)),
    ];
}
