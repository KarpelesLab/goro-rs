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

    // bzopen() & co. and the bz* aliases of the stream functions are served
    // by the stream layer of ext/standard.
    goro_ext_standard::register_bz2_stream_functions(vm);

    vm.builtin_param_names.insert(
        b"bzcompress".to_vec(),
        vec![
            b"data".to_vec(),
            b"block_size".to_vec(),
            b"work_factor".to_vec(),
        ],
    );
    vm.builtin_param_names.insert(
        b"bzdecompress".to_vec(),
        vec![b"data".to_vec(), b"use_less_memory".to_vec()],
    );
    vm.builtin_param_names
        .insert(b"bzopen".to_vec(), vec![b"file".to_vec(), b"mode".to_vec()]);
    vm.builtin_param_names
        .insert(b"bzread".to_vec(), vec![b"bz".to_vec(), b"length".to_vec()]);
    vm.builtin_param_names.insert(
        b"bzwrite".to_vec(),
        vec![b"bz".to_vec(), b"data".to_vec(), b"length".to_vec()],
    );
    vm.builtin_param_names
        .insert(b"bzclose".to_vec(), vec![b"bz".to_vec()]);
    vm.builtin_param_names
        .insert(b"bzflush".to_vec(), vec![b"bz".to_vec()]);
    vm.builtin_param_names
        .insert(b"bzerrno".to_vec(), vec![b"bz".to_vec()]);
    vm.builtin_param_names
        .insert(b"bzerror".to_vec(), vec![b"bz".to_vec()]);
    vm.builtin_param_names
        .insert(b"bzerrstr".to_vec(), vec![b"bz".to_vec()]);
}

/// bzcompress(data, block_size=4, work_factor=0)
fn php_bzcompress(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = match args.first() {
        Some(v) => v.to_php_string(),
        None => {
            let msg = "bzcompress() expects at least 1 argument";
            let exc = vm.create_exception(b"ArgumentCountError", msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg.to_string(),
                line: vm.current_line,
            });
        }
    };

    // libbz2 takes the input length as a 32-bit unsigned int, so reject any
    // string whose length cannot be expressed.
    if data.as_bytes().len() > u32::MAX as usize {
        let msg = format!(
            "bzcompress(): Argument #1 ($data) must have a length less than or equal to {}",
            u32::MAX
        );
        let exc = vm.create_exception(b"ValueError", &msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }

    let block_size = match args.get(1) {
        Some(Value::Null) | Some(Value::Undef) | None => 4,
        Some(v) => {
            let bs = v.to_long();
            if !(1..=9).contains(&bs) {
                let msg = "bzcompress(): Argument #2 ($block_size) must be between 1 and 9";
                let exc = vm.create_exception(b"ValueError", msg, 0);
                vm.current_exception = Some(exc);
                return Err(VmError {
                    message: msg.to_string(),
                    line: vm.current_line,
                });
            }
            bs as u32
        }
    };

    match args.get(2) {
        Some(Value::Null) | Some(Value::Undef) | None => {}
        Some(v) => {
            let wf = v.to_long();
            if !(0..=250).contains(&wf) {
                let msg = "bzcompress(): Argument #3 ($work_factor) must be between 0 and 250";
                let exc = vm.create_exception(b"ValueError", msg, 0);
                vm.current_exception = Some(exc);
                return Err(VmError {
                    message: msg.to_string(),
                    line: vm.current_line,
                });
            }
        }
    }

    let level = Compression::new(block_size);
    let mut encoder = BzEncoder::new(Vec::new(), level);

    if encoder.write_all(data.as_bytes()).is_err() {
        return Ok(Value::Long(-6));
    }

    match encoder.finish() {
        Ok(compressed) => Ok(Value::String(PhpString::from_vec(compressed))),
        Err(_) => Ok(Value::Long(-6)),
    }
}

/// bzdecompress(data, use_less_memory=false)
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

    if let Some(v) = args.get(1) {
        let _ = v.is_truthy();
    }

    // PHP returns BZ_DATA_ERROR_MAGIC (-5) for non-bzip2 data
    let bytes = data.as_bytes();
    if bytes.len() < 4 || &bytes[..3] != b"BZh" || !(bytes[3] >= b'1' && bytes[3] <= b'9') {
        return Ok(Value::Long(-5));
    }

    let mut decoder = BzDecoder::new(bytes);
    let mut decompressed = Vec::new();

    match decoder.read_to_end(&mut decompressed) {
        Ok(_) => Ok(Value::String(PhpString::from_vec(decompressed))),
        Err(e) => {
            let code = match e.kind() {
                std::io::ErrorKind::UnexpectedEof => -7, // BZ_UNEXPECTED_EOF
                _ => -4,                                 // BZ_DATA_ERROR
            };
            Ok(Value::Long(code))
        }
    }
}
