use std::io::{Read, Write};

use bzip2::Compression;
use bzip2::read::BzDecoder;
use bzip2::write::BzEncoder;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

/// Register all bz2 extension functions
pub fn register(vm: &mut Vm) {
    vm.register_extension(b"bz2");
    vm.register_function(b"bzcompress", php_bzcompress);
    vm.register_function(b"bzdecompress", php_bzdecompress);

    // Stub functions for stream-based bzip2 operations
    vm.register_function(b"bzopen", php_bz_stub_false);
    vm.register_function(b"bzread", php_bz_stub_false);
    vm.register_function(b"bzwrite", php_bz_stub_false);
    vm.register_function(b"bzclose", php_bz_stub_false);
    vm.register_function(b"bzerrno", php_bz_stub_zero);
    vm.register_function(b"bzerror", php_bz_stub_false);
    vm.register_function(b"bzerrstr", php_bz_stub_false);
}

/// bzcompress(data, block_size=4, work_factor=0)
/// Compresses data using bzip2 algorithm.
/// Returns compressed string on success, or error code (int) on failure.
fn php_bzcompress(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = match args.first() {
        Some(v) => v.to_php_string(),
        None => {
            return Err(VmError {
                message: "bzcompress() expects at least 1 argument".to_string(),
                line: 0,
            });
        }
    };

    let block_size = match args.get(1) {
        Some(v) => {
            let bs = v.to_long();
            if bs < 1 || bs > 9 {
                return Err(VmError {
                    message: format!(
                        "bzcompress(): Argument #2 ($block_size) must be between 1 and 9, {} given",
                        bs
                    ),
                    line: 0,
                });
            }
            bs as u32
        }
        None => 4,
    };

    // work_factor is accepted but ignored (the bzip2 crate does not expose it)
    if let Some(v) = args.get(2) {
        let wf = v.to_long();
        if wf < 0 || wf > 250 {
            return Err(VmError {
                message: format!(
                    "bzcompress(): Argument #3 ($work_factor) must be between 0 and 250, {} given",
                    wf
                ),
                line: 0,
            });
        }
    }

    let level = Compression::new(block_size);
    let mut encoder = BzEncoder::new(Vec::new(), level);

    if encoder.write_all(data.as_bytes()).is_err() {
        return Ok(Value::Long(-6)); // BZ_IO_ERROR
    }

    match encoder.finish() {
        Ok(compressed) => Ok(Value::String(PhpString::from_vec(compressed))),
        Err(_) => Ok(Value::Long(-6)), // BZ_IO_ERROR
    }
}

/// bzdecompress(data, use_less_memory=false)
/// Decompresses bzip2 compressed data.
/// Returns decompressed string on success, or error code (int) on failure.
fn php_bzdecompress(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = match args.first() {
        Some(v) => v.to_php_string(),
        None => {
            return Err(VmError {
                message: "bzdecompress() expects at least 1 argument".to_string(),
                line: 0,
            });
        }
    };

    // use_less_memory is accepted but not used (the bzip2 crate handles this internally)
    if let Some(v) = args.get(1) {
        let _ = v.is_truthy();
    }

    let mut decoder = BzDecoder::new(data.as_bytes());
    let mut decompressed = Vec::new();

    match decoder.read_to_end(&mut decompressed) {
        Ok(_) => Ok(Value::String(PhpString::from_vec(decompressed))),
        Err(e) => {
            // Map I/O errors to bzip2 error codes
            let code = match e.kind() {
                std::io::ErrorKind::UnexpectedEof => -7, // BZ_UNEXPECTED_EOF
                _ => -4,                                 // BZ_DATA_ERROR
            };
            Ok(Value::Long(code))
        }
    }
}

/// Stub that returns false for unimplemented stream functions
fn php_bz_stub_false(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}

/// Stub that returns 0 for bzerrno
fn php_bz_stub_zero(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(0))
}
