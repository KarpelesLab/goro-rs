use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use goro_ext_standard::bz2codec;

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

    // The output buffer (len + len / 100 + 600 bytes) has its size in a
    // 32-bit unsigned int.
    let len = data.as_bytes().len();
    if len + len / 100 + 600 > u32::MAX as usize {
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

    let work_factor = match args.get(2) {
        Some(Value::Null) | Some(Value::Undef) | None => 0,
        Some(v) => v.to_long() as u32,
    };
    // BZ2_bzBuffToBuffCompress()
    let compressed = bz2codec::compress(data.as_bytes(), block_size, work_factor);
    Ok(Value::String(PhpString::from_vec(compressed)))
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

    // BZ2_bzDecompress() until the end of the stream or of the input: a
    // truncated stream gives the data decoded until then.
    match bz2codec::decompress(data.as_bytes()) {
        (out, Ok(_)) => Ok(Value::String(PhpString::from_vec(out))),
        (_, Err(e)) => Ok(Value::Long(e.code() as i64)),
    }
}
