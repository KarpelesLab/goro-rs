use goro_core::array::PhpArray;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

use flate2::read::{DeflateDecoder, GzDecoder, ZlibDecoder};
use flate2::write::{DeflateEncoder, GzEncoder, ZlibEncoder};
use flate2::{Compress, Compression, Decompress, FlushCompress, FlushDecompress, Status};
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::rc::Rc;

pub fn register(vm: &mut Vm) {
    vm.register_extension(b"zlib");
    vm.register_function(b"gzcompress", gzcompress);
    vm.register_function(b"gzuncompress", gzuncompress);
    vm.register_function(b"gzdeflate", gzdeflate);
    vm.register_function(b"gzinflate", gzinflate);
    vm.register_function(b"gzencode", gzencode);
    vm.register_function(b"gzdecode", gzdecode);
    vm.register_function(b"zlib_encode", zlib_encode);
    vm.register_function(b"zlib_decode", zlib_decode);
    vm.register_function(b"zlib_get_coding_type", zlib_get_coding_type);
    vm.register_function(b"ob_gzhandler", ob_gzhandler_fn);

    // gzopen family
    vm.register_function(b"gzopen", gzopen_fn);
    vm.register_function(b"gzclose", gzclose_fn);
    vm.register_function(b"gzread", gzread_fn);
    vm.register_function(b"gzwrite", gzwrite_fn);
    vm.register_function(b"gzputs", gzwrite_fn); // alias
    vm.register_function(b"gzgets", gzgets_fn);
    vm.register_function(b"gzgetc", gzgetc_fn);
    vm.register_function(b"gzeof", gzeof_fn);
    vm.register_function(b"gzrewind", gzrewind_fn);
    vm.register_function(b"gzseek", gzseek_fn);
    vm.register_function(b"gztell", gztell_fn);
    vm.register_function(b"gzpassthru", gzpassthru_fn);
    vm.register_function(b"gzfile", gzfile_fn);
    vm.register_function(b"readgzfile", readgzfile_fn);

    // deflate_init/add, inflate_init/add
    vm.register_function(b"deflate_init", deflate_init_fn);
    vm.register_function(b"deflate_add", deflate_add_fn);
    vm.register_function(b"inflate_init", inflate_init_fn);
    vm.register_function(b"inflate_add", inflate_add_fn);
    vm.register_function(b"inflate_get_read_len", inflate_get_read_len_fn);
    vm.register_function(b"inflate_get_status", inflate_get_status_fn);

    vm.builtin_param_names.insert(b"gzcompress".to_vec(), vec![b"data".to_vec(), b"level".to_vec(), b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"gzuncompress".to_vec(), vec![b"data".to_vec(), b"max_length".to_vec()]);
    vm.builtin_param_names.insert(b"gzdeflate".to_vec(), vec![b"data".to_vec(), b"level".to_vec(), b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"gzinflate".to_vec(), vec![b"data".to_vec(), b"max_length".to_vec()]);
    vm.builtin_param_names.insert(b"gzencode".to_vec(), vec![b"data".to_vec(), b"level".to_vec(), b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"gzdecode".to_vec(), vec![b"data".to_vec(), b"max_length".to_vec()]);
    vm.builtin_param_names.insert(b"zlib_encode".to_vec(), vec![b"data".to_vec(), b"encoding".to_vec(), b"level".to_vec()]);
    vm.builtin_param_names.insert(b"zlib_decode".to_vec(), vec![b"data".to_vec(), b"max_length".to_vec()]);
    vm.builtin_param_names.insert(b"zlib_get_coding_type".to_vec(), vec![]);

    vm.builtin_param_names.insert(b"gzopen".to_vec(), vec![b"filename".to_vec(), b"mode".to_vec(), b"use_include_path".to_vec()]);
    vm.builtin_param_names.insert(b"gzclose".to_vec(), vec![b"stream".to_vec()]);
    vm.builtin_param_names.insert(b"gzread".to_vec(), vec![b"stream".to_vec(), b"length".to_vec()]);
    vm.builtin_param_names.insert(b"gzwrite".to_vec(), vec![b"stream".to_vec(), b"data".to_vec(), b"length".to_vec()]);
    vm.builtin_param_names.insert(b"gzputs".to_vec(), vec![b"stream".to_vec(), b"data".to_vec(), b"length".to_vec()]);
    vm.builtin_param_names.insert(b"gzgets".to_vec(), vec![b"stream".to_vec(), b"length".to_vec()]);
    vm.builtin_param_names.insert(b"gzgetc".to_vec(), vec![b"stream".to_vec()]);
    vm.builtin_param_names.insert(b"gzeof".to_vec(), vec![b"stream".to_vec()]);
    vm.builtin_param_names.insert(b"gzrewind".to_vec(), vec![b"stream".to_vec()]);
    vm.builtin_param_names.insert(b"gzseek".to_vec(), vec![b"stream".to_vec(), b"offset".to_vec(), b"whence".to_vec()]);
    vm.builtin_param_names.insert(b"gztell".to_vec(), vec![b"stream".to_vec()]);
    vm.builtin_param_names.insert(b"gzpassthru".to_vec(), vec![b"stream".to_vec()]);
    vm.builtin_param_names.insert(b"gzfile".to_vec(), vec![b"filename".to_vec(), b"use_include_path".to_vec()]);
    vm.builtin_param_names.insert(b"readgzfile".to_vec(), vec![b"filename".to_vec(), b"use_include_path".to_vec()]);

    vm.builtin_param_names.insert(b"deflate_init".to_vec(), vec![b"encoding".to_vec(), b"options".to_vec()]);
    vm.builtin_param_names.insert(b"deflate_add".to_vec(), vec![b"context".to_vec(), b"data".to_vec(), b"flush_mode".to_vec()]);
    vm.builtin_param_names.insert(b"inflate_init".to_vec(), vec![b"encoding".to_vec(), b"options".to_vec()]);
    vm.builtin_param_names.insert(b"inflate_add".to_vec(), vec![b"context".to_vec(), b"data".to_vec(), b"flush_mode".to_vec()]);
    vm.builtin_param_names.insert(b"inflate_get_read_len".to_vec(), vec![b"context".to_vec()]);
    vm.builtin_param_names.insert(b"inflate_get_status".to_vec(), vec![b"context".to_vec()]);

    vm.constants.insert(b"ZLIB_ENCODING_RAW".to_vec(), Value::Long(-15));
    vm.constants.insert(b"ZLIB_ENCODING_DEFLATE".to_vec(), Value::Long(15));
    vm.constants.insert(b"ZLIB_ENCODING_GZIP".to_vec(), Value::Long(31));
    vm.constants.insert(b"FORCE_GZIP".to_vec(), Value::Long(31));
    vm.constants.insert(b"FORCE_DEFLATE".to_vec(), Value::Long(15));
    vm.constants.insert(b"ZLIB_NO_FLUSH".to_vec(), Value::Long(0));
    vm.constants.insert(b"ZLIB_PARTIAL_FLUSH".to_vec(), Value::Long(1));
    vm.constants.insert(b"ZLIB_SYNC_FLUSH".to_vec(), Value::Long(2));
    vm.constants.insert(b"ZLIB_FULL_FLUSH".to_vec(), Value::Long(3));
    vm.constants.insert(b"ZLIB_FINISH".to_vec(), Value::Long(4));
    vm.constants.insert(b"ZLIB_BLOCK".to_vec(), Value::Long(5));
    vm.constants.insert(b"ZLIB_OK".to_vec(), Value::Long(0));
    vm.constants.insert(b"ZLIB_STREAM_END".to_vec(), Value::Long(1));
    vm.constants.insert(b"ZLIB_NEED_DICT".to_vec(), Value::Long(2));
    vm.constants.insert(b"ZLIB_ERRNO".to_vec(), Value::Long(-1));
    vm.constants.insert(b"ZLIB_STREAM_ERROR".to_vec(), Value::Long(-2));
    vm.constants.insert(b"ZLIB_DATA_ERROR".to_vec(), Value::Long(-3));
    vm.constants.insert(b"ZLIB_MEM_ERROR".to_vec(), Value::Long(-4));
    vm.constants.insert(b"ZLIB_BUF_ERROR".to_vec(), Value::Long(-5));
    vm.constants.insert(b"ZLIB_VERSION_ERROR".to_vec(), Value::Long(-6));
    vm.constants.insert(b"ZLIB_FILTERED".to_vec(), Value::Long(1));
    vm.constants.insert(b"ZLIB_HUFFMAN_ONLY".to_vec(), Value::Long(2));
    vm.constants.insert(b"ZLIB_RLE".to_vec(), Value::Long(3));
    vm.constants.insert(b"ZLIB_FIXED".to_vec(), Value::Long(4));
    vm.constants.insert(b"ZLIB_DEFAULT_STRATEGY".to_vec(), Value::Long(0));
    // ZLIB_VERNUM: zlib 1.2.11 = 0x12b0
    vm.constants.insert(b"ZLIB_VERNUM".to_vec(), Value::Long(0x12b0));
    vm.constants.insert(b"ZLIB_VERSION".to_vec(), Value::String(PhpString::from_bytes(b"1.2.11")));
}

fn comp(level: i64) -> Compression {
    if level == -1 { Compression::default() } else { Compression::new(level.clamp(0, 9) as u32) }
}

fn validate_level(vm: &mut Vm, func_name: &str, level: i64) -> Result<bool, VmError> {
    if !(-1..=9).contains(&level) {
        let msg = format!("{}(): Argument #2 ($level) must be between -1 and 9", func_name);
        let exc = vm.create_exception(b"ValueError", &msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    Ok(true)
}

fn validate_encoding(vm: &mut Vm, func_name: &str, encoding: i64, arg_num: u32) -> Result<bool, VmError> {
    if encoding != 31 && encoding != 15 && encoding != -15 {
        let msg = format!("{}(): Argument #{} ($encoding) must be one of ZLIB_ENCODING_RAW, ZLIB_ENCODING_GZIP, or ZLIB_ENCODING_DEFLATE", func_name, arg_num);
        let exc = vm.create_exception(b"ValueError", &msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    Ok(true)
}

fn read_limited(r: &mut dyn Read, max: i64) -> Result<Vec<u8>, std::io::Error> {
    if max > 0 {
        // Read up to max bytes
        let mut buf = vec![0u8; max as usize];
        let mut n = 0;
        while n < buf.len() {
            match r.read(&mut buf[n..]) {
                Ok(0) => break,
                Ok(k) => n += k,
                Err(e) => return Err(e),
            }
        }
        buf.truncate(n);
        Ok(buf)
    } else {
        let mut buf = Vec::new();
        r.read_to_end(&mut buf)?;
        Ok(buf)
    }
}

fn gzcompress(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = args.first().unwrap_or(&Value::Null).to_php_string();
    let level = args.get(1).map(|v| v.to_long()).unwrap_or(-1);
    validate_level(vm, "gzcompress", level)?;
    if args.len() >= 3 {
        let encoding = args[2].to_long();
        validate_encoding(vm, "gzcompress", encoding, 3)?;
    }
    let mut enc = ZlibEncoder::new(Vec::new(), comp(level));
    enc.write_all(data.as_bytes()).ok();
    match enc.finish() {
        Ok(c) => Ok(Value::String(PhpString::from_vec(c))),
        Err(_) => Ok(Value::False),
    }
}

fn gzuncompress(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = args.first().unwrap_or(&Value::Null).to_php_string();
    let max = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    if max < 0 {
        let msg = "gzuncompress(): Argument #2 ($max_length) must be greater than or equal to 0";
        let exc = vm.create_exception(b"ValueError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg.to_string(), line: vm.current_line });
    }
    let bytes = data.as_bytes();
    if bytes.is_empty() || bytes.len() < 2 {
        vm.emit_warning("gzuncompress(): data error");
        return Ok(Value::False);
    }
    // Use streaming Decompress to detect "insufficient memory" (max_length too small)
    decompress_with_limit(vm, bytes, max, "gzuncompress", true, false)
}

fn gzdeflate(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = args.first().unwrap_or(&Value::Null).to_php_string();
    let level = args.get(1).map(|v| v.to_long()).unwrap_or(-1);
    validate_level(vm, "gzdeflate", level)?;
    if args.len() >= 3 {
        let encoding = args[2].to_long();
        validate_encoding(vm, "gzdeflate", encoding, 3)?;
    }
    let mut enc = DeflateEncoder::new(Vec::new(), comp(level));
    enc.write_all(data.as_bytes()).ok();
    match enc.finish() {
        Ok(c) => Ok(Value::String(PhpString::from_vec(c))),
        Err(_) => Ok(Value::False),
    }
}

fn gzinflate(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = args.first().unwrap_or(&Value::Null).to_php_string();
    let max = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    if max < 0 {
        let msg = "gzinflate(): Argument #2 ($max_length) must be greater than or equal to 0";
        let exc = vm.create_exception(b"ValueError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg.to_string(), line: vm.current_line });
    }
    let bytes = data.as_bytes();
    if bytes.is_empty() {
        vm.emit_warning("gzinflate(): data error");
        return Ok(Value::False);
    }
    decompress_with_limit(vm, bytes, max, "gzinflate", false, false)
}

/// Decompress with optional limit. zlib_header=true means zlib stream, false=raw deflate.
/// Returns warning + false on errors. If max_length is too small to fit decompressed data,
/// returns "insufficient memory" warning.
fn decompress_with_limit(vm: &mut Vm, input: &[u8], max: i64, fname: &str, zlib_header: bool, _gzip: bool) -> Result<Value, VmError> {
    let mut dec = Decompress::new(zlib_header);
    let mut out = Vec::new();
    let max_out = if max > 0 { max as usize } else { usize::MAX };
    let mut total_in = 0;
    let mut total_out = 0;
    loop {
        // If we've already filled the buffer, the next byte would overflow
        // unless the stream is at its end. Do one more probe with empty output.
        let space = if max_out == usize::MAX {
            64 * 1024
        } else {
            max_out.saturating_sub(total_out).min(64 * 1024)
        };
        if space == 0 {
            // Try one decompress with 0 output to see if stream is ended; if not, fail.
            let in_buf = &input[total_in..];
            let mut tmp = [0u8; 0];
            let res = dec.decompress(in_buf, &mut tmp, FlushDecompress::None);
            match res {
                Ok(Status::StreamEnd) => return Ok(Value::String(PhpString::from_vec(out))),
                _ => {
                    vm.emit_warning(&format!("{}(): insufficient memory", fname));
                    return Ok(Value::False);
                }
            }
        }
        let cur_len = out.len();
        out.resize(cur_len + space, 0);
        let in_buf = &input[total_in..];
        let in_before = dec.total_in();
        let out_before = dec.total_out();
        let res = dec.decompress(in_buf, &mut out[cur_len..], FlushDecompress::None);
        let consumed = (dec.total_in() - in_before) as usize;
        let produced = (dec.total_out() - out_before) as usize;
        total_in += consumed;
        total_out += produced;
        out.truncate(cur_len + produced);
        match res {
            Ok(Status::StreamEnd) => {
                return Ok(Value::String(PhpString::from_vec(out)));
            }
            Ok(Status::Ok) => {
                if total_in >= input.len() && produced == 0 {
                    vm.emit_warning(&format!("{}(): data error", fname));
                    return Ok(Value::False);
                }
                continue;
            }
            Ok(Status::BufError) => {
                if total_out >= max_out {
                    vm.emit_warning(&format!("{}(): insufficient memory", fname));
                    return Ok(Value::False);
                }
                if total_in >= input.len() {
                    vm.emit_warning(&format!("{}(): data error", fname));
                    return Ok(Value::False);
                }
                continue;
            }
            Err(_) => {
                vm.emit_warning(&format!("{}(): data error", fname));
                return Ok(Value::False);
            }
        }
    }
}

fn gzencode(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = args.first().unwrap_or(&Value::Null).to_php_string();
    let level = args.get(1).map(|v| v.to_long()).unwrap_or(-1);
    validate_level(vm, "gzencode", level)?;
    let encoding = args.get(2).map(|v| v.to_long()).unwrap_or(31);
    if args.len() >= 3 {
        validate_encoding(vm, "gzencode", encoding, 3)?;
    }
    let c = comp(level);
    let b = data.as_bytes();
    let result: Result<Vec<u8>, _> = match encoding {
        31 => { let mut e = GzEncoder::new(Vec::new(), c); e.write_all(b).ok(); e.finish() }
        15 => { let mut e = ZlibEncoder::new(Vec::new(), c); e.write_all(b).ok(); e.finish() }
        -15 => { let mut e = DeflateEncoder::new(Vec::new(), c); e.write_all(b).ok(); e.finish() }
        _ => return Ok(Value::False),
    };
    match result {
        Ok(c) => Ok(Value::String(PhpString::from_vec(c))),
        Err(_) => Ok(Value::False),
    }
}

fn gzdecode(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = args.first().unwrap_or(&Value::Null).to_php_string();
    let max = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let bytes = data.as_bytes();
    if bytes.is_empty() {
        vm.emit_warning("gzdecode(): data error");
        return Ok(Value::False);
    }
    let mut dec = GzDecoder::new(bytes);
    match read_limited(&mut dec, max) {
        Ok(d) => Ok(Value::String(PhpString::from_vec(d))),
        Err(_) => { vm.emit_warning("gzdecode(): data error"); Ok(Value::False) }
    }
}

fn zlib_encode(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = args.first().unwrap_or(&Value::Null).to_php_string();
    let encoding = args.get(1).map(|v| v.to_long()).unwrap_or(15);
    validate_encoding(vm, "zlib_encode", encoding, 2)?;
    let level = args.get(2).map(|v| v.to_long()).unwrap_or(-1);
    if args.len() >= 3 {
        validate_level(vm, "zlib_encode", level)?;
    }
    let c = comp(level);
    let b = data.as_bytes();
    let result: Result<Vec<u8>, _> = match encoding {
        31 => { let mut e = GzEncoder::new(Vec::new(), c); e.write_all(b).ok(); e.finish() }
        15 => { let mut e = ZlibEncoder::new(Vec::new(), c); e.write_all(b).ok(); e.finish() }
        -15 => { let mut e = DeflateEncoder::new(Vec::new(), c); e.write_all(b).ok(); e.finish() }
        _ => return Ok(Value::False),
    };
    match result {
        Ok(c) => Ok(Value::String(PhpString::from_vec(c))),
        Err(_) => Ok(Value::False),
    }
}

fn zlib_decode(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = args.first().unwrap_or(&Value::Null).to_php_string();
    let max = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let b = data.as_bytes();
    if b.is_empty() {
        vm.emit_warning("zlib_decode(): data error");
        return Ok(Value::False);
    }
    // Try gzip
    if b.len() >= 2 && b[0] == 0x1f && b[1] == 0x8b {
        if let Ok(d) = read_limited(&mut GzDecoder::new(b), max) { return Ok(Value::String(PhpString::from_vec(d))); }
    }
    // Try zlib
    if b.len() >= 2 && (b[0] & 0x0F) == 8 {
        if let Ok(d) = read_limited(&mut ZlibDecoder::new(b), max) { return Ok(Value::String(PhpString::from_vec(d))); }
    }
    // Try raw deflate
    if let Ok(d) = read_limited(&mut DeflateDecoder::new(b), max) {
        // Heuristic: empty result without valid data means failure
        if !d.is_empty() || b.len() >= 2 {
            return Ok(Value::String(PhpString::from_vec(d)));
        }
    }
    vm.emit_warning("zlib_decode(): data error");
    Ok(Value::False)
}

fn zlib_get_coding_type(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // In CLI mode, no content encoding is active
    Ok(Value::False)
}

fn ob_gzhandler_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // ob_gzhandler($data, $flags) - we don't actually compress, just pass through
    // since we have no way to know the Accept-Encoding header
    let data = args.first().unwrap_or(&Value::Null).to_php_string();
    Ok(Value::String(data))
}

// ========== gzopen family ==========

/// GzHandle represents an opened gzip stream (for reading or writing).
enum GzHandle {
    Reader {
        /// The decompressed data buffer
        decompressed: Vec<u8>,
        /// Current position in decompressed data
        pos: usize,
        /// Whether we're at EOF
        eof: bool,
        /// Whether this is the writer mode handle
        #[allow(dead_code)]
        is_writer: bool,
    },
    Writer {
        /// Path to write to
        path: String,
        /// Accumulated uncompressed data
        buffer: Vec<u8>,
        /// Current logical position (allowing seeks past end)
        pos: usize,
        /// Compression level
        level: Compression,
        /// Whether the file should be plain text (mode contains no compression and is plain).
        /// For now we always gzip-encode on close.
        plain: bool,
    },
}

thread_local! {
    static GZ_HANDLES: RefCell<HashMap<i64, GzHandle>> = RefCell::new(HashMap::new());
    static NEXT_GZ_ID: std::cell::Cell<i64> = const { std::cell::Cell::new(10000) };
    // deflate/inflate contexts
    static DEFLATE_CONTEXTS: RefCell<HashMap<i64, DeflateContext>> = RefCell::new(HashMap::new());
    static INFLATE_CONTEXTS: RefCell<HashMap<i64, InflateContext>> = RefCell::new(HashMap::new());
    static NEXT_CTX_ID: std::cell::Cell<i64> = const { std::cell::Cell::new(20000) };
}

fn alloc_gz_id() -> i64 {
    NEXT_GZ_ID.with(|id| {
        let gid = id.get();
        id.set(gid + 1);
        gid
    })
}

fn gzopen_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let filename = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy();
    let mode_str = args.get(1).map(|v| v.to_php_string().to_string_lossy()).unwrap_or_else(|| "r".to_string());

    // php://output and similar non-seekable streams are not supported by zlib wrapper.
    if filename.starts_with("php://output") || filename.starts_with("php://stdout") || filename.starts_with("php://stderr") || filename.starts_with("php://input") {
        vm.emit_warning(&format!("gzopen({}): could not make seekable - {}", filename, filename));
        return Ok(Value::False);
    }

    // Parse mode: extract r/w and optional compression level
    let mode_clean: String = mode_str.chars().filter(|&c| c != 'b' && c != 't' && c != 'f' && c != 'h').collect();

    // Check for r+/w+ (not allowed for gzip)
    if mode_clean.contains('+') {
        vm.emit_warning("gzopen(): Cannot open a zlib stream for reading and writing at the same time!");
        return Ok(Value::False);
    }

    // Strip 'e' (close-on-exec) modifier when computing r/w. PHP treats the
    // pure 'e' mode as invalid for fopen, producing a "Failed to open stream"
    // warning with the actual file name (because PHP defers to fopen's mode
    // parser). Other invalid modes (e.g. 'c') get a "gzopen failed" warning.
    let stripped_e: String = mode_clean.chars().filter(|&c| c != 'e').collect();
    let had_e = stripped_e.len() != mode_clean.len();
    let is_read = stripped_e.starts_with('r');
    let is_write = stripped_e.starts_with('w') || stripped_e.starts_with('a') || stripped_e.starts_with('x');

    if !is_read && !is_write {
        if had_e {
            vm.emit_warning(&format!("gzopen({}): Failed to open stream: `{}' is not a valid mode for fopen", filename, mode_str));
        } else {
            vm.emit_warning("gzopen(): gzopen failed");
        }
        return Ok(Value::False);
    }

    // Extract compression level from mode string (e.g., "w9", "w1")
    let level_from_mode: i64 = mode_clean.chars()
        .find(|c| c.is_ascii_digit())
        .map(|c| c as i64 - '0' as i64)
        .unwrap_or(-1);

    if is_read {
        match std::fs::read(&filename) {
            Ok(data) => {
                // Check if gzipped data
                let is_gz = data.len() >= 2 && data[0] == 0x1f && data[1] == 0x8b;
                let gid = alloc_gz_id();
                let decompressed = if is_gz {
                    let mut dec = GzDecoder::new(data.as_slice());
                    let mut buf = Vec::new();
                    if dec.read_to_end(&mut buf).is_err() {
                        // Fall back to raw data
                        buf = data;
                    }
                    buf
                } else {
                    data
                };
                GZ_HANDLES.with(|handles| {
                    handles.borrow_mut().insert(gid, GzHandle::Reader {
                        decompressed,
                        pos: 0,
                        eof: false,
                        is_writer: false,
                    });
                });
                goro_core::value::register_resource(gid, b"stream");
                goro_core::value::register_close_hook(gid, Box::new(move || {
                    GZ_HANDLES.with(|handles| { handles.borrow_mut().remove(&gid); });
                }));
                Ok(Value::Resource(gid))
            }
            Err(_) => {
                vm.emit_warning(&format!("gzopen({}): Failed to open stream: No such file or directory", filename));
                Ok(Value::False)
            }
        }
    } else {
        // Write mode - check parent dir exists
        if let Some(parent) = std::path::Path::new(&filename).parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                vm.emit_warning(&format!("gzopen({}): Failed to open stream: No such file or directory", filename));
                return Ok(Value::False);
            }
        }
        let gid = alloc_gz_id();
        GZ_HANDLES.with(|handles| {
            handles.borrow_mut().insert(gid, GzHandle::Writer {
                path: filename.to_string(),
                buffer: Vec::new(),
                pos: 0,
                level: comp(level_from_mode),
                plain: level_from_mode == 0,
            });
        });
        goro_core::value::register_resource(gid, b"stream");
        // Register a close hook so fclose() flushes the gz writer to disk.
        goro_core::value::register_close_hook(gid, Box::new(move || {
            flush_gz_handle(gid);
        }));
        Ok(Value::Resource(gid))
    }
}

/// Flush a gz writer handle to disk (called from gzclose or fclose hook).
fn flush_gz_handle(gid: i64) {
    let handle = GZ_HANDLES.with(|handles| handles.borrow_mut().remove(&gid));
    if let Some(handle) = handle {
        if let GzHandle::Writer { path, buffer, level, plain, .. } = handle {
            if plain {
                let _ = std::fs::write(&path, &buffer);
            } else {
                let mut enc = GzEncoder::new(Vec::new(), level);
                let _ = enc.write_all(&buffer);
                if let Ok(compressed) = enc.finish() {
                    let _ = std::fs::write(&path, &compressed);
                }
            }
        }
    }
}

fn require_open_resource(vm: &mut Vm, gid: i64, fname: &str) -> Result<bool, VmError> {
    if !goro_core::value::is_open_resource(gid) {
        let msg = format!("{}(): Argument #1 ($stream) must be an open stream resource", fname);
        let exc = vm.create_exception(b"TypeError", &msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    Ok(true)
}

fn gzclose_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let gid = args.first().unwrap_or(&Value::Null).to_long();
    // Was this a valid open gz resource?
    let was_open = goro_core::value::is_open_resource(gid)
        && GZ_HANDLES.with(|handles| handles.borrow().contains_key(&gid));
    if was_open {
        // close_resource will run the close hook (which flushes the writer).
        goro_core::value::close_resource(gid);
        Ok(Value::True)
    } else {
        Ok(Value::False)
    }
}

fn gzread_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let gid = args.first().unwrap_or(&Value::Null).to_long();
    require_open_resource(vm, gid, "gzread")?;
    let length = args.get(1).map(|v| v.to_long()).unwrap_or(0) as usize;
    if length == 0 {
        return Ok(Value::String(PhpString::from_bytes(b"")));
    }

    GZ_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(handle) = handles.get_mut(&gid) {
            if let GzHandle::Reader { decompressed, pos, eof, .. } = handle {
                if *pos >= decompressed.len() {
                    *eof = true;
                    return Ok(Value::String(PhpString::from_bytes(b"")));
                }
                let end = (*pos + length).min(decompressed.len());
                let data = decompressed[*pos..end].to_vec();
                *pos = end;
                if *pos >= decompressed.len() {
                    *eof = true;
                }
                Ok(Value::String(PhpString::from_vec(data)))
            } else {
                // Writer mode - read returns false
                Ok(Value::False)
            }
        } else {
            Ok(Value::False)
        }
    })
}

fn gzwrite_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let gid = args.first().unwrap_or(&Value::Null).to_long();
    require_open_resource(vm, gid, "gzwrite")?;
    let data = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let bytes = data.as_bytes();
    // length parameter: if specified and <= 0, write 0 bytes
    let length = if args.len() >= 3 {
        let l = args[2].to_long();
        if l <= 0 {
            return Ok(Value::Long(0));
        }
        (l as usize).min(bytes.len())
    } else {
        bytes.len()
    };
    let to_write = &bytes[..length];

    GZ_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(handle) = handles.get_mut(&gid) {
            if let GzHandle::Writer { buffer, pos, .. } = handle {
                // Pad with zeros if pos is past buffer end
                if *pos > buffer.len() {
                    buffer.resize(*pos, 0);
                }
                if *pos == buffer.len() {
                    buffer.extend_from_slice(to_write);
                } else {
                    // Overwrite at current position
                    let end = *pos + to_write.len();
                    if end > buffer.len() {
                        buffer.resize(end, 0);
                    }
                    buffer[*pos..end].copy_from_slice(to_write);
                }
                *pos += to_write.len();
                Ok(Value::Long(to_write.len() as i64))
            } else {
                // Reader mode - write does nothing, returns 0
                Ok(Value::Long(0))
            }
        } else {
            Ok(Value::False)
        }
    })
}

fn gzgets_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let gid = args.first().unwrap_or(&Value::Null).to_long();
    require_open_resource(vm, gid, "gzgets")?;
    let max_length = args.get(1).map(|v| v.to_long() as usize);

    GZ_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(handle) = handles.get_mut(&gid) {
            if let GzHandle::Reader { decompressed, pos, eof, .. } = handle {
                if *pos >= decompressed.len() {
                    *eof = true;
                    return Ok(Value::False);
                }
                // Read until newline or max_length - 1 bytes (length includes null terminator in PHP)
                let limit = match max_length {
                    Some(l) if l > 0 => l - 1,
                    _ => decompressed.len() - *pos,
                };
                let start = *pos;
                let mut count = 0;
                while *pos < decompressed.len() && count < limit {
                    let b = decompressed[*pos];
                    *pos += 1;
                    count += 1;
                    if b == b'\n' {
                        break;
                    }
                }
                if *pos >= decompressed.len() {
                    *eof = true;
                }
                let data = decompressed[start..*pos].to_vec();
                if data.is_empty() {
                    Ok(Value::False)
                } else {
                    Ok(Value::String(PhpString::from_vec(data)))
                }
            } else {
                Ok(Value::False)
            }
        } else {
            Ok(Value::False)
        }
    })
}

fn gzgetc_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let gid = args.first().unwrap_or(&Value::Null).to_long();
    require_open_resource(vm, gid, "gzgetc")?;

    GZ_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(handle) = handles.get_mut(&gid) {
            if let GzHandle::Reader { decompressed, pos, eof, .. } = handle {
                if *pos >= decompressed.len() {
                    *eof = true;
                    return Ok(Value::False);
                }
                let c = decompressed[*pos];
                *pos += 1;
                if *pos >= decompressed.len() {
                    *eof = true;
                }
                Ok(Value::String(PhpString::from_vec(vec![c])))
            } else {
                Ok(Value::False)
            }
        } else {
            Ok(Value::False)
        }
    })
}

fn gzeof_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let gid = args.first().unwrap_or(&Value::Null).to_long();
    require_open_resource(vm, gid, "gzeof")?;

    GZ_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(handle) = handles.get_mut(&gid) {
            match handle {
                GzHandle::Reader { decompressed, pos, eof, .. } => {
                    if *pos >= decompressed.len() {
                        *eof = true;
                    }
                    if *eof { Ok(Value::True) } else { Ok(Value::False) }
                }
                GzHandle::Writer { .. } => Ok(Value::False),
            }
        } else {
            Ok(Value::True)
        }
    })
}

fn gzrewind_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let gid = args.first().unwrap_or(&Value::Null).to_long();
    require_open_resource(vm, gid, "gzrewind")?;

    GZ_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(handle) = handles.get_mut(&gid) {
            match handle {
                GzHandle::Reader { pos, eof, .. } => {
                    *pos = 0;
                    *eof = false;
                    Ok(Value::True)
                }
                GzHandle::Writer { .. } => {
                    // Cannot rewind a write stream
                    Ok(Value::False)
                }
            }
        } else {
            Ok(Value::False)
        }
    })
}

fn gzseek_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let gid = args.first().unwrap_or(&Value::Null).to_long();
    require_open_resource(vm, gid, "gzseek")?;
    let offset = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let whence = args.get(2).map(|v| v.to_long()).unwrap_or(0); // SEEK_SET

    if whence == 2 {
        // SEEK_END - not supported for gzip streams
        vm.emit_warning("gzseek(): SEEK_END is not supported");
        return Ok(Value::Long(-1));
    }

    GZ_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(handle) = handles.get_mut(&gid) {
            match handle {
                GzHandle::Reader { decompressed, pos, eof, .. } => {
                    let new_pos = match whence {
                        0 => offset,
                        1 => *pos as i64 + offset,
                        _ => return Ok(Value::Long(-1)),
                    };
                    if new_pos < 0 {
                        return Ok(Value::Long(-1));
                    }
                    *pos = new_pos as usize;
                    if *pos > decompressed.len() {
                        *pos = decompressed.len();
                    }
                    *eof = false;
                    Ok(Value::Long(0))
                }
                GzHandle::Writer { pos, buffer, .. } => {
                    let new_pos = match whence {
                        0 => offset,
                        1 => *pos as i64 + offset,
                        _ => return Ok(Value::Long(-1)),
                    };
                    if new_pos < 0 {
                        return Ok(Value::Long(-1));
                    }
                    // Pad with zeros if seeking past end
                    let new_pos = new_pos as usize;
                    if new_pos > buffer.len() {
                        buffer.resize(new_pos, 0);
                    }
                    *pos = new_pos;
                    Ok(Value::Long(0))
                }
            }
        } else {
            Ok(Value::Long(-1))
        }
    })
}

fn gztell_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let gid = args.first().unwrap_or(&Value::Null).to_long();
    require_open_resource(vm, gid, "gztell")?;

    GZ_HANDLES.with(|handles| {
        let handles = handles.borrow();
        if let Some(handle) = handles.get(&gid) {
            match handle {
                GzHandle::Reader { pos, .. } => Ok(Value::Long(*pos as i64)),
                GzHandle::Writer { pos, .. } => Ok(Value::Long(*pos as i64)),
            }
        } else {
            Ok(Value::False)
        }
    })
}

fn gzpassthru_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let gid = args.first().unwrap_or(&Value::Null).to_long();
    require_open_resource(vm, gid, "gzpassthru")?;

    let data = GZ_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(handle) = handles.get_mut(&gid) {
            if let GzHandle::Reader { decompressed, pos, eof, .. } = handle {
                if *pos >= decompressed.len() {
                    *eof = true;
                    return None;
                }
                let remaining = decompressed[*pos..].to_vec();
                *pos = decompressed.len();
                *eof = true;
                Some(remaining)
            } else {
                None
            }
        } else {
            None
        }
    });

    if let Some(d) = data {
        let len = d.len();
        vm.write_output(&d);
        Ok(Value::Long(len as i64))
    } else {
        Ok(Value::Long(0))
    }
}

fn gzfile_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let filename = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy();

    let data = match std::fs::read(&filename) {
        Ok(d) => d,
        Err(_) => {
            vm.emit_warning(&format!("gzfile({}): Failed to open stream: No such file or directory", filename));
            return Ok(Value::False);
        }
    };

    // Decompress if gzipped
    let decompressed = if data.len() >= 2 && data[0] == 0x1f && data[1] == 0x8b {
        let mut dec = GzDecoder::new(data.as_slice());
        let mut buf = Vec::new();
        match dec.read_to_end(&mut buf) {
            Ok(_) => buf,
            Err(_) => return Ok(Value::False),
        }
    } else {
        data
    };

    // Split into lines (preserving line endings like PHP's file())
    let mut result = PhpArray::new();
    let mut start = 0;
    let len = decompressed.len();
    while start < len {
        // Find next newline
        let end = match decompressed[start..].iter().position(|&b| b == b'\n') {
            Some(pos) => start + pos + 1, // include the newline
            None => len,                   // no more newlines
        };
        result.push(Value::String(PhpString::from_vec(decompressed[start..end].to_vec())));
        start = end;
    }

    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn readgzfile_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let filename = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy();

    let data = match std::fs::read(&filename) {
        Ok(d) => d,
        Err(_) => {
            vm.emit_warning(&format!("readgzfile({}): Failed to open stream: No such file or directory", filename));
            return Ok(Value::False);
        }
    };

    // Decompress if gzipped
    let decompressed = if data.len() >= 2 && data[0] == 0x1f && data[1] == 0x8b {
        let mut dec = GzDecoder::new(data.as_slice());
        let mut buf = Vec::new();
        match dec.read_to_end(&mut buf) {
            Ok(_) => buf,
            Err(_) => return Ok(Value::False),
        }
    } else {
        data
    };

    let len = decompressed.len() as i64;
    vm.write_output(&decompressed);
    Ok(Value::Long(len))
}

// ========== deflate_init / deflate_add / inflate_init / inflate_add ==========

struct DeflateContext {
    #[allow(dead_code)]
    encoding: i64,
    compress: Compress,
    finished: bool,
}

struct InflateContext {
    encoding: i64,
    decompress: Decompress,
    total_read: usize,
    status: i64, // 0 = OK, 1 = STREAM_END
    #[allow(dead_code)]
    dictionary: Option<Vec<u8>>,
    #[allow(dead_code)]
    pending_dict_apply: bool,
    /// For gzip encoding (31), we manually parse the gzip header and feed
    /// the rest as raw deflate. This stores the buffer until header is parsed.
    gz_header_buf: Vec<u8>,
    gz_header_done: bool,
    /// Bytes consumed for header (counted toward total_in)
    gz_header_consumed: usize,
    /// Bytes consumed by trailer
    gz_trailer_remaining: usize,
}

fn alloc_ctx_id() -> i64 {
    NEXT_CTX_ID.with(|id| {
        let cid = id.get();
        id.set(cid + 1);
        cid
    })
}

fn make_compress(encoding: i64, level: Compression) -> Compress {
    match encoding {
        15 => Compress::new(level, true),
        -15 => Compress::new(level, false),
        31 => {
            // GZIP: miniz_oxide has no gzip window bits here, so fall back to
            // new with zlib_header=true (zlib header).
            Compress::new(level, true)
        }
        _ => Compress::new(level, true),
    }
}

fn make_decompress(encoding: i64) -> Decompress {
    match encoding {
        15 => Decompress::new(true),
        -15 => Decompress::new(false),
        // For gzip we treat the body as raw deflate (Decompress::new(false))
        // and handle the gzip header/trailer manually.
        31 => Decompress::new(false),
        _ => Decompress::new(true),
    }
}

/// Parse a gzip header from buf. Returns Some(header_length) if a complete
/// header has been consumed, or None if more bytes are needed. Returns
/// Some(0) on parse error indicator (but we just trust callers checked magic).
fn parse_gzip_header(buf: &[u8]) -> Option<usize> {
    if buf.len() < 10 {
        return None;
    }
    if buf[0] != 0x1f || buf[1] != 0x8b || buf[2] != 8 {
        return Some(0); // signal as error via 0
    }
    let flg = buf[3];
    let mut pos = 10;
    // FEXTRA
    if flg & 0x04 != 0 {
        if buf.len() < pos + 2 { return None; }
        let xlen = u16::from_le_bytes([buf[pos], buf[pos + 1]]) as usize;
        pos += 2;
        if buf.len() < pos + xlen { return None; }
        pos += xlen;
    }
    // FNAME
    if flg & 0x08 != 0 {
        let end = buf[pos..].iter().position(|&b| b == 0);
        {
            let e = end?;
            pos += e + 1
        }
    }
    // FCOMMENT
    if flg & 0x10 != 0 {
        let end = buf[pos..].iter().position(|&b| b == 0);
        {
            let e = end?;
            pos += e + 1
        }
    }
    // FHCRC
    if flg & 0x02 != 0 {
        if buf.len() < pos + 2 { return None; }
        pos += 2;
    }
    Some(pos)
}

/// Convert a PHP value into a dictionary byte vector for deflate/inflate.
/// PHP supports arrays of strings (concatenated with NUL terminators) or a single string.
/// Returns None on error.
fn extract_dictionary(vm: &mut Vm, fname: &str, value: &Value) -> Result<Option<Vec<u8>>, VmError> {
    match value {
        Value::Array(arr) => {
            let mut out = Vec::new();
            let arr = arr.borrow();
            for (_k, v) in arr.iter() {
                match v {
                    Value::Object(_) => {
                        let msg = "Object of class stdClass could not be converted to string";
                        let exc = vm.create_exception(b"Error", msg, 0);
                        vm.current_exception = Some(exc);
                        return Err(VmError { message: msg.to_string(), line: vm.current_line });
                    }
                    _ => {
                        let s = v.to_php_string();
                        let b = s.as_bytes();
                        if b.is_empty() {
                            let msg = format!("{}(): Argument #2 ($options) must not contain empty strings", fname);
                            let exc = vm.create_exception(b"ValueError", &msg, 0);
                            vm.current_exception = Some(exc);
                            return Err(VmError { message: msg, line: vm.current_line });
                        }
                        if b.contains(&0) {
                            let msg = format!("{}(): Argument #2 ($options) must not contain strings with null bytes", fname);
                            let exc = vm.create_exception(b"ValueError", &msg, 0);
                            vm.current_exception = Some(exc);
                            return Err(VmError { message: msg, line: vm.current_line });
                        }
                        out.extend_from_slice(b);
                        out.push(0);
                    }
                }
            }
            // Strip trailing null per PHP behavior (some versions)
            // Actually PHP joins with null separator, no trailing null
            if out.last() == Some(&0) {
                out.pop();
            }
            Ok(Some(out))
        }
        Value::String(s) => Ok(Some(s.as_bytes().to_vec())),
        Value::Null => Ok(None),
        _ => {
            let s = value.to_php_string();
            Ok(Some(s.as_bytes().to_vec()))
        }
    }
}

fn deflate_init_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let encoding = args.first().map(|v| v.to_long()).unwrap_or(15);
    if encoding != 31 && encoding != 15 && encoding != -15 {
        let msg = "deflate_init(): Argument #1 ($encoding) must be one of ZLIB_ENCODING_RAW, ZLIB_ENCODING_GZIP, or ZLIB_ENCODING_DEFLATE";
        let exc = vm.create_exception(b"ValueError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg.to_string(), line: vm.current_line });
    }

    let mut level = -1i64;
    let mut dictionary: Option<Vec<u8>> = None;
    if let Some(opts) = args.get(1) {
        let level_val: Option<Value>;
        let memory_val: Option<Value>;
        let dict_val: Option<Value>;
        match opts {
            Value::Array(arr) => {
                let arr = arr.borrow();
                level_val = arr.get(&goro_core::array::ArrayKey::String(PhpString::from_bytes(b"level"))).cloned();
                memory_val = arr.get(&goro_core::array::ArrayKey::String(PhpString::from_bytes(b"memory"))).cloned();
                dict_val = arr.get(&goro_core::array::ArrayKey::String(PhpString::from_bytes(b"dictionary"))).cloned();
            }
            Value::Object(obj) => {
                let obj = obj.borrow();
                let find = |name: &[u8]| obj.properties.iter().find(|(k, _)| k.as_slice() == name).map(|(_, v)| v.clone());
                level_val = find(b"level");
                memory_val = find(b"memory");
                dict_val = find(b"dictionary");
            }
            _ => {
                level_val = None;
                memory_val = None;
                dict_val = None;
            }
        }
        if let Some(v) = level_val {
            level = v.to_long();
            if !(-1..=9).contains(&level) {
                let msg = "deflate_init(): \"level\" option must be between -1 and 9";
                let exc = vm.create_exception(b"ValueError", msg, 0);
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg.to_string(), line: vm.current_line });
            }
        }
        if let Some(v) = memory_val {
            let m = v.to_long();
            if !(1..=9).contains(&m) {
                let msg = "deflate_init(): \"memory\" option must be between 1 and 9";
                let exc = vm.create_exception(b"ValueError", msg, 0);
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg.to_string(), line: vm.current_line });
            }
        }
        if let Some(v) = dict_val {
            dictionary = extract_dictionary(vm, "deflate_init", &v)?;
        }
    }

    let compress = make_compress(encoding, comp(level));
    // Note: dictionary support requires "any_c_zlib" or "zlib-rs" feature which we don't have
    // when using miniz_oxide backend. Dictionary will be silently ignored.
    let _ = dictionary;

    let cid = alloc_ctx_id();
    DEFLATE_CONTEXTS.with(|ctxs| {
        ctxs.borrow_mut().insert(cid, DeflateContext {
            encoding,
            compress,
            finished: false,
        });
    });
    goro_core::value::register_resource(cid, b"zlib.deflate");
    Ok(Value::Resource(cid))
}

fn deflate_add_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let cid = args.first().unwrap_or(&Value::Null).to_long();
    let data = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let flush_mode = args.get(2).map(|v| v.to_long()).unwrap_or(2); // ZLIB_SYNC_FLUSH

    if !(0..=5).contains(&flush_mode) {
        let msg = "deflate_add(): Argument #3 ($flush_mode) must be one of ZLIB_NO_FLUSH, ZLIB_PARTIAL_FLUSH, ZLIB_SYNC_FLUSH, ZLIB_FULL_FLUSH, ZLIB_BLOCK, or ZLIB_FINISH";
        let exc = vm.create_exception(b"ValueError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg.to_string(), line: vm.current_line });
    }

    // Check if this is a valid resource (not a generic file resource)
    let rtype = goro_core::value::resource_type_of(cid);
    if rtype.as_deref() != Some(b"zlib.deflate") {
        let msg = "deflate_add(): Argument #1 ($context) must be of type DeflateContext, resource given";
        let exc = vm.create_exception(b"TypeError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg.to_string(), line: vm.current_line });
    }

    
    DEFLATE_CONTEXTS.with(|ctxs| -> Result<Value, VmError> {
        let mut ctxs = ctxs.borrow_mut();
        if let Some(ctx) = ctxs.get_mut(&cid) {
            let flush = match flush_mode {
                0 => FlushCompress::None,
                1 => FlushCompress::Partial,
                2 => FlushCompress::Sync,
                3 => FlushCompress::Full,
                4 => FlushCompress::Finish,
                _ => FlushCompress::Sync,
            };
            let input = data.as_bytes();
            let mut output = Vec::new();
            let mut total_in = 0;
            loop {
                let mut chunk = vec![0u8; (input.len() - total_in).max(64).max(16384)];
                let in_before = ctx.compress.total_in();
                let out_before = ctx.compress.total_out();
                let res = ctx.compress.compress(&input[total_in..], &mut chunk, flush);
                let consumed = (ctx.compress.total_in() - in_before) as usize;
                let produced = (ctx.compress.total_out() - out_before) as usize;
                total_in += consumed;
                output.extend_from_slice(&chunk[..produced]);
                match res {
                    Ok(Status::StreamEnd) => {
                        ctx.finished = true;
                        // Reset for next use (PHP allows reuse after finish)
                        ctx.compress.reset();
                        break;
                    }
                    Ok(Status::Ok) => {
                        if flush == FlushCompress::None && total_in >= input.len() {
                            break;
                        }
                        if (flush == FlushCompress::Sync || flush == FlushCompress::Partial || flush == FlushCompress::Full) && total_in >= input.len() && produced < chunk.len() {
                            break;
                        }
                        continue;
                    }
                    Ok(Status::BufError) => {
                        if total_in >= input.len() && produced == 0 {
                            break;
                        }
                        continue;
                    }
                    Err(_) => return Ok(Value::False),
                }
            }
            Ok(Value::String(PhpString::from_vec(output)))
        } else {
            let msg = "deflate_add(): supplied resource is not a valid zlib context resource";
            vm.emit_warning(msg);
            Ok(Value::False)
        }
    })
}

fn inflate_init_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let encoding = args.first().map(|v| v.to_long()).unwrap_or(15);
    if encoding != 31 && encoding != 15 && encoding != -15 {
        let msg = "Encoding mode must be ZLIB_ENCODING_RAW, ZLIB_ENCODING_GZIP or ZLIB_ENCODING_DEFLATE";
        let exc = vm.create_exception(b"ValueError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg.to_string(), line: vm.current_line });
    }

    let mut dictionary: Option<Vec<u8>> = None;
    if let Some(opts) = args.get(1) {
        let dict_val: Option<Value> = match opts {
            Value::Array(arr) => {
                let arr = arr.borrow();
                arr.get(&goro_core::array::ArrayKey::String(PhpString::from_bytes(b"dictionary"))).cloned()
            }
            Value::Object(obj) => {
                let obj = obj.borrow();
                obj.properties.iter().find(|(k, _)| k.as_slice() == b"dictionary").map(|(_, v)| v.clone())
            }
            _ => None,
        };
        if let Some(v) = dict_val {
            dictionary = extract_dictionary(vm, "inflate_init", &v)?;
        }
    }

    let decompress = make_decompress(encoding);
    let pending_dict_apply = false;
    // Note: dictionary support requires "any_c_zlib" or "zlib-rs" feature which we don't have
    // when using miniz_oxide backend. Dictionary will be silently ignored.

    let cid = alloc_ctx_id();
    INFLATE_CONTEXTS.with(|ctxs| {
        ctxs.borrow_mut().insert(cid, InflateContext {
            encoding,
            decompress,
            total_read: 0,
            status: 0,
            dictionary,
            pending_dict_apply,
            gz_header_buf: Vec::new(),
            gz_header_done: encoding != 31,
            gz_header_consumed: 0,
            gz_trailer_remaining: if encoding == 31 { 8 } else { 0 },
        });
    });
    goro_core::value::register_resource(cid, b"zlib.inflate");
    Ok(Value::Resource(cid))
}

fn inflate_add_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let cid = args.first().unwrap_or(&Value::Null).to_long();
    let data = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let flush_mode = args.get(2).map(|v| v.to_long()).unwrap_or(2); // ZLIB_SYNC_FLUSH

    if !(0..=5).contains(&flush_mode) {
        let msg = "inflate_add(): Argument #3 ($flush_mode) must be one of ZLIB_NO_FLUSH, ZLIB_PARTIAL_FLUSH, ZLIB_SYNC_FLUSH, ZLIB_FULL_FLUSH, ZLIB_BLOCK, or ZLIB_FINISH";
        let exc = vm.create_exception(b"ValueError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg.to_string(), line: vm.current_line });
    }

    let rtype = goro_core::value::resource_type_of(cid);
    if rtype.as_deref() != Some(b"zlib.inflate") {
        let msg = "inflate_add(): Argument #1 ($context) must be of type InflateContext, resource given";
        let exc = vm.create_exception(b"TypeError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg.to_string(), line: vm.current_line });
    }

    
    INFLATE_CONTEXTS.with(|ctxs| -> Result<Value, VmError> {
        let mut ctxs = ctxs.borrow_mut();
        if let Some(ctx) = ctxs.get_mut(&cid) {
            // After STREAM_END, decide whether to reset for reuse or consume trailer.
            // For gzip: there's an 8-byte trailer to consume first.
            // For zlib/raw deflate: no trailer, any new data triggers reset.
            if ctx.status == 1 && !data.as_bytes().is_empty() {
                let needs_reset = if ctx.encoding == 31 {
                    // Try to consume trailer first
                    let dlen = data.as_bytes().len();
                    if ctx.gz_trailer_remaining >= dlen {
                        // All input is trailer
                        ctx.gz_trailer_remaining -= dlen;
                        ctx.total_read += dlen;
                        return Ok(Value::String(PhpString::from_bytes(b"")));
                    } else {
                        // Trailer done; remaining bytes are new stream
                        let trailer_part = ctx.gz_trailer_remaining;
                        ctx.gz_trailer_remaining = 0;
                        // We'll proceed with data[trailer_part..]
                        // For simplicity, reset and feed full data (zlib doesn't care about extra trailer at start typically; better to just reset and not handle this complex case)
                        let _ = trailer_part;
                        true
                    }
                } else {
                    true
                };
                if needs_reset {
                    ctx.decompress.reset(ctx.encoding == 15);
                    ctx.status = 0;
                    ctx.total_read = 0;
                    if ctx.encoding == 31 {
                        ctx.gz_header_buf.clear();
                        ctx.gz_header_done = false;
                        ctx.gz_header_consumed = 0;
                        ctx.gz_trailer_remaining = 8;
                    }
                }
            }

            let flush = match flush_mode {
                0 => FlushDecompress::None,
                4 => FlushDecompress::Finish,
                _ => FlushDecompress::Sync,
            };

            let input_vec: Vec<u8>;
            let input: &[u8];
            ctx.total_read += data.as_bytes().len();
            // For gzip encoding, parse the header first
            if ctx.encoding == 31 && !ctx.gz_header_done {
                ctx.gz_header_buf.extend_from_slice(data.as_bytes());
                match parse_gzip_header(&ctx.gz_header_buf) {
                    Some(0) => {
                        vm.emit_warning("inflate_add(): data error");
                        return Ok(Value::False);
                    }
                    Some(n) => {
                        ctx.gz_header_done = true;
                        ctx.gz_header_consumed = n;
                        // Use bytes after header as input
                        input_vec = ctx.gz_header_buf[n..].to_vec();
                        ctx.gz_header_buf.clear();
                        input = &input_vec;
                    }
                    None => {
                        // Need more data - return empty
                        return Ok(Value::String(PhpString::from_bytes(b"")));
                    }
                }
            } else {
                input = data.as_bytes();
                input_vec = Vec::new();
            }
            let _ = input_vec; // borrow lifetime
            let mut output = Vec::new();
            let mut total_consumed = 0usize;

            // If we already finished (status==1), just consume bytes as trailer
            if ctx.status == 1 {
                if ctx.encoding == 31 {
                    let remaining_trailer = ctx.gz_trailer_remaining.min(input.len());
                    ctx.gz_trailer_remaining -= remaining_trailer;
                }
                return Ok(Value::String(PhpString::from_vec(output)));
            }

            loop {
                let mut chunk = vec![0u8; 16384];
                let in_before = ctx.decompress.total_in();
                let out_before = ctx.decompress.total_out();
                let in_slice = if total_consumed >= input.len() { &[] } else { &input[total_consumed..] };
                let res = ctx.decompress.decompress(in_slice, &mut chunk, flush);
                let consumed = (ctx.decompress.total_in() - in_before) as usize;
                let produced = (ctx.decompress.total_out() - out_before) as usize;
                total_consumed += consumed;
                output.extend_from_slice(&chunk[..produced]);

                match res {
                    Ok(Status::StreamEnd) => {
                        ctx.status = 1; // ZLIB_STREAM_END
                        // For gzip, consume up to 8 trailer bytes from remaining input
                        if ctx.encoding == 31 {
                            let remaining = input.len() - total_consumed;
                            let to_consume = ctx.gz_trailer_remaining.min(remaining);
                            ctx.gz_trailer_remaining -= to_consume;
                        }
                        break;
                    }
                    Ok(Status::Ok) => {
                        if consumed == 0 && produced == 0 {
                            break;
                        }
                        if produced < chunk.len() && total_consumed >= input.len() {
                            break;
                        }
                        continue;
                    }
                    Ok(Status::BufError) => {
                        break;
                    }
                    Err(e) => {
                        if let Some(_adler) = e.needs_dictionary() {
                            vm.emit_warning("inflate_add(): inflating: invalid dictionary");
                            return Ok(Value::False);
                        }
                        vm.emit_warning("inflate_add(): data error");
                        return Ok(Value::False);
                    }
                }
            }
            Ok(Value::String(PhpString::from_vec(output)))
        } else {
            let msg = "inflate_add(): supplied resource is not a valid zlib context resource";
            vm.emit_warning(msg);
            Ok(Value::False)
        }
    })
}

fn inflate_get_read_len_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let cid = args.first().unwrap_or(&Value::Null).to_long();
    INFLATE_CONTEXTS.with(|ctxs| {
        let ctxs = ctxs.borrow();
        if let Some(ctx) = ctxs.get(&cid) {
            // Return total_in from decompress + header overhead (bytes actually consumed)
            let extra = ctx.gz_header_consumed as u64;
            let trailer = if ctx.encoding == 31 && ctx.status == 1 { 8 } else { 0 };
            Ok(Value::Long((ctx.decompress.total_in() + extra + trailer) as i64))
        } else {
            Ok(Value::False)
        }
    })
}

fn inflate_get_status_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let cid = args.first().unwrap_or(&Value::Null).to_long();
    INFLATE_CONTEXTS.with(|ctxs| {
        let ctxs = ctxs.borrow();
        if let Some(ctx) = ctxs.get(&cid) {
            Ok(Value::Long(ctx.status))
        } else {
            Ok(Value::False)
        }
    })
}
