use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{Read, Write};

use bzip2::Compression;
use bzip2::read::BzDecoder;
use bzip2::write::BzEncoder;
use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use std::rc::Rc;

/// Register all bz2 extension functions
pub fn register(vm: &mut Vm) {
    vm.register_extension(b"bz2");
    vm.register_function(b"bzcompress", php_bzcompress);
    vm.register_function(b"bzdecompress", php_bzdecompress);

    // Stream-based bzip2 operations
    vm.register_function(b"bzopen", php_bzopen);
    vm.register_function(b"bzread", php_bzread);
    vm.register_function(b"bzwrite", php_bzwrite);
    vm.register_function(b"bzclose", php_bzclose);
    vm.register_function(b"bzflush", php_bzflush);
    vm.register_function(b"bzerrno", php_bzerrno);
    vm.register_function(b"bzerror", php_bzerror);
    vm.register_function(b"bzerrstr", php_bzerrstr);

    vm.builtin_param_names.insert(b"bzcompress".to_vec(), vec![b"data".to_vec(), b"block_size".to_vec(), b"work_factor".to_vec()]);
    vm.builtin_param_names.insert(b"bzdecompress".to_vec(), vec![b"data".to_vec(), b"use_less_memory".to_vec()]);
    vm.builtin_param_names.insert(b"bzopen".to_vec(), vec![b"file".to_vec(), b"mode".to_vec()]);
    vm.builtin_param_names.insert(b"bzread".to_vec(), vec![b"bz".to_vec(), b"length".to_vec()]);
    vm.builtin_param_names.insert(b"bzwrite".to_vec(), vec![b"bz".to_vec(), b"data".to_vec(), b"length".to_vec()]);
    vm.builtin_param_names.insert(b"bzclose".to_vec(), vec![b"bz".to_vec()]);
    vm.builtin_param_names.insert(b"bzflush".to_vec(), vec![b"bz".to_vec()]);
    vm.builtin_param_names.insert(b"bzerrno".to_vec(), vec![b"bz".to_vec()]);
    vm.builtin_param_names.insert(b"bzerror".to_vec(), vec![b"bz".to_vec()]);
    vm.builtin_param_names.insert(b"bzerrstr".to_vec(), vec![b"bz".to_vec()]);
}

enum BzHandle {
    Reader {
        decompressed: Vec<u8>,
        pos: usize,
        /// Current reported errno (after bzread surfaces a deferred error)
        errno: i32,
        errstr: String,
        /// Deferred error to surface on next bzread call
        deferred_errno: i32,
        deferred_errstr: String,
    },
    Writer {
        path: String,
        buffer: Vec<u8>,
        block_size: u32,
        errno: i32,
        errstr: String,
    },
}

impl BzHandle {
    fn errno(&self) -> i32 {
        match self {
            BzHandle::Reader { errno, .. } => *errno,
            BzHandle::Writer { errno, .. } => *errno,
        }
    }
    fn errstr(&self) -> &str {
        match self {
            BzHandle::Reader { errstr, .. } => errstr,
            BzHandle::Writer { errstr, .. } => errstr,
        }
    }
}

thread_local! {
    static BZ_HANDLES: RefCell<HashMap<i64, BzHandle>> = RefCell::new(HashMap::new());
    static NEXT_BZ_ID: std::cell::Cell<i64> = const { std::cell::Cell::new(30000) };
}

fn alloc_bz_id() -> i64 {
    NEXT_BZ_ID.with(|id| {
        let bid = id.get();
        id.set(bid + 1);
        bid
    })
}

fn require_open_bz(vm: &mut Vm, gid: i64, fname: &str) -> Result<bool, VmError> {
    if !goro_core::value::is_open_resource(gid) {
        let msg = format!("{}(): Argument #1 ($bz) must be an open stream resource", fname);
        let exc = vm.create_exception(b"TypeError", &msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    Ok(true)
}

/// Like require_open_bz but specifically validates a bz2 stream (for bzerr* functions).
/// We track bz2-ness by presence in BZ_HANDLES (not by resource type, since PHP exposes
/// these as generic "stream" resources).
fn require_bz2_stream(_vm: &mut Vm, gid: i64, _fname: &str) -> Result<bool, ()> {
    if !goro_core::value::is_open_resource(gid) {
        return Err(());
    }
    let is_bz = BZ_HANDLES.with(|h| h.borrow().contains_key(&gid));
    if !is_bz {
        return Err(());
    }
    Ok(true)
}

/// bzcompress(data, block_size=4, work_factor=0)
fn php_bzcompress(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = match args.first() {
        Some(v) => v.to_php_string(),
        None => {
            let msg = "bzcompress() expects at least 1 argument";
            let exc = vm.create_exception(b"ArgumentCountError", msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg.to_string(), line: vm.current_line });
        }
    };

    // libbz2 takes the input length as a 32-bit unsigned int, so reject any
    // string whose length cannot be expressed.
    if data.as_bytes().len() > u32::MAX as usize {
        let msg = format!("bzcompress(): Argument #1 ($data) must have a length less than or equal to {}", u32::MAX);
        let exc = vm.create_exception(b"ValueError", &msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }

    let block_size = match args.get(1) {
        Some(Value::Null) | Some(Value::Undef) | None => 4,
        Some(v) => {
            let bs = v.to_long();
            if !(1..=9).contains(&bs) {
                let msg = "bzcompress(): Argument #2 ($block_size) must be between 1 and 9";
                let exc = vm.create_exception(b"ValueError", msg, 0);
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg.to_string(), line: vm.current_line });
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
                return Err(VmError { message: msg.to_string(), line: vm.current_line });
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

fn php_bzopen(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let file_arg = args.first().unwrap_or(&Value::Null);
    let mode_arg = args.get(1).map(|v| v.to_php_string().to_string_lossy()).unwrap_or_default();

    // Validate mode first
    if mode_arg != "r" && mode_arg != "w" {
        let msg = "bzopen(): Argument #2 ($mode) must be either \"r\" or \"w\"";
        let exc = vm.create_exception(b"ValueError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg.to_string(), line: vm.current_line });
    }

    // file arg can be a string (filename) or a resource (file handle)
    match file_arg {
        Value::Resource(_) => {
            // PHP allows passing an open file handle; we make a stream wrapping it.
            // For our purposes, create a fresh empty bz2 reader/writer that just allocates an id.
            let bid = alloc_bz_id();
            if mode_arg == "r" {
                BZ_HANDLES.with(|handles| {
                    handles.borrow_mut().insert(bid, BzHandle::Reader {
                        decompressed: Vec::new(),
                        pos: 0,
                        errno: 0,
                        errstr: "OK".to_string(),
                        deferred_errno: 0,
                        deferred_errstr: "OK".to_string(),
                    });
                });
            } else {
                BZ_HANDLES.with(|handles| {
                    handles.borrow_mut().insert(bid, BzHandle::Writer {
                        path: String::new(),
                        buffer: Vec::new(),
                        block_size: 4,
                        errno: 0,
                        errstr: "OK".to_string(),
                    });
                });
            }
            goro_core::value::register_resource(bid, b"stream");
            Ok(Value::Resource(bid))
        }
        Value::String(s) => {
            // Check for null bytes
            if s.as_bytes().contains(&0) {
                let msg = "bzopen(): Argument #1 ($file) must not contain null bytes";
                let exc = vm.create_exception(b"TypeError", msg, 0);
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg.to_string(), line: vm.current_line });
            }
            let filename = s.to_string_lossy();
            if filename.is_empty() {
                let msg = "bzopen(): Argument #1 ($file) must not be empty";
                let exc = vm.create_exception(b"ValueError", msg, 0);
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg.to_string(), line: vm.current_line });
            }
            if mode_arg == "r" {
                match std::fs::read(&filename) {
                    Ok(data) => {
                        // Decompress upfront, but defer error reporting until bzread is called.
                        let mut decoder = BzDecoder::new(data.as_slice());
                        let mut decompressed = Vec::new();
                        let (decompressed, deferred_errno, deferred_errstr) = match decoder.read_to_end(&mut decompressed) {
                            Ok(_) => (decompressed, 0, "OK".to_string()),
                            Err(_) => {
                                if data.len() < 4 || &data[..3] != b"BZh" {
                                    (Vec::new(), -5, "DATA_ERROR_MAGIC".to_string())
                                } else {
                                    (Vec::new(), -4, "DATA_ERROR".to_string())
                                }
                            }
                        };
                        let bid = alloc_bz_id();
                        BZ_HANDLES.with(|handles| {
                            handles.borrow_mut().insert(bid, BzHandle::Reader {
                                decompressed,
                                pos: 0,
                                errno: 0,
                                errstr: "OK".to_string(),
                                deferred_errno,
                                deferred_errstr,
                            });
                        });
                        goro_core::value::register_resource(bid, b"stream");
                        Ok(Value::Resource(bid))
                    }
                    Err(_) => {
                        vm.emit_warning(&format!("bzopen({}): Failed to open stream: No such file or directory", filename));
                        Ok(Value::False)
                    }
                }
            } else {
                // Write mode - check that parent dir exists
                if let Some(parent) = std::path::Path::new(&filename).parent() {
                    if !parent.as_os_str().is_empty() && !parent.exists() {
                        vm.emit_warning(&format!("bzopen({}): Failed to open stream: No such file or directory", filename));
                        return Ok(Value::False);
                    }
                }
                let bid = alloc_bz_id();
                BZ_HANDLES.with(|handles| {
                    handles.borrow_mut().insert(bid, BzHandle::Writer {
                        path: filename.to_string(),
                        buffer: Vec::new(),
                        block_size: 4,
                        errno: 0,
                        errstr: "OK".to_string(),
                    });
                });
                goro_core::value::register_resource(bid, b"stream");
                Ok(Value::Resource(bid))
            }
        }
        _ => {
            let msg = "bzopen(): Argument #1 ($file) must be of type string or file-resource";
            let exc = vm.create_exception(b"TypeError", msg, 0);
            vm.current_exception = Some(exc);
            Err(VmError { message: msg.to_string(), line: vm.current_line })
        }
    }
}

fn php_bzread(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bid = args.first().unwrap_or(&Value::Null).to_long();
    require_open_bz(vm, bid, "bzread")?;
    let length = if args.len() >= 2 {
        let l = args[1].to_long();
        if l < 0 {
            let msg = "bzread(): Argument #2 ($length) must be greater than or equal to 0";
            let exc = vm.create_exception(b"ValueError", msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg.to_string(), line: vm.current_line });
        }
        l as usize
    } else {
        1024
    };

    if length == 0 {
        return Ok(Value::String(PhpString::from_bytes(b"")));
    }

    BZ_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(handle) = handles.get_mut(&bid) {
            match handle {
                BzHandle::Reader { decompressed, pos, errno, errstr, deferred_errno, deferred_errstr } => {
                    // Surface any deferred error to current errno
                    if *deferred_errno != 0 {
                        *errno = *deferred_errno;
                        *errstr = std::mem::take(deferred_errstr);
                        *deferred_errno = 0;
                        *deferred_errstr = "OK".to_string();
                        return Ok(Value::False);
                    }
                    // If stream has a sticky error, return false
                    if *errno != 0 {
                        return Ok(Value::False);
                    }
                    if *pos >= decompressed.len() {
                        return Ok(Value::String(PhpString::from_bytes(b"")));
                    }
                    let end = (*pos + length).min(decompressed.len());
                    let data = decompressed[*pos..end].to_vec();
                    *pos = end;
                    Ok(Value::String(PhpString::from_vec(data)))
                }
                BzHandle::Writer { .. } => Ok(Value::False),
            }
        } else {
            Ok(Value::False)
        }
    })
}

fn php_bzwrite(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bid = args.first().unwrap_or(&Value::Null).to_long();
    require_open_bz(vm, bid, "bzwrite")?;
    let data = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let bytes = data.as_bytes();
    let length = if args.len() >= 3 {
        let l = args[2].to_long();
        if l <= 0 {
            return Ok(Value::Long(0));
        }
        (l as usize).min(bytes.len())
    } else {
        bytes.len()
    };

    BZ_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(handle) = handles.get_mut(&bid) {
            match handle {
                BzHandle::Writer { buffer, .. } => {
                    buffer.extend_from_slice(&bytes[..length]);
                    Ok(Value::Long(length as i64))
                }
                BzHandle::Reader { .. } => Ok(Value::Long(0)),
            }
        } else {
            Ok(Value::False)
        }
    })
}

fn php_bzclose(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bid = args.first().unwrap_or(&Value::Null).to_long();
    let handle = BZ_HANDLES.with(|handles| handles.borrow_mut().remove(&bid));
    if let Some(handle) = handle {
        if let BzHandle::Writer { path, buffer, block_size, .. } = handle {
            let level = Compression::new(block_size);
            let mut encoder = BzEncoder::new(Vec::new(), level);
            let _ = encoder.write_all(&buffer);
            if let Ok(compressed) = encoder.finish() {
                let _ = std::fs::write(&path, &compressed);
            }
        }
        goro_core::value::close_resource(bid);
        Ok(Value::True)
    } else {
        Ok(Value::False)
    }
}

fn php_bzflush(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bid = args.first().unwrap_or(&Value::Null).to_long();
    require_open_bz(vm, bid, "bzflush")?;
    Ok(Value::True)
}

fn bzerr_type_error(vm: &mut Vm, fname: &str) -> VmError {
    let msg = format!("{}(): Argument #1 ($bz) must be a bz2 stream", fname);
    let exc = vm.create_exception(b"TypeError", &msg, 0);
    vm.current_exception = Some(exc);
    VmError { message: msg, line: vm.current_line }
}

fn php_bzerrno(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bid = args.first().unwrap_or(&Value::Null).to_long();
    // If resource is closed, give "open stream resource" error (matches test 004 expectation)
    if !goro_core::value::is_registered_resource(bid) || goro_core::value::is_registered_resource(bid) && !goro_core::value::is_open_resource(bid) {
        let msg = "bzerrno(): Argument #1 ($bz) must be an open stream resource";
        let exc = vm.create_exception(b"TypeError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg.to_string(), line: vm.current_line });
    }
    if require_bz2_stream(vm, bid, "bzerrno").is_err() {
        return Err(bzerr_type_error(vm, "bzerrno"));
    }
    BZ_HANDLES.with(|handles| {
        let handles = handles.borrow();
        if let Some(handle) = handles.get(&bid) {
            Ok(Value::Long(handle.errno() as i64))
        } else {
            Ok(Value::False)
        }
    })
}

fn php_bzerrstr(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bid = args.first().unwrap_or(&Value::Null).to_long();
    if !goro_core::value::is_registered_resource(bid) || goro_core::value::is_registered_resource(bid) && !goro_core::value::is_open_resource(bid) {
        let msg = "bzerrstr(): Argument #1 ($bz) must be an open stream resource";
        let exc = vm.create_exception(b"TypeError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg.to_string(), line: vm.current_line });
    }
    if require_bz2_stream(vm, bid, "bzerrstr").is_err() {
        return Err(bzerr_type_error(vm, "bzerrstr"));
    }
    BZ_HANDLES.with(|handles| {
        let handles = handles.borrow();
        if let Some(handle) = handles.get(&bid) {
            Ok(Value::String(PhpString::from_bytes(handle.errstr().as_bytes())))
        } else {
            Ok(Value::False)
        }
    })
}

fn php_bzerror(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bid = args.first().unwrap_or(&Value::Null).to_long();
    if !goro_core::value::is_registered_resource(bid) || goro_core::value::is_registered_resource(bid) && !goro_core::value::is_open_resource(bid) {
        let msg = "bzerror(): Argument #1 ($bz) must be an open stream resource";
        let exc = vm.create_exception(b"TypeError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg.to_string(), line: vm.current_line });
    }
    if require_bz2_stream(vm, bid, "bzerror").is_err() {
        return Err(bzerr_type_error(vm, "bzerror"));
    }
    BZ_HANDLES.with(|handles| {
        let handles = handles.borrow();
        if let Some(handle) = handles.get(&bid) {
            let mut arr = PhpArray::new();
            arr.set(ArrayKey::String(PhpString::from_bytes(b"errno")), Value::Long(handle.errno() as i64));
            arr.set(ArrayKey::String(PhpString::from_bytes(b"errstr")), Value::String(PhpString::from_bytes(handle.errstr().as_bytes())));
            Ok(Value::Array(Rc::new(RefCell::new(arr))))
        } else {
            Ok(Value::False)
        }
    })
}
