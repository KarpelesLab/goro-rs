mod compress;
mod decompress;

use goro_core::value::Value;
use goro_core::string::PhpString;
use goro_core::vm::{Vm, VmError};

/// Register all bz2 extension functions
pub fn register(vm: &mut Vm) {
    vm.register_function(b"bzcompress", php_bzcompress);
    vm.register_function(b"bzdecompress", php_bzdecompress);
}

/// bzcompress(data, block_size=4, work_factor=0)
/// Compresses data using bzip2 algorithm
/// Returns compressed string on success, or error code (int) on failure
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

    // work_factor is accepted but ignored (only affects compression speed, not output)
    let _work_factor = match args.get(2) {
        Some(v) => {
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
            wf as u32
        }
        None => 0,
    };

    let compressed = compress::bzcompress(data.as_bytes(), block_size);
    Ok(Value::String(PhpString::from_vec(compressed)))
}

/// bzdecompress(data, use_less_memory=false)
/// Decompresses bzip2 compressed data
/// Returns decompressed string on success, or error code (int) on failure
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

    // use_less_memory is accepted but ignored (implementation detail)
    let _use_less_memory = match args.get(1) {
        Some(v) => v.is_truthy(),
        None => false,
    };

    match decompress::bzdecompress(data.as_bytes()) {
        Ok(decompressed) => Ok(Value::String(PhpString::from_vec(decompressed))),
        Err(code) => Ok(Value::Long(code)),
    }
}
