use goro_core::array::PhpArray;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

mod codec;

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::rc::Rc;

pub fn register(vm: &mut Vm) {
    vm.register_extension(b"zlib");
    vm.register_function(b"gzcompress", codec::gzcompress);
    vm.register_function(b"gzuncompress", codec::gzuncompress);
    vm.register_function(b"gzdeflate", codec::gzdeflate);
    vm.register_function(b"gzinflate", codec::gzinflate);
    vm.register_function(b"gzencode", codec::gzencode);
    vm.register_function(b"gzdecode", codec::gzdecode);
    vm.register_function(b"zlib_encode", codec::zlib_encode);
    vm.register_function(b"zlib_decode", codec::zlib_decode);
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
    vm.register_function(b"deflate_init", codec::deflate_init);
    vm.register_function(b"deflate_add", codec::deflate_add);
    vm.register_function(b"inflate_init", codec::inflate_init);
    vm.register_function(b"inflate_add", codec::inflate_add);
    vm.register_function(b"inflate_get_read_len", codec::inflate_get_read_len);
    vm.register_function(b"inflate_get_status", codec::inflate_get_status);

    vm.builtin_param_names.insert(
        b"gzcompress".to_vec(),
        vec![b"data".to_vec(), b"level".to_vec(), b"encoding".to_vec()],
    );
    vm.builtin_param_names.insert(
        b"gzuncompress".to_vec(),
        vec![b"data".to_vec(), b"max_length".to_vec()],
    );
    vm.builtin_param_names.insert(
        b"gzdeflate".to_vec(),
        vec![b"data".to_vec(), b"level".to_vec(), b"encoding".to_vec()],
    );
    vm.builtin_param_names.insert(
        b"gzinflate".to_vec(),
        vec![b"data".to_vec(), b"max_length".to_vec()],
    );
    vm.builtin_param_names.insert(
        b"gzencode".to_vec(),
        vec![b"data".to_vec(), b"level".to_vec(), b"encoding".to_vec()],
    );
    vm.builtin_param_names.insert(
        b"gzdecode".to_vec(),
        vec![b"data".to_vec(), b"max_length".to_vec()],
    );
    vm.builtin_param_names.insert(
        b"zlib_encode".to_vec(),
        vec![b"data".to_vec(), b"encoding".to_vec(), b"level".to_vec()],
    );
    vm.builtin_param_names.insert(
        b"zlib_decode".to_vec(),
        vec![b"data".to_vec(), b"max_length".to_vec()],
    );
    vm.builtin_param_names
        .insert(b"zlib_get_coding_type".to_vec(), vec![]);

    vm.builtin_param_names.insert(
        b"gzopen".to_vec(),
        vec![
            b"filename".to_vec(),
            b"mode".to_vec(),
            b"use_include_path".to_vec(),
        ],
    );
    vm.builtin_param_names
        .insert(b"gzclose".to_vec(), vec![b"stream".to_vec()]);
    vm.builtin_param_names.insert(
        b"gzread".to_vec(),
        vec![b"stream".to_vec(), b"length".to_vec()],
    );
    vm.builtin_param_names.insert(
        b"gzwrite".to_vec(),
        vec![b"stream".to_vec(), b"data".to_vec(), b"length".to_vec()],
    );
    vm.builtin_param_names.insert(
        b"gzputs".to_vec(),
        vec![b"stream".to_vec(), b"data".to_vec(), b"length".to_vec()],
    );
    vm.builtin_param_names.insert(
        b"gzgets".to_vec(),
        vec![b"stream".to_vec(), b"length".to_vec()],
    );
    vm.builtin_param_names
        .insert(b"gzgetc".to_vec(), vec![b"stream".to_vec()]);
    vm.builtin_param_names
        .insert(b"gzeof".to_vec(), vec![b"stream".to_vec()]);
    vm.builtin_param_names
        .insert(b"gzrewind".to_vec(), vec![b"stream".to_vec()]);
    vm.builtin_param_names.insert(
        b"gzseek".to_vec(),
        vec![b"stream".to_vec(), b"offset".to_vec(), b"whence".to_vec()],
    );
    vm.builtin_param_names
        .insert(b"gztell".to_vec(), vec![b"stream".to_vec()]);
    vm.builtin_param_names
        .insert(b"gzpassthru".to_vec(), vec![b"stream".to_vec()]);
    vm.builtin_param_names.insert(
        b"gzfile".to_vec(),
        vec![b"filename".to_vec(), b"use_include_path".to_vec()],
    );
    vm.builtin_param_names.insert(
        b"readgzfile".to_vec(),
        vec![b"filename".to_vec(), b"use_include_path".to_vec()],
    );

    vm.builtin_param_names.insert(
        b"deflate_init".to_vec(),
        vec![b"encoding".to_vec(), b"options".to_vec()],
    );
    vm.builtin_param_names.insert(
        b"deflate_add".to_vec(),
        vec![
            b"context".to_vec(),
            b"data".to_vec(),
            b"flush_mode".to_vec(),
        ],
    );
    vm.builtin_param_names.insert(
        b"inflate_init".to_vec(),
        vec![b"encoding".to_vec(), b"options".to_vec()],
    );
    vm.builtin_param_names.insert(
        b"inflate_add".to_vec(),
        vec![
            b"context".to_vec(),
            b"data".to_vec(),
            b"flush_mode".to_vec(),
        ],
    );
    vm.builtin_param_names
        .insert(b"inflate_get_read_len".to_vec(), vec![b"context".to_vec()]);
    vm.builtin_param_names
        .insert(b"inflate_get_status".to_vec(), vec![b"context".to_vec()]);

    vm.constants
        .insert(b"ZLIB_ENCODING_RAW".to_vec(), Value::Long(-15));
    vm.constants
        .insert(b"ZLIB_ENCODING_DEFLATE".to_vec(), Value::Long(15));
    vm.constants
        .insert(b"ZLIB_ENCODING_GZIP".to_vec(), Value::Long(31));
    vm.constants.insert(b"FORCE_GZIP".to_vec(), Value::Long(31));
    vm.constants
        .insert(b"FORCE_DEFLATE".to_vec(), Value::Long(15));
    vm.constants
        .insert(b"ZLIB_NO_FLUSH".to_vec(), Value::Long(0));
    vm.constants
        .insert(b"ZLIB_PARTIAL_FLUSH".to_vec(), Value::Long(1));
    vm.constants
        .insert(b"ZLIB_SYNC_FLUSH".to_vec(), Value::Long(2));
    vm.constants
        .insert(b"ZLIB_FULL_FLUSH".to_vec(), Value::Long(3));
    vm.constants.insert(b"ZLIB_FINISH".to_vec(), Value::Long(4));
    vm.constants.insert(b"ZLIB_BLOCK".to_vec(), Value::Long(5));
    vm.constants.insert(b"ZLIB_OK".to_vec(), Value::Long(0));
    vm.constants
        .insert(b"ZLIB_STREAM_END".to_vec(), Value::Long(1));
    vm.constants
        .insert(b"ZLIB_NEED_DICT".to_vec(), Value::Long(2));
    vm.constants.insert(b"ZLIB_ERRNO".to_vec(), Value::Long(-1));
    vm.constants
        .insert(b"ZLIB_STREAM_ERROR".to_vec(), Value::Long(-2));
    vm.constants
        .insert(b"ZLIB_DATA_ERROR".to_vec(), Value::Long(-3));
    vm.constants
        .insert(b"ZLIB_MEM_ERROR".to_vec(), Value::Long(-4));
    vm.constants
        .insert(b"ZLIB_BUF_ERROR".to_vec(), Value::Long(-5));
    vm.constants
        .insert(b"ZLIB_VERSION_ERROR".to_vec(), Value::Long(-6));
    vm.constants
        .insert(b"ZLIB_FILTERED".to_vec(), Value::Long(1));
    vm.constants
        .insert(b"ZLIB_HUFFMAN_ONLY".to_vec(), Value::Long(2));
    vm.constants.insert(b"ZLIB_RLE".to_vec(), Value::Long(3));
    vm.constants.insert(b"ZLIB_FIXED".to_vec(), Value::Long(4));
    vm.constants
        .insert(b"ZLIB_DEFAULT_STRATEGY".to_vec(), Value::Long(0));
    vm.constants
        .insert(b"ZLIB_VERNUM".to_vec(), Value::Long(goro_zlib::ZLIB_VERNUM));
    vm.constants.insert(
        b"ZLIB_VERSION".to_vec(),
        Value::String(PhpString::from_bytes(goro_zlib::ZLIB_VERSION.as_bytes())),
    );
    codec::register_classes(vm);
}

fn comp(level: i64) -> Compression {
    if level == -1 {
        Compression::default()
    } else {
        Compression::new(level.clamp(0, 9) as u32)
    }
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
}

/// php_stream_gzopen(): the inner (file) stream is a resource too, then the
/// zlib stream wrapping it.
fn alloc_gz_id() -> i64 {
    let _inner = goro_core::value::alloc_resource_id();
    goro_core::value::alloc_resource_id()
}

fn gzopen_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // php_stream_context_from_zval(NULL, false)
    goro_core::value::touch_default_stream_context();
    let filename = args
        .first()
        .unwrap_or(&Value::Null)
        .to_php_string()
        .to_string_lossy();
    let mode_str = args
        .get(1)
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_else(|| "r".to_string());

    // php://output and similar non-seekable streams are not supported by zlib wrapper.
    if filename.starts_with("php://output")
        || filename.starts_with("php://stdout")
        || filename.starts_with("php://stderr")
        || filename.starts_with("php://input")
    {
        vm.emit_warning(&format!(
            "gzopen({}): could not make seekable - {}",
            filename, filename
        ));
        return Ok(Value::False);
    }

    // Parse mode: extract r/w and optional compression level
    let mode_clean: String = mode_str
        .chars()
        .filter(|&c| c != 'b' && c != 't' && c != 'f' && c != 'h')
        .collect();

    // Check for r+/w+ (not allowed for gzip)
    if mode_clean.contains('+') {
        vm.emit_warning(
            "gzopen(): Cannot open a zlib stream for reading and writing at the same time!",
        );
        return Ok(Value::False);
    }

    // Strip 'e' (close-on-exec) modifier when computing r/w. PHP treats the
    // pure 'e' mode as invalid for fopen, producing a "Failed to open stream"
    // warning with the actual file name (because PHP defers to fopen's mode
    // parser). Other invalid modes (e.g. 'c') get a "gzopen failed" warning.
    let stripped_e: String = mode_clean.chars().filter(|&c| c != 'e').collect();
    let had_e = stripped_e.len() != mode_clean.len();
    let is_read = stripped_e.starts_with('r');
    let is_write =
        stripped_e.starts_with('w') || stripped_e.starts_with('a') || stripped_e.starts_with('x');

    if !is_read && !is_write {
        if had_e {
            vm.emit_warning(&format!(
                "gzopen({}): Failed to open stream: `{}' is not a valid mode for fopen",
                filename, mode_str
            ));
        } else {
            vm.emit_warning("gzopen(): gzopen failed");
        }
        return Ok(Value::False);
    }

    // Extract compression level from mode string (e.g., "w9", "w1")
    let level_from_mode: i64 = mode_clean
        .chars()
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
                    handles.borrow_mut().insert(
                        gid,
                        GzHandle::Reader {
                            decompressed,
                            pos: 0,
                            eof: false,
                            is_writer: false,
                        },
                    );
                });
                goro_core::value::register_resource(gid, b"stream");
                goro_core::value::register_close_hook(
                    gid,
                    Box::new(move || {
                        GZ_HANDLES.with(|handles| {
                            handles.borrow_mut().remove(&gid);
                        });
                    }),
                );
                Ok(Value::Resource(gid))
            }
            Err(_) => {
                vm.emit_warning(&format!(
                    "gzopen({}): Failed to open stream: No such file or directory",
                    filename
                ));
                Ok(Value::False)
            }
        }
    } else {
        // Write mode - check parent dir exists
        if let Some(parent) = std::path::Path::new(&filename).parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                vm.emit_warning(&format!(
                    "gzopen({}): Failed to open stream: No such file or directory",
                    filename
                ));
                return Ok(Value::False);
            }
        }
        let gid = alloc_gz_id();
        GZ_HANDLES.with(|handles| {
            handles.borrow_mut().insert(
                gid,
                GzHandle::Writer {
                    path: filename.to_string(),
                    buffer: Vec::new(),
                    pos: 0,
                    level: comp(level_from_mode),
                    plain: level_from_mode == 0,
                },
            );
        });
        goro_core::value::register_resource(gid, b"stream");
        // Register a close hook so fclose() flushes the gz writer to disk.
        goro_core::value::register_close_hook(
            gid,
            Box::new(move || {
                flush_gz_handle(gid);
            }),
        );
        Ok(Value::Resource(gid))
    }
}

/// Flush a gz writer handle to disk (called from gzclose or fclose hook).
fn flush_gz_handle(gid: i64) {
    let handle = GZ_HANDLES.with(|handles| handles.borrow_mut().remove(&gid));
    if let Some(handle) = handle {
        if let GzHandle::Writer {
            path,
            buffer,
            level,
            plain,
            ..
        } = handle
        {
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
        let msg = format!(
            "{}(): Argument #1 ($stream) must be an open stream resource",
            fname
        );
        let exc = vm.create_exception(b"TypeError", &msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
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
            if let GzHandle::Reader {
                decompressed,
                pos,
                eof,
                ..
            } = handle
            {
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
            if let GzHandle::Reader {
                decompressed,
                pos,
                eof,
                ..
            } = handle
            {
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
            if let GzHandle::Reader {
                decompressed,
                pos,
                eof,
                ..
            } = handle
            {
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
                GzHandle::Reader {
                    decompressed,
                    pos,
                    eof,
                    ..
                } => {
                    if *pos >= decompressed.len() {
                        *eof = true;
                    }
                    if *eof {
                        Ok(Value::True)
                    } else {
                        Ok(Value::False)
                    }
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
                GzHandle::Reader {
                    decompressed,
                    pos,
                    eof,
                    ..
                } => {
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
            if let GzHandle::Reader {
                decompressed,
                pos,
                eof,
                ..
            } = handle
            {
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
    // php_stream_context_from_zval(NULL, false)
    goro_core::value::touch_default_stream_context();
    let filename = args
        .first()
        .unwrap_or(&Value::Null)
        .to_php_string()
        .to_string_lossy();

    let data = match std::fs::read(&filename) {
        Ok(d) => d,
        Err(_) => {
            vm.emit_warning(&format!(
                "gzfile({}): Failed to open stream: No such file or directory",
                filename
            ));
            return Ok(Value::False);
        }
    };
    // The inner file stream and the zlib stream are resources.
    let _ = alloc_gz_id();

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
            None => len,                  // no more newlines
        };
        result.push(Value::String(PhpString::from_vec(
            decompressed[start..end].to_vec(),
        )));
        start = end;
    }

    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn readgzfile_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // php_stream_context_from_zval(NULL, false)
    goro_core::value::touch_default_stream_context();
    let filename = args
        .first()
        .unwrap_or(&Value::Null)
        .to_php_string()
        .to_string_lossy();

    let data = match std::fs::read(&filename) {
        Ok(d) => d,
        Err(_) => {
            vm.emit_warning(&format!(
                "readgzfile({}): Failed to open stream: No such file or directory",
                filename
            ));
            return Ok(Value::False);
        }
    };
    // The inner file stream and the zlib stream are resources.
    let _ = alloc_gz_id();

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
