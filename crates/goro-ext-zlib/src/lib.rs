use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

use flate2::read::{DeflateDecoder, GzDecoder, ZlibDecoder};
use flate2::write::{DeflateEncoder, GzEncoder, ZlibEncoder};
use flate2::Compression;
use std::io::{Read, Write};

pub fn register(vm: &mut Vm) {
    vm.register_function(b"gzcompress", gzcompress);
    vm.register_function(b"gzuncompress", gzuncompress);
    vm.register_function(b"gzdeflate", gzdeflate);
    vm.register_function(b"gzinflate", gzinflate);
    vm.register_function(b"gzencode", gzencode);
    vm.register_function(b"gzdecode", gzdecode);
    vm.register_function(b"zlib_encode", zlib_encode);
    vm.register_function(b"zlib_decode", zlib_decode);

    vm.builtin_param_names.insert(b"gzcompress".to_vec(), vec![b"data".to_vec(), b"level".to_vec()]);
    vm.builtin_param_names.insert(b"gzuncompress".to_vec(), vec![b"data".to_vec(), b"max_length".to_vec()]);
    vm.builtin_param_names.insert(b"gzdeflate".to_vec(), vec![b"data".to_vec(), b"level".to_vec(), b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"gzinflate".to_vec(), vec![b"data".to_vec(), b"max_length".to_vec()]);
    vm.builtin_param_names.insert(b"gzencode".to_vec(), vec![b"data".to_vec(), b"level".to_vec(), b"encoding".to_vec()]);
    vm.builtin_param_names.insert(b"gzdecode".to_vec(), vec![b"data".to_vec(), b"max_length".to_vec()]);
    vm.builtin_param_names.insert(b"zlib_encode".to_vec(), vec![b"data".to_vec(), b"encoding".to_vec(), b"level".to_vec()]);
    vm.builtin_param_names.insert(b"zlib_decode".to_vec(), vec![b"data".to_vec(), b"max_length".to_vec()]);

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

fn comp(level: i64) -> Compression {
    if level == -1 { Compression::default() } else { Compression::new(level.clamp(0, 9) as u32) }
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

fn gzcompress(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = args.first().unwrap_or(&Value::Null).to_php_string();
    let level = args.get(1).map(|v| v.to_long()).unwrap_or(-1);
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
    let mut dec = ZlibDecoder::new(data.as_bytes());
    match read_limited(&mut dec, max) {
        Ok(d) => Ok(Value::String(PhpString::from_vec(d))),
        Err(_) => { vm.emit_warning("gzuncompress(): data error"); Ok(Value::False) }
    }
}

fn gzdeflate(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = args.first().unwrap_or(&Value::Null).to_php_string();
    let level = args.get(1).map(|v| v.to_long()).unwrap_or(-1);
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
    let mut dec = DeflateDecoder::new(data.as_bytes());
    match read_limited(&mut dec, max) {
        Ok(d) => Ok(Value::String(PhpString::from_vec(d))),
        Err(_) => { vm.emit_warning("gzinflate(): data error"); Ok(Value::False) }
    }
}

fn gzencode(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = args.first().unwrap_or(&Value::Null).to_php_string();
    let level = args.get(1).map(|v| v.to_long()).unwrap_or(-1);
    let encoding = args.get(2).map(|v| v.to_long()).unwrap_or(31);
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

fn zlib_encode(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = args.first().unwrap_or(&Value::Null).to_php_string();
    let encoding = args.get(1).map(|v| v.to_long()).unwrap_or(15);
    let level = args.get(2).map(|v| v.to_long()).unwrap_or(-1);
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
