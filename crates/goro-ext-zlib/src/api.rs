//! PHP's zlib functions on bytes: argument checks, PHP's buffer handling
//! around zlib's calls, and PHP's warnings.
//!
//! Behaviour sources: the PHP manual's zlib pages, ext/zlib/tests/*.phpt,
//! and observation of PHP 8.5 recorded in tests/fixtures/behaviour*.txt.

use crate::deflate::{DeflateParams, DeflateStatus, Deflater, Flush, Strategy, Wrap};
use crate::inflate::{InflateFlush, InflateStatus, InflateWrap, Inflater};

/// `ZLIB_ENCODING_RAW`, `ZLIB_ENCODING_GZIP`, `ZLIB_ENCODING_DEFLATE`.
pub const ENCODING_RAW: i64 = -15;
pub const ENCODING_GZIP: i64 = 31;
pub const ENCODING_DEFLATE: i64 = 15;

/// How a PHP-level call fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZlibError {
    /// A thrown `ValueError` with its full message.
    ValueError(String),
    /// A thrown `TypeError` with its full message.
    TypeError(String),
    /// An `E_WARNING` (message without the `func(): ` prefix), after which
    /// the function returns `false`.
    Warning(String),
}

/// A diagnostic raised without failing the call; the message excludes the
/// `func(): ` prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Diag {
    Warning(String),
    Notice(String),
}

fn level_error(func: &str, arg: u32) -> ZlibError {
    ZlibError::ValueError(format!(
        "{func}(): Argument #{arg} ($level) must be between -1 and 9"
    ))
}

fn encoding_error(func: &str, arg: u32) -> ZlibError {
    ZlibError::ValueError(format!(
        "{func}(): Argument #{arg} ($encoding) must be one of ZLIB_ENCODING_RAW, ZLIB_ENCODING_GZIP, or ZLIB_ENCODING_DEFLATE"
    ))
}

fn wrap_of(encoding: i64) -> Option<Wrap> {
    match encoding {
        ENCODING_RAW => Some(Wrap::Raw),
        ENCODING_DEFLATE => Some(Wrap::Zlib),
        ENCODING_GZIP => Some(Wrap::Gzip),
        _ => None,
    }
}

/// Compresses in one go as `gzcompress()`, `gzdeflate()`, `gzencode()`
/// and `zlib_encode()` do: memory level 9 (observed on PHP 8.5: one-shot
/// output matches zlib's `memLevel` 9), 32K window, default strategy.
pub fn compress(data: &[u8], level: i32, wrap: Wrap) -> Vec<u8> {
    let mut d = Deflater::new(DeflateParams {
        level,
        window_bits: 15,
        mem_level: 9,
        strategy: Strategy::Default,
        wrap,
    })
    .expect("valid parameters");
    let mut out = Vec::with_capacity(data.len() / 3 + 64);
    d.deflate(data, Flush::Finish, &mut out);
    out
}

fn encode(
    func: &str,
    data: &[u8],
    level: i64,
    level_arg: u32,
    encoding: i64,
    enc_arg: u32,
) -> Result<Vec<u8>, ZlibError> {
    if !(-1..=9).contains(&level) {
        return Err(level_error(func, level_arg));
    }
    let wrap = wrap_of(encoding).ok_or_else(|| encoding_error(func, enc_arg))?;
    Ok(compress(data, level as i32, wrap))
}

/// `gzcompress(string $data, int $level = -1, int $encoding = ZLIB_ENCODING_DEFLATE)`.
pub fn gzcompress(data: &[u8], level: i64, encoding: i64) -> Result<Vec<u8>, ZlibError> {
    encode("gzcompress", data, level, 2, encoding, 3)
}

/// `gzdeflate(string $data, int $level = -1, int $encoding = ZLIB_ENCODING_RAW)`.
pub fn gzdeflate(data: &[u8], level: i64, encoding: i64) -> Result<Vec<u8>, ZlibError> {
    encode("gzdeflate", data, level, 2, encoding, 3)
}

/// `gzencode(string $data, int $level = -1, int $encoding = ZLIB_ENCODING_GZIP)`.
pub fn gzencode(data: &[u8], level: i64, encoding: i64) -> Result<Vec<u8>, ZlibError> {
    encode("gzencode", data, level, 2, encoding, 3)
}

/// `zlib_encode(string $data, int $encoding, int $level = -1)`.
///
/// The level is checked before the encoding (observed on PHP 8.5).
pub fn zlib_encode(data: &[u8], encoding: i64, level: i64) -> Result<Vec<u8>, ZlibError> {
    encode("zlib_encode", data, level, 3, encoding, 2)
}

/// Outcome of a one-shot decode before PHP turns it into a warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Decoded {
    End,
    Mem,
    NeedDict,
    Data,
    Buf,
}

/// Decodes the way PHP's one-shot functions drive zlib (observed on PHP
/// 8.5): the input is followed by the string's terminating NUL byte, the
/// output buffer starts at the input size (or `max_length` when smaller)
/// and grows by an eighth plus one byte per round, `max_length` is only
/// checked between rounds, and at most 100 rounds run.
fn decode_rounds(input: &[u8], max: usize, wrap: InflateWrap) -> (Decoded, Vec<u8>) {
    let mut data = Vec::with_capacity(input.len() + 1);
    data.extend_from_slice(input);
    data.push(0);
    let in_len = data.len();
    let mut size = if max > 0 && max < in_len { max } else { in_len };
    let mut inf = Inflater::new(wrap, 15);
    let mut out = Vec::new();
    let mut pos = 0;
    let mut round = 0;
    let mut status;
    loop {
        if max > 0 && max <= out.len() {
            return (Decoded::Mem, out);
        }
        let avail = size - out.len();
        let (n, st) = inf.inflate(&data[pos..], &mut out, avail, InflateFlush::None);
        pos += n;
        size += size / 8 + 1;
        status = st;
        round += 1;
        let more =
            matches!(st, InflateStatus::BufError) || (st == InflateStatus::Ok && pos < data.len());
        if !more || round >= 100 {
            break;
        }
    }
    let d = match status {
        InflateStatus::StreamEnd => Decoded::End,
        InflateStatus::NeedDict => Decoded::NeedDict,
        InflateStatus::DataError(_) | InflateStatus::Ok => Decoded::Data,
        InflateStatus::BufError => Decoded::Buf,
    };
    (d, out)
}

fn decoded_result(d: Decoded, out: Vec<u8>) -> Result<Vec<u8>, ZlibError> {
    match d {
        Decoded::End => Ok(out),
        Decoded::Mem => Err(ZlibError::Warning("insufficient memory".into())),
        Decoded::NeedDict => Err(ZlibError::Warning("need dictionary".into())),
        Decoded::Data => Err(ZlibError::Warning("data error".into())),
        Decoded::Buf => Err(ZlibError::Warning("buffer error".into())),
    }
}

fn decode(
    func: &str,
    data: &[u8],
    max_length: i64,
    wrap: InflateWrap,
) -> Result<Vec<u8>, ZlibError> {
    if max_length < 0 {
        return Err(ZlibError::ValueError(format!(
            "{func}(): Argument #2 ($max_length) must be greater than or equal to 0"
        )));
    }
    let (d, out) = decode_rounds(data, max_length as usize, wrap);
    decoded_result(d, out)
}

/// `gzuncompress(string $data, int $max_length = 0)`: zlib format.
pub fn gzuncompress(data: &[u8], max_length: i64) -> Result<Vec<u8>, ZlibError> {
    decode("gzuncompress", data, max_length, InflateWrap::Zlib)
}

/// `gzinflate(string $data, int $max_length = 0)`: raw deflate.
pub fn gzinflate(data: &[u8], max_length: i64) -> Result<Vec<u8>, ZlibError> {
    decode("gzinflate", data, max_length, InflateWrap::Raw)
}

/// `gzdecode(string $data, int $max_length = 0)`: gzip format (first
/// member only).
pub fn gzdecode(data: &[u8], max_length: i64) -> Result<Vec<u8>, ZlibError> {
    decode("gzdecode", data, max_length, InflateWrap::Gzip)
}

/// `zlib_decode(string $data, int $max_length = 0)`: zlib or gzip, and raw
/// deflate when that fails with a data error (observed on PHP 8.5).
pub fn zlib_decode(data: &[u8], max_length: i64) -> Result<Vec<u8>, ZlibError> {
    if max_length < 0 {
        return Err(ZlibError::ValueError(
            "zlib_decode(): Argument #2 ($max_length) must be greater than or equal to 0".into(),
        ));
    }
    let max = max_length as usize;
    let (d, out) = decode_rounds(data, max, InflateWrap::Auto);
    if d == Decoded::Data {
        let (d, out) = decode_rounds(data, max, InflateWrap::Raw);
        return decoded_result(d, out);
    }
    decoded_result(d, out)
}

// ------------------------------------------------------------------ options

/// The `dictionary` option of `deflate_init()` / `inflate_init()` after the
/// engine's conversions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DictOption {
    /// A string, used as is.
    Str(Vec<u8>),
    /// An array, each element already converted to a string.
    List(Vec<Vec<u8>>),
    /// Anything else, with PHP's type name for the error ("int", "null").
    Other(String),
}

/// Builds the preset dictionary: array elements are NUL-terminated and
/// concatenated (test ext/zlib/tests/dictionary_usage.phpt); empty means
/// none.
fn dictionary(func: &str, d: &Option<DictOption>) -> Result<Option<Vec<u8>>, ZlibError> {
    match d {
        None => Ok(None),
        Some(DictOption::Str(s)) => Ok(if s.is_empty() { None } else { Some(s.clone()) }),
        Some(DictOption::List(items)) => {
            let mut out = Vec::new();
            for s in items {
                if s.is_empty() {
                    return Err(ZlibError::ValueError(format!(
                        "{func}(): Argument #2 ($options) must not contain empty strings"
                    )));
                }
                if s.contains(&0) {
                    return Err(ZlibError::ValueError(format!(
                        "{func}(): Argument #2 ($options) must not contain strings with null bytes"
                    )));
                }
                out.extend_from_slice(s);
                out.push(0);
            }
            Ok(if out.is_empty() { None } else { Some(out) })
        }
        Some(DictOption::Other(t)) => Err(ZlibError::TypeError(format!(
            "{func}(): Argument #2 ($options) must be of type zero-terminated string or array, {t} given"
        ))),
    }
}

/// Options of `deflate_init()`, as integers after PHP's `(int)` conversion
/// (non-numeric strings become 0, as observed on PHP 8.5). `None` means
/// the option is absent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeflateOptions {
    pub level: Option<i64>,
    pub memory: Option<i64>,
    pub window: Option<i64>,
    pub strategy: Option<i64>,
    pub dictionary: Option<DictOption>,
}

/// Options of `inflate_init()`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InflateOptions {
    pub window: Option<i64>,
    pub dictionary: Option<DictOption>,
}

const FLUSH_ERROR: &str = "($flush_mode) must be one of ZLIB_NO_FLUSH, ZLIB_PARTIAL_FLUSH, ZLIB_SYNC_FLUSH, ZLIB_FULL_FLUSH, ZLIB_BLOCK, or ZLIB_FINISH";

// ----------------------------------------------------------------- deflate

/// A `DeflateContext` object's state.
pub struct DeflateContext {
    d: Deflater,
}

/// `deflate_init(int $encoding, array|object $options = [])`.
///
/// The options are checked in the order level, memory, window, strategy,
/// dictionary, then the encoding (observed on PHP 8.5; test
/// ext/zlib/tests/leak_invalid_encoding_with_dict.phpt).
pub fn deflate_init(encoding: i64, opts: &DeflateOptions) -> Result<DeflateContext, ZlibError> {
    let level = opts.level.unwrap_or(-1);
    if !(-1..=9).contains(&level) {
        return Err(ZlibError::ValueError(
            "deflate_init(): \"level\" option must be between -1 and 9".into(),
        ));
    }
    let memory = opts.memory.unwrap_or(8);
    if !(1..=9).contains(&memory) {
        return Err(ZlibError::ValueError(
            "deflate_init(): \"memory\" option must be between 1 and 9".into(),
        ));
    }
    let window = opts.window.unwrap_or(15);
    if !(8..=15).contains(&window) {
        return Err(ZlibError::ValueError(
            "deflate_init(): \"window\" option must be between 8 and 15".into(),
        ));
    }
    let strategy = Strategy::from_i64(opts.strategy.unwrap_or(0)).ok_or_else(|| {
        ZlibError::ValueError(
            "deflate_init(): \"strategy\" option must be one of ZLIB_FILTERED, ZLIB_HUFFMAN_ONLY, ZLIB_RLE, ZLIB_FIXED, or ZLIB_DEFAULT_STRATEGY".into(),
        )
    })?;
    let dict = dictionary("deflate_init", &opts.dictionary)?;
    let wrap = wrap_of(encoding).ok_or_else(|| encoding_error("deflate_init", 1))?;
    let mut d = Deflater::new(DeflateParams {
        level: level as i32,
        window_bits: window as u32,
        mem_level: memory as u32,
        strategy,
        wrap,
    })
    .map_err(|_| ZlibError::Warning("Failed allocating zlib.deflate context".into()))?;
    if let Some(dict) = dict {
        // zlib refuses a dictionary for gzip streams; PHP goes on without it
        // (observed on PHP 8.5).
        let _ = d.set_dictionary(&dict);
    }
    Ok(DeflateContext { d })
}

/// PHP's first guess at the output size for `n` input bytes (observed on
/// PHP 8.5 through stored blocks: `(size_t)(n * 1.015) + 23`).
fn buffer_guess(n: usize) -> usize {
    ((n as f64) * 1.015) as usize + 23
}

impl DeflateContext {
    /// `deflate_add(DeflateContext $context, string $data, int $flush_mode = ZLIB_SYNC_FLUSH)`.
    pub fn add(&mut self, data: &[u8], flush_mode: i64) -> Result<Vec<u8>, ZlibError> {
        let flush = Flush::from_i64(flush_mode).ok_or_else(|| {
            ZlibError::ValueError(format!("deflate_add(): Argument #3 {FLUSH_ERROR}"))
        })?;
        let mut out = Vec::new();
        if data.is_empty() && flush != Flush::Finish {
            return Ok(out);
        }
        let mut input = data;
        let mut avail = buffer_guess(data.len()).max(64);
        out.reserve(avail);
        loop {
            if avail == 0 {
                avail = 64;
            }
            let st = self.d.deflate_step(&mut input, flush, &mut out, &mut avail);
            match st {
                DeflateStatus::Ok if avail == 0 => continue,
                DeflateStatus::StreamEnd => {
                    self.d.reset();
                    break;
                }
                _ => break,
            }
        }
        Ok(out)
    }
}

// ----------------------------------------------------------------- inflate

/// An `InflateContext` object's state.
pub struct InflateContext {
    inf: Inflater,
    dict: Option<Vec<u8>>,
    raw: bool,
    status: InflateStatus,
}

/// `inflate_init(int $encoding, array|object $options = [])`.
///
/// The window is checked first, then the dictionary, then the encoding
/// (observed on PHP 8.5).
pub fn inflate_init(encoding: i64, opts: &InflateOptions) -> Result<InflateContext, ZlibError> {
    let window = opts.window.unwrap_or(15);
    if !(8..=15).contains(&window) {
        return Err(ZlibError::ValueError(format!(
            "zlib window size (logarithm) ({window}) must be within 8..15"
        )));
    }
    let dict = dictionary("inflate_init", &opts.dictionary)?;
    let wrap = match encoding {
        ENCODING_RAW => InflateWrap::Raw,
        ENCODING_DEFLATE => InflateWrap::Zlib,
        ENCODING_GZIP => InflateWrap::Gzip,
        _ => {
            return Err(ZlibError::ValueError(
                "Encoding mode must be ZLIB_ENCODING_RAW, ZLIB_ENCODING_GZIP or ZLIB_ENCODING_DEFLATE".into(),
            ));
        }
    };
    let mut inf = Inflater::new(wrap, window as u32);
    let raw = wrap == InflateWrap::Raw;
    if raw && let Some(d) = &dict {
        let _ = inf.set_dictionary(d);
    }
    Ok(InflateContext {
        inf,
        dict,
        raw,
        status: InflateStatus::Ok,
    })
}

impl InflateContext {
    /// `inflate_add(InflateContext $context, string $data, int $flush_mode = ZLIB_SYNC_FLUSH)`.
    ///
    /// A finished stream starts over with the next call; input left over
    /// when a call stops (end of stream, or a block boundary with
    /// `ZLIB_BLOCK`) is dropped, as PHP 8.5 does.
    pub fn add(&mut self, data: &[u8], flush_mode: i64) -> Result<Vec<u8>, ZlibError> {
        if !(0..=5).contains(&flush_mode) {
            return Err(ZlibError::ValueError(format!(
                "inflate_add(): Argument #3 {FLUSH_ERROR}"
            )));
        }
        if self.status == InflateStatus::StreamEnd {
            self.inf.reset();
            if self.raw
                && let Some(d) = &self.dict
            {
                let _ = self.inf.set_dictionary(d);
            }
            self.status = InflateStatus::Ok;
        }
        let flush = match flush_mode {
            4 => InflateFlush::Finish,
            5 => InflateFlush::Block,
            _ => InflateFlush::None,
        };
        let mut out = Vec::new();
        if data.is_empty() && flush != InflateFlush::Finish {
            return Ok(out);
        }
        let mut input = data;
        loop {
            let (n, st) = self.inf.inflate(input, &mut out, usize::MAX, flush);
            input = &input[n..];
            self.status = st;
            match st {
                InflateStatus::NeedDict => {
                    let Some(d) = &self.dict else {
                        return Err(ZlibError::Warning(
                            "Inflating this data requires a preset dictionary, please specify it in the options array of inflate_init()".into(),
                        ));
                    };
                    if self.inf.set_dictionary(d).is_err() {
                        return Err(ZlibError::Warning(
                            "Dictionary does not match expected dictionary (incorrect adler32 hash)"
                                .into(),
                        ));
                    }
                }
                InflateStatus::DataError(_) => {
                    return Err(ZlibError::Warning("data error".into()));
                }
                _ => return Ok(out),
            }
        }
    }

    /// `inflate_get_status()`: the last zlib return code.
    pub fn status(&self) -> i64 {
        self.status.code()
    }

    /// `inflate_get_read_len()`: input bytes used since the stream started.
    pub fn read_len(&self) -> i64 {
        self.inf.total_in() as i64
    }
}
