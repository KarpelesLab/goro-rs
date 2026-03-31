use goro_core::array::PhpArray;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

use flate2::read::{DeflateDecoder, GzDecoder, ZlibDecoder};
use flate2::write::{DeflateEncoder, GzEncoder, ZlibEncoder};
use flate2::Compression;
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{Read, Write, Cursor};
use std::rc::Rc;

pub fn register(vm: &mut Vm) {
    vm.register_function(b"gzcompress", gzcompress);
    vm.register_function(b"gzuncompress", gzuncompress);
    vm.register_function(b"gzdeflate", gzdeflate);
    vm.register_function(b"gzinflate", gzinflate);
    vm.register_function(b"gzencode", gzencode);
    vm.register_function(b"gzdecode", gzdecode);
    vm.register_function(b"zlib_encode", zlib_encode);
    vm.register_function(b"zlib_decode", zlib_decode);
    vm.register_function(b"zlib_get_coding_type", zlib_get_coding_type);

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
    if level < -1 || level > 9 {
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
        let mut buf = vec![0u8; max as usize];
        let n = r.read(&mut buf)?;
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
    // Empty string or data too short for valid zlib
    if bytes.is_empty() || bytes.len() < 2 {
        vm.emit_warning("gzuncompress(): data error");
        return Ok(Value::False);
    }
    let mut dec = ZlibDecoder::new(bytes);
    match read_limited(&mut dec, max) {
        Ok(d) => Ok(Value::String(PhpString::from_vec(d))),
        Err(_) => { vm.emit_warning("gzuncompress(): data error"); Ok(Value::False) }
    }
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
    let mut dec = DeflateDecoder::new(bytes);
    let mut out = Vec::new();
    match dec.read_to_end(&mut out) {
        Ok(_) => {
            if max > 0 {
                out.truncate(max as usize);
            }
            Ok(Value::String(PhpString::from_vec(out)))
        }
        Err(_) => { vm.emit_warning("gzinflate(): data error"); Ok(Value::False) }
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
    let mut dec = GzDecoder::new(data.as_bytes());
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
    // Try gzip
    if b.len() >= 2 && b[0] == 0x1f && b[1] == 0x8b {
        if let Ok(d) = read_limited(&mut GzDecoder::new(b), max) { return Ok(Value::String(PhpString::from_vec(d))); }
    }
    // Try zlib
    if b.len() >= 2 && (b[0] & 0x0F) == 8 {
        if let Ok(d) = read_limited(&mut ZlibDecoder::new(b), max) { return Ok(Value::String(PhpString::from_vec(d))); }
    }
    // Try raw deflate
    if let Ok(d) = read_limited(&mut DeflateDecoder::new(b), max) { return Ok(Value::String(PhpString::from_vec(d))); }
    vm.emit_warning("zlib_decode(): data error");
    Ok(Value::False)
}

fn zlib_get_coding_type(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // In CLI mode, no content encoding is active
    Ok(Value::False)
}

// ========== gzopen family ==========

/// GzHandle represents an opened gzip stream (for reading or writing).
enum GzHandle {
    Reader {
        /// The raw compressed data from the file
        raw_data: Vec<u8>,
        /// The decompressed data buffer (lazily filled)
        decompressed: Vec<u8>,
        /// Current position in decompressed data
        pos: usize,
        /// Whether all data has been decompressed
        fully_decompressed: bool,
        /// Whether we're at EOF
        eof: bool,
    },
    Writer {
        /// Path to write to
        path: String,
        /// Accumulated uncompressed data
        buffer: Vec<u8>,
        /// Compression level
        level: Compression,
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

/// Ensure reader has decompressed all data
fn ensure_fully_decompressed(handle: &mut GzHandle) {
    if let GzHandle::Reader { raw_data, decompressed, fully_decompressed, .. } = handle {
        if !*fully_decompressed {
            let mut dec = GzDecoder::new(raw_data.as_slice());
            let mut buf = Vec::new();
            let _ = dec.read_to_end(&mut buf);
            if buf.is_empty() {
                // Maybe it's a plain text file (not gzipped)
                buf = raw_data.clone();
            }
            *decompressed = buf;
            *fully_decompressed = true;
        }
    }
}

fn gzopen_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let filename = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy();
    let mode_str = args.get(1).map(|v| v.to_php_string().to_string_lossy()).unwrap_or_else(|| "r".to_string());

    // Parse mode: extract r/w and optional compression level
    let mode_clean: String = mode_str.chars().filter(|&c| c != 'b' && c != 't').collect();

    // Check for r+/w+ (not allowed for gzip)
    if mode_clean.contains('+') {
        vm.emit_warning("gzopen(): Cannot open a zlib stream for reading and writing at the same time!");
        return Ok(Value::False);
    }

    let is_read = mode_clean.starts_with('r');
    let is_write = mode_clean.starts_with('w') || mode_clean.starts_with('a');

    if !is_read && !is_write {
        vm.emit_warning("gzopen(): gzopen failed");
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
                if is_gz {
                    GZ_HANDLES.with(|handles| {
                        handles.borrow_mut().insert(gid, GzHandle::Reader {
                            raw_data: data,
                            decompressed: Vec::new(),
                            pos: 0,
                            fully_decompressed: false,
                            eof: false,
                        });
                    });
                } else {
                    // Plain text file - treat as already decompressed
                    GZ_HANDLES.with(|handles| {
                        handles.borrow_mut().insert(gid, GzHandle::Reader {
                            raw_data: Vec::new(),
                            decompressed: data,
                            pos: 0,
                            fully_decompressed: true,
                            eof: false,
                        });
                    });
                }
                Ok(Value::Long(gid))
            }
            Err(_) => {
                vm.emit_warning(&format!("gzopen({}): Failed to open stream: No such file or directory", filename));
                Ok(Value::False)
            }
        }
    } else {
        // Write mode
        let gid = alloc_gz_id();
        GZ_HANDLES.with(|handles| {
            handles.borrow_mut().insert(gid, GzHandle::Writer {
                path: filename.to_string(),
                buffer: Vec::new(),
                level: comp(level_from_mode),
            });
        });
        Ok(Value::Long(gid))
    }
}

fn gzclose_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let gid = args.first().unwrap_or(&Value::Null).to_long();
    GZ_HANDLES.with(|handles| {
        if let Some(handle) = handles.borrow_mut().remove(&gid) {
            // If writer, flush the gzipped data to disk
            if let GzHandle::Writer { path, buffer, level } = handle {
                let mut enc = GzEncoder::new(Vec::new(), level);
                let _ = enc.write_all(&buffer);
                if let Ok(compressed) = enc.finish() {
                    let _ = std::fs::write(&path, &compressed);
                }
            }
            Ok(Value::True)
        } else {
            Ok(Value::False)
        }
    })
}

fn gzread_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let gid = args.first().unwrap_or(&Value::Null).to_long();
    let length = args.get(1).map(|v| v.to_long()).unwrap_or(0) as usize;
    if length == 0 {
        return Ok(Value::String(PhpString::from_bytes(b"")));
    }

    GZ_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(handle) = handles.get_mut(&gid) {
            ensure_fully_decompressed(handle);
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
                Ok(Value::False)
            }
        } else {
            Ok(Value::False)
        }
    })
}

fn gzwrite_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let gid = args.first().unwrap_or(&Value::Null).to_long();
    let data = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let bytes = data.as_bytes();
    let length = args.get(2).map(|v| v.to_long() as usize).unwrap_or(bytes.len());
    let to_write = &bytes[..length.min(bytes.len())];

    GZ_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(handle) = handles.get_mut(&gid) {
            if let GzHandle::Writer { buffer, .. } = handle {
                buffer.extend_from_slice(to_write);
                Ok(Value::Long(to_write.len() as i64))
            } else {
                Ok(Value::False)
            }
        } else {
            Ok(Value::False)
        }
    })
}

fn gzgets_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let gid = args.first().unwrap_or(&Value::Null).to_long();
    let max_length = args.get(1).map(|v| v.to_long() as usize);

    GZ_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(handle) = handles.get_mut(&gid) {
            ensure_fully_decompressed(handle);
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

fn gzgetc_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let gid = args.first().unwrap_or(&Value::Null).to_long();

    GZ_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(handle) = handles.get_mut(&gid) {
            ensure_fully_decompressed(handle);
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

fn gzeof_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let gid = args.first().unwrap_or(&Value::Null).to_long();

    GZ_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(handle) = handles.get_mut(&gid) {
            ensure_fully_decompressed(handle);
            if let GzHandle::Reader { decompressed, pos, eof, .. } = handle {
                if *pos >= decompressed.len() {
                    *eof = true;
                }
                if *eof { Ok(Value::True) } else { Ok(Value::False) }
            } else if let GzHandle::Writer { .. } = handle {
                // Writing - report false (not at eof)
                Ok(Value::False)
            } else {
                Ok(Value::True)
            }
        } else {
            Ok(Value::True)
        }
    })
}

fn gzrewind_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let gid = args.first().unwrap_or(&Value::Null).to_long();

    GZ_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(handle) = handles.get_mut(&gid) {
            if let GzHandle::Reader { pos, eof, .. } = handle {
                *pos = 0;
                *eof = false;
                Ok(Value::True)
            } else {
                Ok(Value::False)
            }
        } else {
            Ok(Value::False)
        }
    })
}

fn gzseek_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let gid = args.first().unwrap_or(&Value::Null).to_long();
    let offset = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let whence = args.get(2).map(|v| v.to_long()).unwrap_or(0); // SEEK_SET

    GZ_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(handle) = handles.get_mut(&gid) {
            ensure_fully_decompressed(handle);
            if let GzHandle::Reader { decompressed, pos, eof, .. } = handle {
                let new_pos = match whence {
                    0 => offset, // SEEK_SET
                    1 => *pos as i64 + offset, // SEEK_CUR
                    2 => decompressed.len() as i64 + offset, // SEEK_END (not supported for gz, but handle anyway)
                    _ => return Ok(Value::Long(-1)),
                };
                if new_pos < 0 {
                    return Ok(Value::Long(-1));
                }
                *pos = new_pos as usize;
                *eof = false;
                Ok(Value::Long(0))
            } else {
                Ok(Value::Long(-1))
            }
        } else {
            Ok(Value::Long(-1))
        }
    })
}

fn gztell_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let gid = args.first().unwrap_or(&Value::Null).to_long();

    GZ_HANDLES.with(|handles| {
        let handles = handles.borrow();
        if let Some(handle) = handles.get(&gid) {
            match handle {
                GzHandle::Reader { pos, .. } => Ok(Value::Long(*pos as i64)),
                GzHandle::Writer { buffer, .. } => Ok(Value::Long(buffer.len() as i64)),
            }
        } else {
            Ok(Value::False)
        }
    })
}

fn gzpassthru_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let gid = args.first().unwrap_or(&Value::Null).to_long();

    GZ_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(handle) = handles.get_mut(&gid) {
            ensure_fully_decompressed(handle);
            if let GzHandle::Reader { decompressed, pos, eof, .. } = handle {
                if *pos >= decompressed.len() {
                    *eof = true;
                    return Ok(Value::Long(0));
                }
                let remaining = &decompressed[*pos..];
                let len = remaining.len();
                vm.write_output(remaining);
                *pos = decompressed.len();
                *eof = true;
                Ok(Value::Long(len as i64))
            } else {
                Ok(Value::Long(0))
            }
        } else {
            Ok(Value::False)
        }
    })
}

fn gzfile_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let filename = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy();

    let data = match std::fs::read(&filename) {
        Ok(d) => d,
        Err(_) => return Ok(Value::False),
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
    encoding: i64,
    level: Compression,
    buffer: Vec<u8>,
}

struct InflateContext {
    encoding: i64,
    buffer: Vec<u8>,
    total_read: usize,
    status: i64, // 0 = OK, 1 = STREAM_END
}

fn alloc_ctx_id() -> i64 {
    NEXT_CTX_ID.with(|id| {
        let cid = id.get();
        id.set(cid + 1);
        cid
    })
}

fn deflate_init_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let encoding = args.first().map(|v| v.to_long()).unwrap_or(15);
    validate_encoding(vm, "deflate_init", encoding, 1)?;

    let mut level = -1i64;
    if let Some(opts) = args.get(1) {
        if let Value::Array(arr) = opts {
            let arr = arr.borrow();
            if let Some(v) = arr.get(&goro_core::array::ArrayKey::String(PhpString::from_bytes(b"level"))) {
                level = v.to_long();
            }
        }
    }

    if level != -1 {
        validate_level(vm, "deflate_init", level)?;
    }

    let cid = alloc_ctx_id();
    DEFLATE_CONTEXTS.with(|ctxs| {
        ctxs.borrow_mut().insert(cid, DeflateContext {
            encoding,
            level: comp(level),
            buffer: Vec::new(),
        });
    });
    Ok(Value::Long(cid))
}

fn deflate_add_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let cid = args.first().unwrap_or(&Value::Null).to_long();
    let data = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let flush_mode = args.get(2).map(|v| v.to_long()).unwrap_or(2); // ZLIB_SYNC_FLUSH

    DEFLATE_CONTEXTS.with(|ctxs| {
        let mut ctxs = ctxs.borrow_mut();
        if let Some(ctx) = ctxs.get_mut(&cid) {
            ctx.buffer.extend_from_slice(data.as_bytes());

            if flush_mode == 4 {
                // ZLIB_FINISH - compress everything and finish
                let buf = std::mem::take(&mut ctx.buffer);
                let result: Result<Vec<u8>, _> = match ctx.encoding {
                    31 => { let mut e = GzEncoder::new(Vec::new(), ctx.level); e.write_all(&buf).ok(); e.finish() }
                    15 => { let mut e = ZlibEncoder::new(Vec::new(), ctx.level); e.write_all(&buf).ok(); e.finish() }
                    -15 => { let mut e = DeflateEncoder::new(Vec::new(), ctx.level); e.write_all(&buf).ok(); e.finish() }
                    _ => return Ok(Value::False),
                };
                match result {
                    Ok(compressed) => Ok(Value::String(PhpString::from_vec(compressed))),
                    Err(_) => Ok(Value::False),
                }
            } else {
                // ZLIB_SYNC_FLUSH, ZLIB_PARTIAL_FLUSH, ZLIB_FULL_FLUSH, etc.
                // For incremental, we flush what we have
                let buf = std::mem::take(&mut ctx.buffer);
                if buf.is_empty() {
                    return Ok(Value::String(PhpString::from_bytes(b"")));
                }
                // Use sync flush: compress with a flush marker
                let result: Result<Vec<u8>, _> = match ctx.encoding {
                    31 => {
                        let mut e = GzEncoder::new(Vec::new(), ctx.level);
                        e.write_all(&buf).ok();
                        e.flush().ok();
                        // Read what's available
                        Ok::<Vec<u8>, std::io::Error>(e.get_ref().clone())
                    }
                    15 => {
                        let mut e = ZlibEncoder::new(Vec::new(), ctx.level);
                        e.write_all(&buf).ok();
                        e.flush().ok();
                        Ok::<Vec<u8>, std::io::Error>(e.get_ref().clone())
                    }
                    -15 => {
                        let mut e = DeflateEncoder::new(Vec::new(), ctx.level);
                        e.write_all(&buf).ok();
                        e.flush().ok();
                        Ok::<Vec<u8>, std::io::Error>(e.get_ref().clone())
                    }
                    _ => return Ok(Value::False),
                };
                match result {
                    Ok(compressed) => Ok(Value::String(PhpString::from_vec(compressed))),
                    Err(_) => Ok(Value::False),
                }
            }
        } else {
            vm.emit_warning("deflate_add(): supplied resource is not a valid zlib context resource");
            Ok(Value::False)
        }
    })
}

fn inflate_init_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let encoding = args.first().map(|v| v.to_long()).unwrap_or(15);
    validate_encoding(vm, "inflate_init", encoding, 1)?;

    let cid = alloc_ctx_id();
    INFLATE_CONTEXTS.with(|ctxs| {
        ctxs.borrow_mut().insert(cid, InflateContext {
            encoding,
            buffer: Vec::new(),
            total_read: 0,
            status: 0,
        });
    });
    Ok(Value::Long(cid))
}

fn inflate_add_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let cid = args.first().unwrap_or(&Value::Null).to_long();
    let data = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let flush_mode = args.get(2).map(|v| v.to_long()).unwrap_or(2); // ZLIB_SYNC_FLUSH
    let _ = flush_mode;

    INFLATE_CONTEXTS.with(|ctxs| {
        let mut ctxs = ctxs.borrow_mut();
        if let Some(ctx) = ctxs.get_mut(&cid) {
            ctx.buffer.extend_from_slice(data.as_bytes());
            ctx.total_read += data.as_bytes().len();

            // Try to decompress what we have
            let buf = ctx.buffer.clone();
            let result: Result<Vec<u8>, _> = match ctx.encoding {
                31 => {
                    let mut d = GzDecoder::new(Cursor::new(&buf));
                    let mut out = Vec::new();
                    d.read_to_end(&mut out).map(|_| out)
                }
                15 => {
                    let mut d = ZlibDecoder::new(Cursor::new(&buf));
                    let mut out = Vec::new();
                    d.read_to_end(&mut out).map(|_| out)
                }
                -15 => {
                    let mut d = DeflateDecoder::new(Cursor::new(&buf));
                    let mut out = Vec::new();
                    d.read_to_end(&mut out).map(|_| out)
                }
                _ => return Ok(Value::False),
            };
            match result {
                Ok(decompressed) => {
                    ctx.status = 1; // ZLIB_STREAM_END
                    Ok(Value::String(PhpString::from_vec(decompressed)))
                }
                Err(_) => {
                    // Partial data - return empty for now
                    Ok(Value::String(PhpString::from_bytes(b"")))
                }
            }
        } else {
            vm.emit_warning("inflate_add(): supplied resource is not a valid zlib context resource");
            Ok(Value::False)
        }
    })
}

fn inflate_get_read_len_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let cid = args.first().unwrap_or(&Value::Null).to_long();
    INFLATE_CONTEXTS.with(|ctxs| {
        let ctxs = ctxs.borrow();
        if let Some(ctx) = ctxs.get(&cid) {
            Ok(Value::Long(ctx.total_read as i64))
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
