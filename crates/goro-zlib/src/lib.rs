//! A pure Rust port of madler zlib 1.3.1.
//!
//! The compressor reproduces zlib's output byte for byte (same match
//! finder, same block splitting and Huffman tree construction), which PHP
//! code and its test-suite depend on. The decompressor follows inflate.c's
//! state machine so return codes and consumption match zlib's.

// Index loops mirror the C source, which keeps the port easy to audit.
#![allow(clippy::needless_range_loop, clippy::explicit_counter_loop)]

pub mod checksum;
pub mod deflate;
pub mod gz;
pub mod inflate;
mod inftrees;

pub use checksum::{adler32, crc32};
pub use deflate::Deflate;
pub use inflate::Inflate;

pub const Z_NO_FLUSH: i32 = 0;
pub const Z_PARTIAL_FLUSH: i32 = 1;
pub const Z_SYNC_FLUSH: i32 = 2;
pub const Z_FULL_FLUSH: i32 = 3;
pub const Z_FINISH: i32 = 4;
pub const Z_BLOCK: i32 = 5;
pub const Z_TREES: i32 = 6;

pub const Z_OK: i32 = 0;
pub const Z_STREAM_END: i32 = 1;
pub const Z_NEED_DICT: i32 = 2;
pub const Z_ERRNO: i32 = -1;
pub const Z_STREAM_ERROR: i32 = -2;
pub const Z_DATA_ERROR: i32 = -3;
pub const Z_MEM_ERROR: i32 = -4;
pub const Z_BUF_ERROR: i32 = -5;
pub const Z_VERSION_ERROR: i32 = -6;

pub const Z_FILTERED: i32 = 1;
pub const Z_HUFFMAN_ONLY: i32 = 2;
pub const Z_RLE: i32 = 3;
pub const Z_FIXED: i32 = 4;
pub const Z_DEFAULT_STRATEGY: i32 = 0;

pub const Z_BINARY: i32 = 0;
pub const Z_TEXT: i32 = 1;
pub const Z_UNKNOWN: i32 = 2;

pub const Z_DEFLATED: i32 = 8;
pub const Z_DEFAULT_COMPRESSION: i32 = -1;
pub const MAX_WBITS: i32 = 15;
pub const MAX_MEM_LEVEL: i32 = 9;
pub const DEF_MEM_LEVEL: i32 = 8;

pub const ZLIB_VERSION: &str = "1.3.1";
pub const ZLIB_VERNUM: i64 = 0x1310;

/// zError(): the message for a zlib return code.
pub fn z_error(err: i32) -> &'static str {
    match err {
        Z_NEED_DICT => "need dictionary",
        Z_STREAM_END => "stream end",
        Z_OK => "",
        Z_ERRNO => "file error",
        Z_STREAM_ERROR => "stream error",
        Z_DATA_ERROR => "data error",
        Z_MEM_ERROR => "insufficient memory",
        Z_BUF_ERROR => "buffer error",
        Z_VERSION_ERROR => "incompatible version",
        _ => "",
    }
}

/// The caller's buffers for one deflate()/inflate() call: `next_in` is
/// `input[in_pos..]`, `next_out` is `output[out_pos..]`.
pub struct Io<'a> {
    pub input: &'a [u8],
    pub in_pos: usize,
    pub output: &'a mut [u8],
    pub out_pos: usize,
}

impl<'a> Io<'a> {
    pub fn new(input: &'a [u8], output: &'a mut [u8]) -> Self {
        Io {
            input,
            in_pos: 0,
            output,
            out_pos: 0,
        }
    }
    #[inline]
    pub fn avail_in(&self) -> usize {
        self.input.len() - self.in_pos
    }
    #[inline]
    pub fn avail_out(&self) -> usize {
        self.output.len() - self.out_pos
    }
}

/// compress2()-style one-shot helper: deflateInit2 + one deflate(Z_FINISH)
/// into a buffer of `out_size` bytes. Returns the zlib status and output.
pub fn deflate_once(
    input: &[u8],
    level: i32,
    window_bits: i32,
    mem_level: i32,
    strategy: i32,
    out_size: usize,
) -> (i32, Vec<u8>) {
    let mut d = match Deflate::new(level, Z_DEFLATED, window_bits, mem_level, strategy) {
        Ok(d) => d,
        Err(e) => return (e, Vec::new()),
    };
    let mut out = vec![0u8; out_size];
    let mut io = Io::new(input, &mut out);
    let st = d.deflate(&mut io, Z_FINISH);
    let n = io.out_pos;
    out.truncate(n);
    (st, out)
}
