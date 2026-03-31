use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

mod deflate;
mod inflate;

/// Register all zlib extension functions and constants
pub fn register(vm: &mut Vm) {
    // Register functions
    vm.register_function(b"gzcompress", gzcompress);
    vm.register_function(b"gzuncompress", gzuncompress);
    vm.register_function(b"gzdeflate", gzdeflate);
    vm.register_function(b"gzinflate", gzinflate);
    vm.register_function(b"gzencode", gzencode);
    vm.register_function(b"gzdecode", gzdecode);
    vm.register_function(b"zlib_encode", zlib_encode);
    vm.register_function(b"zlib_decode", zlib_decode);

    // Register parameter names
    vm.builtin_param_names.insert(b"gzcompress".to_vec(), vec![b"data".to_vec(), b"level".to_vec()]);
    vm.builtin_param_names.insert(b"gzuncompress".to_vec(), vec![b"data".to_vec(), b"max_length".to_vec()]);
    vm.builtin_param_names.insert(b"gzdeflate".to_vec(), vec![b"data".to_vec(), b"level".to_vec(), b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"gzinflate".to_vec(), vec![b"data".to_vec(), b"max_length".to_vec()]);
    vm.builtin_param_names.insert(b"gzencode".to_vec(), vec![b"data".to_vec(), b"level".to_vec(), b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"gzdecode".to_vec(), vec![b"data".to_vec(), b"max_length".to_vec()]);
    vm.builtin_param_names.insert(b"zlib_encode".to_vec(), vec![b"data".to_vec(), b"encoding".to_vec(), b"level".to_vec()]);
    vm.builtin_param_names.insert(b"zlib_decode".to_vec(), vec![b"data".to_vec(), b"max_length".to_vec()]);

    // Register constants
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
}

// ---- Checksum functions ----

/// Compute Adler-32 checksum
fn adler32(data: &[u8]) -> u32 {
    let mut s1: u32 = 1;
    let mut s2: u32 = 0;
    for &byte in data {
        s1 = (s1 + byte as u32) % 65521;
        s2 = (s2 + s1) % 65521;
    }
    (s2 << 16) | s1
}

/// Compute CRC-32 checksum (polynomial 0xEDB88320, standard)
fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFFFFFF;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
        }
    }
    crc ^ 0xFFFFFFFF
}

// ---- zlib format helpers ----

/// Create zlib header bytes (CMF + FLG)
fn zlib_header(level: i32) -> [u8; 2] {
    let cmf: u8 = 0x78; // CM=8 (deflate), CINFO=7 (32K window)
    let flevel = match level {
        0 | 1 => 0x00,     // fastest
        2..=5 => 0x01,     // fast
        6 => 0x02,         // default (level -1 also maps here)
        _ => 0x03,         // maximum (level 7-9)
    };
    let flg_base = flevel << 6;
    // FLG must satisfy: (CMF*256 + FLG) % 31 == 0
    let check = (cmf as u16 * 256 + flg_base as u16) % 31;
    let fcheck = if check == 0 { 0 } else { 31 - check as u8 };
    let flg = flg_base | fcheck;
    [cmf, flg]
}

/// Parse zlib header, returns offset to DEFLATE data start
fn parse_zlib_header(data: &[u8]) -> Option<usize> {
    if data.len() < 6 {
        return None; // minimum: 2 header + 1 deflate + 4 adler32 (but can be less)
    }
    let cmf = data[0];
    let flg = data[1];
    // Verify checksum
    if ((cmf as u16) * 256 + flg as u16) % 31 != 0 {
        return None;
    }
    // CM must be 8 (deflate)
    if cmf & 0x0F != 8 {
        return None;
    }
    // Check for preset dictionary (FDICT bit)
    let has_dict = (flg & 0x20) != 0;
    if has_dict {
        // We don't support preset dictionaries; skip the 4-byte dictid
        if data.len() < 10 {
            return None;
        }
        Some(6) // 2 header + 4 dictid
    } else {
        Some(2) // just the 2-byte header
    }
}

/// Create gzip header (RFC 1952)
fn gzip_header() -> Vec<u8> {
    vec![
        0x1f, 0x8b, // magic
        0x08,       // CM = deflate
        0x00,       // FLG = none
        0x00, 0x00, 0x00, 0x00, // MTIME = 0
        0x00,       // XFL
        0xff,       // OS = unknown
    ]
}

/// Parse gzip header, returns offset to DEFLATE data start
fn parse_gzip_header(data: &[u8]) -> Option<usize> {
    if data.len() < 18 {
        return None; // minimum: 10 header + 1 deflate data + 4 CRC32 + 4 ISIZE (but can have less deflate)
    }
    // Check magic
    if data[0] != 0x1f || data[1] != 0x8b {
        return None;
    }
    // Check CM
    if data[2] != 0x08 {
        return None;
    }
    let flg = data[3];
    let mut pos: usize = 10;

    // FEXTRA
    if flg & 0x04 != 0 {
        if pos + 2 > data.len() {
            return None;
        }
        let xlen = (data[pos] as usize) | ((data[pos + 1] as usize) << 8);
        pos += 2 + xlen;
    }
    // FNAME
    if flg & 0x08 != 0 {
        while pos < data.len() && data[pos] != 0 {
            pos += 1;
        }
        pos += 1; // skip null terminator
    }
    // FCOMMENT
    if flg & 0x10 != 0 {
        while pos < data.len() && data[pos] != 0 {
            pos += 1;
        }
        pos += 1;
    }
    // FHCRC
    if flg & 0x02 != 0 {
        pos += 2;
    }

    if pos >= data.len() {
        return None;
    }
    Some(pos)
}

// ---- Normalization ----

/// Normalize compression level: -1 means default (6), clamp to 0-9
fn normalize_level(level: i64) -> i32 {
    if level == -1 {
        6 // default compression
    } else {
        level.clamp(0, 9) as i32
    }
}

// ---- PHP functions ----

/// gzcompress(string $data, int $level = -1): string|false
/// Compress with zlib format (2-byte header + DEFLATE + 4-byte Adler32)
fn gzcompress(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = match args.first() {
        Some(v) => v.to_php_string(),
        None => {
            vm.emit_warning("gzcompress() expects at least 1 argument");
            return Ok(Value::False);
        }
    };
    let level = args.get(1).map(|v| v.to_long()).unwrap_or(-1);
    let level = normalize_level(level);
    let bytes = data.as_bytes();

    // zlib header
    let header = zlib_header(level);
    // Compress
    let compressed = deflate::deflate(bytes, level);
    // Adler32 checksum (big-endian)
    let checksum = adler32(bytes);

    let mut result = Vec::with_capacity(2 + compressed.len() + 4);
    result.extend_from_slice(&header);
    result.extend_from_slice(&compressed);
    result.extend_from_slice(&checksum.to_be_bytes());

    Ok(Value::String(PhpString::from_vec(result)))
}

/// gzuncompress(string $data, int $max_length = 0): string|false
/// Decompress zlib format
fn gzuncompress(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = match args.first() {
        Some(v) => v.to_php_string(),
        None => {
            vm.emit_warning("gzuncompress() expects at least 1 argument");
            return Ok(Value::False);
        }
    };
    let max_length = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let bytes = data.as_bytes();

    // Parse zlib header
    let offset = match parse_zlib_header(bytes) {
        Some(o) => o,
        None => {
            vm.emit_warning("gzuncompress(): data error");
            return Ok(Value::False);
        }
    };

    // The last 4 bytes are Adler32
    if bytes.len() < offset + 4 {
        vm.emit_warning("gzuncompress(): data error");
        return Ok(Value::False);
    }
    let deflate_data = &bytes[offset..bytes.len() - 4];
    let max_len = if max_length > 0 { Some(max_length as usize) } else { None };

    match inflate::inflate(deflate_data, max_len) {
        Ok(decompressed) => {
            // Verify Adler32
            let expected = u32::from_be_bytes([
                bytes[bytes.len() - 4],
                bytes[bytes.len() - 3],
                bytes[bytes.len() - 2],
                bytes[bytes.len() - 1],
            ]);
            let actual = adler32(&decompressed);
            if actual != expected {
                vm.emit_warning("gzuncompress(): data error");
                return Ok(Value::False);
            }
            Ok(Value::String(PhpString::from_vec(decompressed)))
        }
        Err(_) => {
            vm.emit_warning("gzuncompress(): data error");
            Ok(Value::False)
        }
    }
}

/// gzdeflate(string $data, int $level = -1, int $encoding = ZLIB_ENCODING_RAW): string|false
/// Compress raw DEFLATE
fn gzdeflate(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = match args.first() {
        Some(v) => v.to_php_string(),
        None => {
            vm.emit_warning("gzdeflate() expects at least 1 argument");
            return Ok(Value::False);
        }
    };
    let level = args.get(1).map(|v| v.to_long()).unwrap_or(-1);
    let level = normalize_level(level);
    let bytes = data.as_bytes();

    let compressed = deflate::deflate(bytes, level);
    Ok(Value::String(PhpString::from_vec(compressed)))
}

/// gzinflate(string $data, int $max_length = 0): string|false
/// Decompress raw DEFLATE
fn gzinflate(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = match args.first() {
        Some(v) => v.to_php_string(),
        None => {
            vm.emit_warning("gzinflate() expects at least 1 argument");
            return Ok(Value::False);
        }
    };
    let max_length = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let bytes = data.as_bytes();
    let max_len = if max_length > 0 { Some(max_length as usize) } else { None };

    match inflate::inflate(bytes, max_len) {
        Ok(decompressed) => Ok(Value::String(PhpString::from_vec(decompressed))),
        Err(_) => {
            vm.emit_warning("gzinflate(): data error");
            Ok(Value::False)
        }
    }
}

/// gzencode(string $data, int $level = -1, int $encoding = FORCE_GZIP): string|false
/// Compress with gzip format (10-byte header + DEFLATE + CRC32 + size)
fn gzencode(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = match args.first() {
        Some(v) => v.to_php_string(),
        None => {
            vm.emit_warning("gzencode() expects at least 1 argument");
            return Ok(Value::False);
        }
    };
    let level = args.get(1).map(|v| v.to_long()).unwrap_or(-1);
    let encoding = args.get(2).map(|v| v.to_long()).unwrap_or(31); // FORCE_GZIP
    let level = normalize_level(level);
    let bytes = data.as_bytes();

    match encoding {
        31 => {
            // GZIP format
            let header = gzip_header();
            let compressed = deflate::deflate(bytes, level);
            let crc = crc32(bytes);
            let size = (bytes.len() as u32).to_le_bytes();

            let mut result = Vec::with_capacity(header.len() + compressed.len() + 8);
            result.extend_from_slice(&header);
            result.extend_from_slice(&compressed);
            result.extend_from_slice(&crc.to_le_bytes());
            result.extend_from_slice(&size);

            Ok(Value::String(PhpString::from_vec(result)))
        }
        15 => {
            // ZLIB format (same as gzcompress)
            let header = zlib_header(level);
            let compressed = deflate::deflate(bytes, level);
            let checksum = adler32(bytes);

            let mut result = Vec::with_capacity(2 + compressed.len() + 4);
            result.extend_from_slice(&header);
            result.extend_from_slice(&compressed);
            result.extend_from_slice(&checksum.to_be_bytes());

            Ok(Value::String(PhpString::from_vec(result)))
        }
        -15 => {
            // Raw DEFLATE
            let compressed = deflate::deflate(bytes, level);
            Ok(Value::String(PhpString::from_vec(compressed)))
        }
        _ => {
            vm.emit_warning("gzencode(): encoding mode must be FORCE_GZIP, FORCE_DEFLATE or ZLIB_ENCODING_RAW");
            Ok(Value::False)
        }
    }
}

/// gzdecode(string $data, int $max_length = 0): string|false
/// Decompress gzip format
fn gzdecode(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = match args.first() {
        Some(v) => v.to_php_string(),
        None => {
            vm.emit_warning("gzdecode() expects at least 1 argument");
            return Ok(Value::False);
        }
    };
    let max_length = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let bytes = data.as_bytes();

    // Parse gzip header
    let offset = match parse_gzip_header(bytes) {
        Some(o) => o,
        None => {
            vm.emit_warning("gzdecode(): data error");
            return Ok(Value::False);
        }
    };

    // The last 8 bytes are CRC32 + ISIZE
    if bytes.len() < offset + 8 {
        vm.emit_warning("gzdecode(): data error");
        return Ok(Value::False);
    }
    let deflate_data = &bytes[offset..bytes.len() - 8];
    let max_len = if max_length > 0 { Some(max_length as usize) } else { None };

    match inflate::inflate(deflate_data, max_len) {
        Ok(decompressed) => {
            // Verify CRC32
            let expected_crc = u32::from_le_bytes([
                bytes[bytes.len() - 8],
                bytes[bytes.len() - 7],
                bytes[bytes.len() - 6],
                bytes[bytes.len() - 5],
            ]);
            let actual_crc = crc32(&decompressed);
            if actual_crc != expected_crc {
                vm.emit_warning("gzdecode(): data error");
                return Ok(Value::False);
            }
            // Verify ISIZE (original size mod 2^32)
            let expected_size = u32::from_le_bytes([
                bytes[bytes.len() - 4],
                bytes[bytes.len() - 3],
                bytes[bytes.len() - 2],
                bytes[bytes.len() - 1],
            ]);
            if (decompressed.len() as u32) != expected_size {
                vm.emit_warning("gzdecode(): data error");
                return Ok(Value::False);
            }
            Ok(Value::String(PhpString::from_vec(decompressed)))
        }
        Err(_) => {
            vm.emit_warning("gzdecode(): data error");
            Ok(Value::False)
        }
    }
}

/// zlib_encode(string $data, int $encoding, int $level = -1): string|false
fn zlib_encode(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = match args.first() {
        Some(v) => v.to_php_string(),
        None => {
            vm.emit_warning("zlib_encode() expects at least 2 arguments");
            return Ok(Value::False);
        }
    };
    let encoding = match args.get(1) {
        Some(v) => v.to_long(),
        None => {
            vm.emit_warning("zlib_encode() expects at least 2 arguments");
            return Ok(Value::False);
        }
    };
    let level = args.get(2).map(|v| v.to_long()).unwrap_or(-1);
    let level = normalize_level(level);
    let bytes = data.as_bytes();

    match encoding {
        31 => {
            // GZIP format
            let header = gzip_header();
            let compressed = deflate::deflate(bytes, level);
            let crc = crc32(bytes);
            let size = (bytes.len() as u32).to_le_bytes();

            let mut result = Vec::with_capacity(header.len() + compressed.len() + 8);
            result.extend_from_slice(&header);
            result.extend_from_slice(&compressed);
            result.extend_from_slice(&crc.to_le_bytes());
            result.extend_from_slice(&size);

            Ok(Value::String(PhpString::from_vec(result)))
        }
        15 => {
            // ZLIB format
            let header = zlib_header(level);
            let compressed = deflate::deflate(bytes, level);
            let checksum = adler32(bytes);

            let mut result = Vec::with_capacity(2 + compressed.len() + 4);
            result.extend_from_slice(&header);
            result.extend_from_slice(&compressed);
            result.extend_from_slice(&checksum.to_be_bytes());

            Ok(Value::String(PhpString::from_vec(result)))
        }
        -15 => {
            // Raw DEFLATE
            let compressed = deflate::deflate(bytes, level);
            Ok(Value::String(PhpString::from_vec(compressed)))
        }
        _ => {
            vm.emit_warning("zlib_encode(): encoding mode must be ZLIB_ENCODING_RAW, ZLIB_ENCODING_GZIP or ZLIB_ENCODING_DEFLATE");
            Ok(Value::False)
        }
    }
}

/// zlib_decode(string $data, int $max_length = 0): string|false
/// Auto-detects format (gzip, zlib, or raw deflate)
fn zlib_decode(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = match args.first() {
        Some(v) => v.to_php_string(),
        None => {
            vm.emit_warning("zlib_decode() expects at least 1 argument");
            return Ok(Value::False);
        }
    };
    let max_length = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let bytes = data.as_bytes();
    let max_len = if max_length > 0 { Some(max_length as usize) } else { None };

    // Try gzip first
    if bytes.len() >= 2 && bytes[0] == 0x1f && bytes[1] == 0x8b {
        if let Some(offset) = parse_gzip_header(bytes) {
            if bytes.len() >= offset + 8 {
                let deflate_data = &bytes[offset..bytes.len() - 8];
                if let Ok(decompressed) = inflate::inflate(deflate_data, max_len) {
                    return Ok(Value::String(PhpString::from_vec(decompressed)));
                }
            }
        }
    }

    // Try zlib
    if bytes.len() >= 2 && (bytes[0] & 0x0F) == 8 {
        if let Some(offset) = parse_zlib_header(bytes) {
            if bytes.len() >= offset + 4 {
                let deflate_data = &bytes[offset..bytes.len() - 4];
                if let Ok(decompressed) = inflate::inflate(deflate_data, max_len) {
                    return Ok(Value::String(PhpString::from_vec(decompressed)));
                }
            }
        }
    }

    // Try raw deflate
    if let Ok(decompressed) = inflate::inflate(bytes, max_len) {
        return Ok(Value::String(PhpString::from_vec(decompressed)));
    }

    vm.emit_warning("zlib_decode(): data error");
    Ok(Value::False)
}
