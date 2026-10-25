//! ext/hash: a port of php-src's ext/hash (hash.c and the hash_*.c
//! algorithm implementations), plus md5()/sha1()/crc32() and their _file()
//! variants from ext/standard.

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::{ClassEntry, NativeData, PhpObject};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{NativeClassHandler, NativeObjectHandlers, Vm, VmError};
use std::cell::RefCell;
use std::rc::Rc;

mod algos;
mod mhash;
mod ops;
mod tables;

use ops::{Ctx, InitArgs, Ops, fetch_ops};

const PHP_HASH_HMAC: i64 = 1;

/// The raw digest of `data` with the algorithm `algo` (hash('algo', data,
/// true)), for other extensions; None for an unknown algorithm.
pub fn digest(algo: &str, data: &[u8]) -> Option<Vec<u8>> {
    let ops = fetch_ops(algo.as_bytes())?;
    let mut ctx = ops.new_ctx(None);
    ctx.update(data);
    let mut out = vec![0u8; ops.digest_size];
    ctx.finish(&mut out);
    Some(out)
}

/// Register all hash extension functions
pub fn register(vm: &mut Vm) {
    vm.register_extension(b"hash");
    vm.register_function(b"crc32", crc32_fn);
    vm.register_function(b"md5", md5_fn);
    vm.register_function(b"sha1", sha1_fn);
    vm.register_function(b"md5_file", md5_file_fn);
    vm.register_function(b"sha1_file", sha1_file_fn);
    vm.register_function(b"hash", hash_fn);
    vm.register_function(b"hash_file", hash_file_fn);
    vm.register_function(b"hash_hmac", hash_hmac_fn);
    vm.register_function(b"hash_hmac_file", hash_hmac_file_fn);
    vm.register_function(b"hash_init", hash_init_fn);
    vm.register_function(b"hash_update", hash_update_fn);
    vm.register_function(b"hash_update_stream", hash_update_stream_fn);
    vm.register_function(b"hash_update_file", hash_update_file_fn);
    vm.register_function(b"hash_final", hash_final_fn);
    vm.register_function(b"hash_copy", hash_copy_fn);
    vm.register_function(b"hash_algos", hash_algos_fn);
    vm.register_function(b"hash_hmac_algos", hash_hmac_algos_fn);
    vm.register_function(b"hash_pbkdf2", hash_pbkdf2_fn);
    vm.register_function(b"hash_equals", hash_equals_fn);
    vm.register_function(b"hash_hkdf", hash_hkdf_fn);
    mhash::register(vm);

    // Register parameter types for strict_types enforcement + null-deprecation
    use goro_core::opcode::ParamType;
    let s = || Some(ParamType::Simple(b"string".to_vec()));
    let b = || Some(ParamType::Simple(b"bool".to_vec()));
    let i = || Some(ParamType::Simple(b"int".to_vec()));
    vm.register_builtin_param_types(b"md5", vec![s(), b()]);
    vm.register_builtin_param_types(b"sha1", vec![s(), b()]);
    vm.register_builtin_param_types(b"md5_file", vec![s(), b()]);
    vm.register_builtin_param_types(b"sha1_file", vec![s(), b()]);
    vm.register_builtin_param_types(b"crc32", vec![s()]);
    vm.register_builtin_param_types(
        b"hash",
        vec![s(), s(), b(), Some(ParamType::Simple(b"array".to_vec()))],
    );
    vm.register_builtin_param_types(
        b"hash_file",
        vec![s(), s(), b(), Some(ParamType::Simple(b"array".to_vec()))],
    );
    vm.register_builtin_param_types(b"hash_hmac", vec![s(), s(), s(), b()]);
    vm.register_builtin_param_types(b"hash_pbkdf2", vec![s(), s(), s(), i(), i(), b()]);
    vm.register_builtin_param_types(b"hash_update", vec![None, s()]);
    vm.register_builtin_param_types(b"hash_update_stream", vec![None, None, i()]);
    vm.register_builtin_param_types(b"hash_update_file", vec![None, s()]);

    // Register parameter names (for deprecation messages that mention $filename, etc.)
    let names = |l: &[&str]| l.iter().map(|n| n.as_bytes().to_vec()).collect::<Vec<_>>();
    vm.builtin_param_names
        .insert(b"md5_file".to_vec(), names(&["filename", "binary"]));
    vm.builtin_param_names
        .insert(b"sha1_file".to_vec(), names(&["filename", "binary"]));
    vm.builtin_param_names
        .insert(b"md5".to_vec(), names(&["string", "binary"]));
    vm.builtin_param_names
        .insert(b"sha1".to_vec(), names(&["string", "binary"]));
    vm.builtin_param_names.insert(
        b"hash".to_vec(),
        names(&["algo", "data", "binary", "options"]),
    );
    vm.builtin_param_names.insert(
        b"hash_file".to_vec(),
        names(&["algo", "filename", "binary", "options"]),
    );
    vm.builtin_param_names.insert(
        b"hash_hmac".to_vec(),
        names(&["algo", "data", "key", "binary"]),
    );
    vm.builtin_param_names
        .insert(b"hash_update".to_vec(), names(&["context", "data"]));
    vm.builtin_param_names.insert(
        b"hash_update_stream".to_vec(),
        names(&["context", "stream", "length"]),
    );
    vm.builtin_param_names.insert(
        b"hash_update_file".to_vec(),
        names(&["context", "filename", "stream_context"]),
    );
    vm.builtin_param_names.insert(
        b"hash_equals".to_vec(),
        names(&["known_string", "user_string"]),
    );

    register_hash_context(vm);
}

// ===========================================================================
// Helpers

fn throw<T>(vm: &mut Vm, class: &[u8], msg: &str) -> Result<T, VmError> {
    let exc = vm.create_exception(class, msg, vm.current_line);
    vm.current_exception = Some(exc);
    Err(VmError {
        message: msg.to_string(),
        line: vm.current_line,
    })
}

/// Propagate an exception thrown while running an algorithm init function.
fn pending_exception(vm: &mut Vm) -> Result<(), VmError> {
    if vm.current_exception.is_some() {
        return Err(VmError {
            message: "exception".into(),
            line: vm.current_line,
        });
    }
    Ok(())
}

fn hex(digest: &[u8]) -> Value {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = Vec::with_capacity(digest.len() * 2);
    for b in digest {
        out.push(HEX[(b >> 4) as usize]);
        out.push(HEX[(b & 0xf) as usize]);
    }
    Value::String(PhpString::from_vec(out))
}

fn digest_value(digest: Vec<u8>, raw: bool) -> Value {
    if raw {
        Value::String(PhpString::from_vec(digest))
    } else {
        hex(&digest)
    }
}

fn arg(args: &[Value], i: usize) -> Value {
    args.get(i).map(|v| v.deref()).unwrap_or(Value::Null)
}

fn arg_str(args: &[Value], i: usize) -> PhpString {
    arg(args, i).to_php_string()
}

fn arg_bool(args: &[Value], i: usize) -> bool {
    args.get(i).map(|v| v.is_truthy()).unwrap_or(false)
}

/// The `HashTable *args` (options) argument: None when omitted / null.
fn arg_options(args: &[Value], i: usize) -> Option<Rc<RefCell<PhpArray>>> {
    match arg(args, i) {
        Value::Array(a) => Some(a),
        _ => None,
    }
}

/// Create a context, running the algorithm's init with the options array.
fn new_ctx(
    vm: &mut Vm,
    ops: &'static Ops,
    options: Option<&Rc<RefCell<PhpArray>>>,
    fname: &str,
) -> Box<dyn Ctx> {
    match options {
        Some(a) => {
            let arr = a.borrow().clone();
            let mut ia = InitArgs {
                vm,
                args: &arr,
                fname,
            };
            ops.new_ctx(Some(&mut ia))
        }
        None => ops.new_ctx(None),
    }
}

fn finish(ops: &Ops, mut ctx: Box<dyn Ctx>) -> Vec<u8> {
    let mut out = vec![0u8; ops.digest_size];
    ctx.finish(&mut out);
    out
}

/// zend_zval_value_name()
fn zval_value_name(v: &Value) -> String {
    match v {
        Value::Null | Value::Undef => "null".into(),
        Value::True => "true".into(),
        Value::False => "false".into(),
        Value::Long(_) => "int".into(),
        Value::Double(_) => "float".into(),
        Value::String(_) => "string".into(),
        Value::Array(_) => "array".into(),
        Value::Object(o) => {
            goro_core::value::display_class_name(&o.borrow().class_name).to_string()
        }
        Value::Generator(_) => "Generator".into(),
        Value::Resource(_) => "resource".into(),
        Value::Reference(r) => zval_value_name(&r.borrow()),
    }
}

// ---------------------------------------------------------------- streams

/// Call a builtin (stream) function of ext/standard.
fn call_builtin(vm: &mut Vm, name: &[u8], args: &[Value]) -> Option<Result<Value, VmError>> {
    let f = vm.functions.get(name).copied()?;
    Some(f(vm, args))
}

/// php_stream_open_wrapper_ex(path, "rb", REPORT_ERRORS, NULL, context):
/// open through fopen(), reporting its warnings under `fname`.
fn open_stream(
    vm: &mut Vm,
    fname: &str,
    path: &PhpString,
    context: &Value,
) -> Result<Option<Value>, VmError> {
    let mut fargs = vec![
        Value::String(path.clone()),
        Value::String(PhpString::from_bytes(b"rb")),
        Value::False,
    ];
    if !matches!(context, Value::Null | Value::Undef) {
        fargs.push(context.clone());
    }
    let prev = vm.warning_capture.replace(Vec::new());
    let r = call_builtin(vm, b"fopen", &fargs);
    let captured = std::mem::replace(&mut vm.warning_capture, prev).unwrap_or_default();
    for w in captured {
        // "fopen(x): msg" -> "fname(x): msg"
        let msg = match w.find('(') {
            Some(i)
                if w[..i]
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_') =>
            {
                format!("{}{}", fname, &w[i..])
            }
            _ => w,
        };
        vm.emit_warning(&msg);
    }
    match r {
        Some(Ok(v @ Value::Resource(_))) => Ok(Some(v)),
        Some(Err(e)) => Err(e),
        _ => {
            if vm.current_exception.is_some() {
                return Err(VmError {
                    message: "exception".into(),
                    line: vm.current_line,
                });
            }
            Ok(None)
        }
    }
}

/// php_stream_read() of up to `n` bytes: None at EOF, Err(None) when the
/// read failed.
fn stream_read(
    vm: &mut Vm,
    stream: &Value,
    n: usize,
) -> Result<Option<Vec<u8>>, Option<VmError>> {
    match call_builtin(vm, b"fread", &[stream.clone(), Value::Long(n as i64)]) {
        Some(Ok(Value::String(s))) if !s.as_bytes().is_empty() => Ok(Some(s.as_bytes().to_vec())),
        Some(Ok(Value::False)) => Err(None),
        Some(Err(e)) => Err(Some(e)),
        _ => Ok(None),
    }
}

fn stream_close(vm: &mut Vm, stream: &Value) {
    let _ = call_builtin(vm, b"fclose", std::slice::from_ref(stream));
}

/// Feed a whole stream to `ctx` in 1024-byte reads, then close it.
/// false: a read failed (the callers return false).
fn pump_stream(vm: &mut Vm, stream: &Value, ctx: &mut dyn Ctx) -> Result<bool, VmError> {
    let mut ok = true;
    loop {
        match stream_read(vm, stream, 1024) {
            Ok(Some(buf)) => ctx.update(&buf),
            Ok(None) => break,
            Err(None) => {
                ok = false;
                break;
            }
            Err(Some(e)) => {
                stream_close(vm, stream);
                return Err(e);
            }
        }
    }
    stream_close(vm, stream);
    Ok(ok)
}

// ===========================================================================
// md5(), sha1(), crc32() and friends (ext/standard)

fn crc32_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = arg_str(args, 0);
    let crc = !algos::simple::crc32b_update(!0, data.as_bytes());
    Ok(Value::Long(crc as i64))
}

fn md5_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = arg_str(args, 0);
    let digest = algos::md::md5(data.as_bytes());
    Ok(digest_value(digest.to_vec(), arg_bool(args, 1)))
}

fn sha1_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = arg_str(args, 0);
    let digest = algos::sha::sha1(data.as_bytes());
    Ok(digest_value(digest.to_vec(), arg_bool(args, 1)))
}

fn file_digest(
    vm: &mut Vm,
    args: &[Value],
    fname: &str,
    ops: &'static Ops,
) -> Result<Value, VmError> {
    let filename = arg_str(args, 0);
    if filename.as_bytes().is_empty() {
        return throw(vm, b"ValueError", "Path must not be empty");
    }
    if filename.as_bytes().contains(&0) {
        return throw(
            vm,
            b"ValueError",
            &format!(
                "{}(): Argument #1 ($filename) must not contain any null bytes",
                fname
            ),
        );
    }
    let Some(stream) = open_stream(vm, fname, &filename, &Value::Null)? else {
        return Ok(Value::False);
    };
    let mut ctx = ops.new_ctx(None);
    // (md5_file() / sha1_file() hash what was read even when a read failed)
    pump_stream(vm, &stream, ctx.as_mut())?;
    Ok(digest_value(finish(ops, ctx), arg_bool(args, 1)))
}

fn md5_file_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    file_digest(vm, args, "md5_file", &ops::MD5)
}

fn sha1_file_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    file_digest(vm, args, "sha1_file", &ops::SHA1)
}

// ===========================================================================
// hash(), hash_file()

fn do_hash(vm: &mut Vm, args: &[Value], fname: &str, is_file: bool) -> Result<Value, VmError> {
    let algo = arg_str(args, 0);
    let data = arg_str(args, 1);
    let raw = arg_bool(args, 2);
    let options = arg_options(args, 3);
    let Some(ops) = fetch_ops(algo.as_bytes()) else {
        return throw(
            vm,
            b"ValueError",
            &format!(
                "{}(): Argument #1 ($algo) must be a valid hashing algorithm",
                fname
            ),
        );
    };
    let mut stream = None;
    if is_file {
        if data.as_bytes().is_empty() {
            return throw(vm, b"ValueError", "Path must not be empty");
        }
        if data.as_bytes().contains(&0) {
            return throw(
                vm,
                b"ValueError",
                &format!(
                    "{}(): Argument #1 ($algo) must not contain any null bytes",
                    fname
                ),
            );
        }
        match open_stream(vm, fname, &data, &Value::Null)? {
            Some(s) => stream = Some(s),
            None => return Ok(Value::False),
        }
    }
    let mut ctx = new_ctx(vm, ops, options.as_ref(), fname);
    if let Some(s) = stream {
        if !pump_stream(vm, &s, ctx.as_mut())? {
            return Ok(Value::False);
        }
    } else {
        ctx.update(data.as_bytes());
    }
    pending_exception(vm)?;
    Ok(digest_value(finish(ops, ctx), raw))
}

fn hash_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    do_hash(vm, args, "hash", false)
}

fn hash_file_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    do_hash(vm, args, "hash_file", true)
}

// ===========================================================================
// HMAC

/// php_hash_hmac_prep_key(): the key padded to the block size, XOR 0x36.
fn hmac_prep_key(ops: &'static Ops, key: &[u8]) -> Vec<u8> {
    let mut k = vec![0u8; ops.block_size];
    if key.len() > ops.block_size {
        let d = ops.digest(key);
        k[..d.len()].copy_from_slice(&d);
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    for b in k.iter_mut() {
        *b ^= 0x36;
    }
    k
}

/// php_hash_hmac_round()
fn hmac_round(ops: &'static Ops, key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut c = ops.new_ctx(None);
    c.update(&key[..ops.block_size]);
    c.update(data);
    finish(ops, c)
}

fn xor_6a(k: &mut [u8]) {
    for b in k.iter_mut() {
        *b ^= 0x6A;
    }
}

fn do_hash_hmac(vm: &mut Vm, args: &[Value], fname: &str, is_file: bool) -> Result<Value, VmError> {
    let algo = arg_str(args, 0);
    let data = arg_str(args, 1);
    let key = arg_str(args, 2);
    let raw = arg_bool(args, 3);
    let ops = match fetch_ops(algo.as_bytes()) {
        Some(o) if o.is_crypto => o,
        _ => {
            return throw(
                vm,
                b"ValueError",
                &format!(
                    "{}(): Argument #1 ($algo) must be a valid cryptographic hashing algorithm",
                    fname
                ),
            );
        }
    };
    let mut stream = None;
    if is_file {
        if data.as_bytes().is_empty() {
            return throw(vm, b"ValueError", "Path must not be empty");
        }
        if data.as_bytes().contains(&0) {
            return throw(
                vm,
                b"ValueError",
                &format!(
                    "{}(): Argument #2 ($filename) must not contain any null bytes",
                    fname
                ),
            );
        }
        match open_stream(vm, fname, &data, &Value::Null)? {
            Some(s) => stream = Some(s),
            None => return Ok(Value::False),
        }
    }
    let mut k = hmac_prep_key(ops, key.as_bytes());
    let digest = if let Some(s) = stream {
        let mut c = ops.new_ctx(None);
        c.update(&k);
        if !pump_stream(vm, &s, c.as_mut())? {
            return Ok(Value::False);
        }
        finish(ops, c)
    } else {
        hmac_round(ops, &k, data.as_bytes())
    };
    xor_6a(&mut k);
    let digest = hmac_round(ops, &k, &digest);
    Ok(digest_value(digest, raw))
}

fn hash_hmac_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    do_hash_hmac(vm, args, "hash_hmac", false)
}

fn hash_hmac_file_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    do_hash_hmac(vm, args, "hash_hmac_file", true)
}

// ===========================================================================
// HashContext

/// php_hashcontext_object (minus the zend_object).
#[derive(Clone)]
struct HashObj {
    ops: Option<&'static Ops>,
    /// None once finalized (or before initialization).
    ctx: Option<Box<dyn Ctx>>,
    options: i64,
    /// The HMAC key (block_size bytes, XOR ipad).
    key: Option<Vec<u8>>,
}

type Shared = RefCell<HashObj>;

static HASH_CONTEXT_HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    clone_obj: Some(hash_context_clone),
    get_properties: Some(hc_get_properties),
    ..NativeObjectHandlers::EMPTY
};

/// var_dump()/print_r() go through HashContext::__debugInfo().
fn hc_get_properties(_vm: &mut Vm, obj: &Rc<RefCell<PhpObject>>, purpose: u8) -> Option<PhpArray> {
    if purpose != 0 {
        return None;
    }
    Some(debug_info(obj))
}

fn debug_info(obj: &Rc<RefCell<PhpObject>>) -> PhpArray {
    let mut arr = PhpArray::new();
    if let Some(st) = state_of(obj) {
        if let Some(ops) = st.borrow().ops {
            arr.set(
                ArrayKey::String(PhpString::from_bytes(b"algo")),
                Value::String(PhpString::from_bytes(ops.algo.as_bytes())),
            );
        }
    }
    arr
}

fn attach(obj: &Rc<RefCell<PhpObject>>, state: HashObj) {
    obj.borrow_mut().native = Some(NativeData {
        data: Rc::new(RefCell::new(state)),
        handlers: &HASH_CONTEXT_HANDLERS,
    });
}

fn state_of(obj: &Rc<RefCell<PhpObject>>) -> Option<Rc<Shared>> {
    let data = obj.borrow().native.as_ref()?.data.clone();
    data.downcast::<Shared>().ok()
}

fn is_hash_context(obj: &Rc<RefCell<PhpObject>>) -> bool {
    obj.borrow().class_name.eq_ignore_ascii_case(b"HashContext")
}

fn register_hash_context(vm: &mut Vm) {
    let mut ce = ClassEntry::new(b"HashContext".to_vec());
    ce.is_final = true;
    // The private constructor (from the stub): `new HashContext` fails with
    // the usual visibility error.
    if let Some(stub) = goro_core::stubs::class_entry(vm, b"HashContext") {
        if let Some(m) = stub.methods.get(b"__construct".as_slice()) {
            ce.methods.insert(b"__construct".to_vec(), m.clone());
        }
    }
    vm.classes.insert(b"hashcontext".to_vec(), ce);
    vm.native_class_handlers.insert(
        b"hashcontext".to_vec(),
        NativeClassHandler {
            has_method: hc_has_method,
            call: hc_call_method,
            prop_added: None,
            prop_write: None,
            compare_check: None,
            prop_read: None,
            param_names: Some(hc_param_names),
        },
    );
    vm.object_create_handlers
        .insert(b"hashcontext".to_vec(), hc_create_object);
}

fn hc_create_object(_vm: &mut Vm, o: &Rc<RefCell<PhpObject>>) {
    if o.borrow().native.is_none() {
        attach(
            o,
            HashObj {
                ops: None,
                ctx: None,
                options: 0,
                key: None,
            },
        );
    }
}

fn hc_canon(method: &[u8]) -> Option<&'static str> {
    let l = method.to_ascii_lowercase();
    Some(match l.as_slice() {
        b"__serialize" => "__serialize",
        b"__unserialize" => "__unserialize",
        b"__debuginfo" => "__debugInfo",
        _ => return None,
    })
}

fn hc_has_method(_class: &[u8], method: &[u8], _is_static: bool) -> bool {
    hc_canon(method).is_some()
}

fn hc_param_names(_class: &[u8], method: &[u8]) -> Option<&'static [&'static str]> {
    match hc_canon(method)? {
        "__unserialize" => Some(&["data"]),
        _ => Some(&[]),
    }
}

fn throw_value(vm: &mut Vm, class: &[u8], msg: &str) -> Value {
    let exc = vm.create_exception(class, msg, vm.current_line);
    vm.current_exception = Some(exc);
    Value::Null
}

fn hc_call_method(
    vm: &mut Vm,
    _class: &[u8],
    method: &[u8],
    _called: &[u8],
    this: Option<&Value>,
    args: &[Value],
) -> Value {
    let Some(m) = hc_canon(method) else {
        return Value::Null;
    };
    let obj = match this.map(|t| t.deref()) {
        Some(Value::Object(o)) => o,
        _ => {
            return throw_value(
                vm,
                b"Error",
                &format!(
                    "Non-static method HashContext::{}() cannot be called statically",
                    m
                ),
            );
        }
    };
    match m {
        "__serialize" => {
            if !args.is_empty() {
                return throw_value(
                    vm,
                    b"ArgumentCountError",
                    &format!(
                        "HashContext::__serialize() expects exactly 0 arguments, {} given",
                        args.len()
                    ),
                );
            }
            hc_serialize(vm, &obj)
        }
        "__unserialize" => {
            let data = match args.first().map(|a| a.deref()) {
                Some(Value::Array(a)) => a,
                Some(other) => {
                    let msg = format!(
                        "HashContext::__unserialize(): Argument #1 ($data) must be of type array, {} given",
                        zval_value_name(&other)
                    );
                    return throw_value(vm, b"TypeError", &msg);
                }
                None => {
                    return throw_value(
                        vm,
                        b"ArgumentCountError",
                        "HashContext::__unserialize() expects exactly 1 argument, 0 given",
                    );
                }
            };
            hc_unserialize(vm, &obj, &data)
        }
        _ => Value::Array(Rc::new(RefCell::new(debug_info(&obj)))),
    }
}

/// HashContext::__serialize()
fn hc_serialize(vm: &mut Vm, obj: &Rc<RefCell<PhpObject>>) -> Value {
    let Some(st) = state_of(obj) else {
        return throw_value(
            vm,
            b"Exception",
            "HashContext for algorithm \"\" cannot be serialized",
        );
    };
    let st = st.borrow();
    let Some(ops) = st.ops else {
        return throw_value(
            vm,
            b"Exception",
            "HashContext for algorithm \"\" cannot be serialized",
        );
    };
    if st.options & PHP_HASH_HMAC != 0 {
        return throw_value(
            vm,
            b"Exception",
            "HashContext with HASH_HMAC option cannot be serialized",
        );
    }
    let state = match (ops.spec, st.ctx.as_ref()) {
        (Some(spec), Some(ctx)) => ops::serialize_spec(&ctx.image(), ops.context_size, spec),
        _ => None,
    };
    let Some(state) = state else {
        return throw_value(
            vm,
            b"Exception",
            &format!(
                "HashContext for algorithm \"{}\" cannot be serialized",
                ops.algo
            ),
        );
    };
    let mut arr = PhpArray::new();
    arr.push(Value::String(PhpString::from_bytes(ops.algo.as_bytes())));
    arr.push(Value::Long(st.options));
    arr.push(Value::Array(Rc::new(RefCell::new(state))));
    arr.push(Value::Long(ops.magic));
    let mut members = PhpArray::new();
    for (k, v) in obj.borrow().properties.iter() {
        members.set(ArrayKey::String(PhpString::from_vec(k.clone())), v.clone());
    }
    arr.push(Value::Array(Rc::new(RefCell::new(members))));
    Value::Array(Rc::new(RefCell::new(arr)))
}

/// HashContext::__unserialize()
fn hc_unserialize(
    vm: &mut Vm,
    obj: &Rc<RefCell<PhpObject>>,
    data: &Rc<RefCell<PhpArray>>,
) -> Value {
    if let Some(st) = state_of(obj) {
        if st.borrow().ctx.is_some() {
            return throw_value(
                vm,
                b"Exception",
                "HashContext::__unserialize called on initialized object",
            );
        }
    }
    let (algo, options, hash_zv, magic, members) = {
        let d = data.borrow();
        let get = |i: i64| d.get(&ArrayKey::Int(i)).map(|v| v.deref());
        let algo = get(0);
        let options = get(1);
        let hash_zv = get(2);
        let magic = get(3);
        let members = get(4);
        match (algo, options, hash_zv, magic, members) {
            (
                Some(Value::String(a)),
                Some(Value::Long(o)),
                Some(h),
                Some(Value::Long(m)),
                Some(Value::Array(mem)),
            ) => (a, o, h, m, mem),
            _ => {
                return throw_value(
                    vm,
                    b"Exception",
                    "Incomplete or ill-formed serialization data",
                );
            }
        }
    };
    if options & PHP_HASH_HMAC != 0 {
        return throw_value(
            vm,
            b"Exception",
            "HashContext with HASH_HMAC option cannot be serialized",
        );
    }
    let Some(ops) = fetch_ops(algo.as_bytes()) else {
        return throw_value(vm, b"Exception", "Unknown hash algorithm");
    };
    let mut ctx = ops.new_ctx(None);
    let mut result = ops::HASH_SPEC_FAILURE;
    if let Some(spec) = ops.spec {
        if magic == ops.magic {
            let mut img = ctx.image();
            img.resize(ops.context_size, 0);
            result = ops::unserialize_spec(&mut img, ops.context_size, &hash_zv, spec);
            if result == 0 {
                ctx.set_image(&img);
                if !ctx.valid() {
                    result = ops::CONTEXT_VALIDATION_FAILURE;
                }
            }
        }
    }
    if result != 0 {
        attach(
            obj,
            HashObj {
                ops: Some(ops),
                ctx: None,
                options,
                key: None,
            },
        );
        return throw_value(
            vm,
            b"Exception",
            &format!(
                "Incomplete or ill-formed serialization data (\"{}\" code {})",
                ops.algo, result
            ),
        );
    }
    attach(
        obj,
        HashObj {
            ops: Some(ops),
            ctx: Some(ctx),
            options,
            key: None,
        },
    );
    // object_properties_load()
    let members: Vec<(ArrayKey, Value)> = members
        .borrow()
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    for (k, v) in members {
        let name = match k {
            ArrayKey::String(s) => s.as_bytes().to_vec(),
            ArrayKey::Int(i) => i.to_string().into_bytes(),
        };
        if !obj.borrow().has_property(&name) {
            vm.emit_deprecated(&format!(
                "Creation of dynamic property HashContext::${} is deprecated",
                goro_core::string::msg_str(&name)
            ));
        }
        obj.borrow_mut().set_property(name, v);
    }
    Value::Null
}

/// php_hashcontext_clone()
fn hash_context_clone(vm: &mut Vm, src: &Rc<RefCell<PhpObject>>, dst: &Rc<RefCell<PhpObject>>) {
    let old = state_of(src).map(|s| s.borrow().clone());
    match old {
        Some(HashObj {
            ops: Some(ops),
            ctx: Some(ctx),
            options,
            key,
        }) => attach(
            dst,
            HashObj {
                ops: Some(ops),
                ctx: Some(ctx.box_clone()),
                options,
                key,
            },
        ),
        old => {
            attach(
                dst,
                HashObj {
                    ops: old.and_then(|o| o.ops),
                    ctx: None,
                    options: 0,
                    key: None,
                },
            );
            let exc = vm.create_exception(
                b"ValueError",
                "Cannot clone a finalized HashContext",
                vm.current_line,
            );
            vm.current_exception = Some(exc);
        }
    }
}

/// Fetch the HashContext argument #1 of `fname` (Z_PARAM_OBJ_OF_CLASS) and
/// check it is not finalized (PHP_HASHCONTEXT_VERIFY).
fn context_arg(
    vm: &mut Vm,
    args: &[Value],
    fname: &str,
) -> Result<(Rc<RefCell<PhpObject>>, Rc<Shared>), VmError> {
    let v = arg(args, 0);
    let obj = match &v {
        Value::Object(o) if is_hash_context(o) => o.clone(),
        _ => {
            let msg = format!(
                "{}(): Argument #1 ($context) must be of type HashContext, {} given",
                fname,
                zval_value_name(&v)
            );
            return throw(vm, b"TypeError", &msg);
        }
    };
    let st = state_of(&obj).filter(|s| s.borrow().ctx.is_some());
    match st {
        Some(s) => Ok((obj, s)),
        None => {
            let msg = format!(
                "{}(): Argument #1 ($context) must be a valid, non-finalized HashContext",
                fname
            );
            throw(vm, b"TypeError", &msg)
        }
    }
}

fn hash_init_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let algo = arg_str(args, 0);
    let options = arg(args, 1).to_long();
    let key = match arg(args, 2) {
        Value::Null | Value::Undef => None,
        v => Some(v.to_php_string()),
    };
    let opts = arg_options(args, 3);
    let Some(ops) = fetch_ops(algo.as_bytes()) else {
        return throw(
            vm,
            b"ValueError",
            "hash_init(): Argument #1 ($algo) must be a valid hashing algorithm",
        );
    };
    if options & PHP_HASH_HMAC != 0 {
        if !ops.is_crypto {
            return throw(
                vm,
                b"ValueError",
                "hash_init(): Argument #1 ($algo) must be a cryptographic hashing algorithm if HMAC is requested",
            );
        }
        if key.as_ref().is_none_or(|k| k.as_bytes().is_empty()) {
            return throw(
                vm,
                b"ValueError",
                "hash_init(): Argument #3 ($key) must not be empty when HMAC is requested",
            );
        }
    }
    let id = vm.next_object_id();
    let obj = Rc::new(RefCell::new(PhpObject::new(b"HashContext".to_vec(), id)));
    let mut ctx = new_ctx(vm, ops, opts.as_ref(), "hash_init");
    let mut hmac_key = None;
    if options & PHP_HASH_HMAC != 0 {
        let key = key.unwrap_or_else(|| PhpString::from_bytes(b""));
        let key = key.as_bytes();
        let mut k = vec![0u8; ops.block_size];
        if key.len() > ops.block_size {
            ctx.update(key);
            let d = finish(ops, ctx);
            k[..d.len()].copy_from_slice(&d);
            ctx = new_ctx(vm, ops, opts.as_ref(), "hash_init");
        } else {
            k[..key.len()].copy_from_slice(key);
        }
        for b in k.iter_mut() {
            *b ^= 0x36;
        }
        ctx.update(&k);
        hmac_key = Some(k);
    }
    attach(
        &obj,
        HashObj {
            ops: Some(ops),
            ctx: Some(ctx),
            options,
            key: hmac_key,
        },
    );
    pending_exception(vm)?;
    Ok(Value::Object(obj))
}

/// ArgumentCountError of zend_parse_parameters().
fn check_arity(
    vm: &mut Vm,
    args: &[Value],
    fname: &str,
    min: usize,
    max: usize,
) -> Result<(), VmError> {
    let n = args.len();
    if n >= min && n <= max {
        return Ok(());
    }
    let (kind, count) = if min == max {
        ("exactly", min)
    } else if n < min {
        ("at least", min)
    } else {
        ("at most", max)
    };
    let msg = format!(
        "{}() expects {} {} argument{}, {} given",
        fname,
        kind,
        count,
        if count == 1 { "" } else { "s" },
        n
    );
    throw(vm, b"ArgumentCountError", &msg)
}

fn has_tostring(vm: &Vm, obj: &Rc<RefCell<PhpObject>>) -> bool {
    let mut cur = obj.borrow().class_name.to_ascii_lowercase();
    if let Some((c, h)) = vm.find_native_class_handler(&cur) {
        if (h.has_method)(&c, b"__tostring", false) {
            return true;
        }
    }
    for _ in 0..64 {
        let Some(ce) = vm.classes.get(&cur) else {
            return false;
        };
        if ce.get_method(b"__tostring").is_some() {
            return true;
        }
        match &ce.parent {
            Some(p) => cur = p.to_ascii_lowercase(),
            None => return false,
        }
    }
    false
}

/// A string parameter ('S'/'P'); null coercion and strict mode are handled
/// by the builtin parameter types registered for the function.
fn zpp_string(
    vm: &mut Vm,
    args: &[Value],
    i: usize,
    fname: &str,
    pname: &str,
) -> Result<PhpString, VmError> {
    let v = arg(args, i);
    match &v {
        Value::Array(_) => {}
        Value::Object(o) if has_tostring(vm, o) => return vm.value_to_string_checked(&v),
        Value::Object(_) => {}
        _ => return Ok(v.to_php_string()),
    }
    let msg = format!(
        "{}(): Argument #{} (${}) must be of type string, {} given",
        fname,
        i + 1,
        pname,
        zval_value_name(&v)
    );
    throw(vm, b"TypeError", &msg)
}

/// An int parameter ('l').
fn zpp_long(
    vm: &mut Vm,
    args: &[Value],
    i: usize,
    fname: &str,
    pname: &str,
) -> Result<i64, VmError> {
    let v = arg(args, i);
    let ok = match &v {
        Value::Long(n) => return Ok(*n),
        Value::Null | Value::Undef | Value::True | Value::False => true,
        Value::Double(d) => d.is_finite(),
        Value::String(s) => goro_core::value::parse_numeric_str(s.as_bytes(), false)
            .is_some_and(|(_, trailing)| !trailing),
        _ => false,
    };
    if ok {
        return Ok(v.to_long());
    }
    let msg = format!(
        "{}(): Argument #{} (${}) must be of type int, {} given",
        fname,
        i + 1,
        pname,
        zval_value_name(&v)
    );
    throw(vm, b"TypeError", &msg)
}

fn hash_update_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    check_arity(vm, args, "hash_update", 2, 2)?;
    let (_, st) = context_arg(vm, args, "hash_update")?;
    let data = zpp_string(vm, args, 1, "hash_update", "data")?;
    if let Some(c) = st.borrow_mut().ctx.as_mut() {
        c.update(data.as_bytes());
    }
    Ok(Value::True)
}

fn hash_update_stream_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    check_arity(vm, args, "hash_update_stream", 2, 3)?;
    let (_, st) = context_arg(vm, args, "hash_update_stream")?;
    let stream = arg(args, 1);
    if !matches!(stream, Value::Resource(_)) {
        let msg = format!(
            "hash_update_stream(): Argument #2 ($stream) must be of type resource, {} given",
            zval_value_name(&stream)
        );
        return throw(vm, b"TypeError", &msg);
    }
    let mut length = if args.len() > 2 {
        zpp_long(vm, args, 2, "hash_update_stream", "length")?
    } else {
        -1
    };
    let mut didread: i64 = 0;
    while length != 0 {
        let mut toread: i64 = 1024;
        if length > 0 && toread > length {
            toread = length;
        }
        let Some(buf) = stream_read(vm, &stream, toread as usize).or_else(|e| e.map_or(Ok(None), Err))? else {
            break;
        };
        if let Some(c) = st.borrow_mut().ctx.as_mut() {
            c.update(&buf);
        }
        length -= buf.len() as i64;
        didread += buf.len() as i64;
    }
    Ok(Value::Long(didread))
}

fn hash_update_file_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    check_arity(vm, args, "hash_update_file", 2, 3)?;
    let (_, st) = context_arg(vm, args, "hash_update_file")?;
    let filename = zpp_string(vm, args, 1, "hash_update_file", "filename")?;
    if filename.as_bytes().contains(&0) {
        return throw(
            vm,
            b"ValueError",
            "hash_update_file(): Argument #2 ($filename) must not contain any null bytes",
        );
    }
    let context = arg(args, 2);
    if !matches!(context, Value::Null | Value::Undef | Value::Resource(_)) {
        let msg = format!(
            "hash_update_file(): Argument #3 ($stream_context) must be of type resource or null, {} given",
            zval_value_name(&context)
        );
        return throw(vm, b"TypeError", &msg);
    }
    let Some(stream) = open_stream(vm, "hash_update_file", &filename, &context)? else {
        return Ok(Value::False);
    };
    let mut ctx = st.borrow_mut().ctx.take();
    let r = match ctx.as_mut() {
        Some(c) => pump_stream(vm, &stream, c.as_mut()),
        None => Ok(true),
    };
    st.borrow_mut().ctx = ctx;
    Ok(if r? { Value::True } else { Value::False })
}

fn hash_final_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let (_, st) = context_arg(vm, args, "hash_final")?;
    let raw = arg_bool(args, 1);
    let mut s = st.borrow_mut();
    let ops = s.ops.expect("initialized context");
    let ctx = s.ctx.take().expect("non-finalized context");
    let mut digest = finish(ops, ctx);
    if s.options & PHP_HASH_HMAC != 0 {
        if let Some(mut k) = s.key.take() {
            xor_6a(&mut k);
            digest = hmac_round(ops, &k, &digest);
        }
    }
    Ok(digest_value(digest, raw))
}

fn hash_copy_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let (obj, st) = context_arg(vm, args, "hash_copy")?;
    let id = vm.next_object_id();
    let mut copy = PhpObject::new(obj.borrow().class_name.clone(), id);
    for (k, v) in obj.borrow().properties.iter() {
        copy.set_property(k.clone(), v.clone());
    }
    let copy = Rc::new(RefCell::new(copy));
    let s = st.borrow().clone();
    attach(&copy, s);
    Ok(Value::Object(copy))
}

fn hash_algos_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut result = PhpArray::new();
    for (name, _) in ops::REGISTRY {
        result.push(Value::String(PhpString::from_bytes(name.as_bytes())));
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn hash_hmac_algos_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut result = PhpArray::new();
    for (name, o) in ops::REGISTRY {
        if o.is_crypto {
            result.push(Value::String(PhpString::from_bytes(name.as_bytes())));
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

// ===========================================================================
// Key derivation

fn hash_hkdf_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let algo = arg_str(args, 0);
    let ikm = arg_str(args, 1);
    let length = args.get(2).map(|v| v.deref().to_long()).unwrap_or(0);
    let info = arg_str(args, 3);
    let salt = arg_str(args, 4);
    let ops = match fetch_ops(algo.as_bytes()) {
        Some(o) if o.is_crypto => o,
        _ => {
            return throw(
                vm,
                b"ValueError",
                "hash_hkdf(): Argument #1 ($algo) must be a valid cryptographic hashing algorithm",
            );
        }
    };
    if ikm.as_bytes().is_empty() {
        return throw(
            vm,
            b"ValueError",
            "hash_hkdf(): Argument #2 ($key) must not be empty",
        );
    }
    let length = if length < 0 {
        return throw(
            vm,
            b"ValueError",
            "hash_hkdf(): Argument #3 ($length) must be greater than or equal to 0",
        );
    } else if length == 0 {
        ops.digest_size
    } else if length as u64 > (ops.digest_size * 255) as u64 {
        return throw(
            vm,
            b"ValueError",
            &format!(
                "hash_hkdf(): Argument #3 ($length) must be less than or equal to {}",
                ops.digest_size * 255
            ),
        );
    } else {
        length as usize
    };

    // Extract
    let mut k = hmac_prep_key(ops, salt.as_bytes());
    let prk = hmac_round(ops, &k, ikm.as_bytes());
    xor_6a(&mut k);
    let prk = hmac_round(ops, &k, &prk);

    // Expand
    let mut out = Vec::with_capacity(length);
    let rounds = (length - 1) / ops.digest_size + 1;
    let mut digest: Vec<u8> = Vec::new();
    for i in 1..=rounds {
        let mut k = hmac_prep_key(ops, &prk);
        let mut c = ops.new_ctx(None);
        c.update(&k);
        if i > 1 {
            c.update(&digest);
        }
        if !info.as_bytes().is_empty() {
            c.update(info.as_bytes());
        }
        c.update(&[(i & 0xff) as u8]);
        digest = finish(ops, c);
        xor_6a(&mut k);
        digest = hmac_round(ops, &k, &digest);
        let n = if i == rounds {
            length - (i - 1) * ops.digest_size
        } else {
            ops.digest_size
        };
        out.extend_from_slice(&digest[..n]);
    }
    Ok(Value::String(PhpString::from_vec(out)))
}

fn hash_pbkdf2_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let algo = arg_str(args, 0);
    let pass = arg_str(args, 1);
    let salt = arg_str(args, 2);
    let iterations = arg(args, 3).to_long();
    let mut length = args.get(4).map(|v| v.deref().to_long()).unwrap_or(0);
    let raw = arg_bool(args, 5);
    let opts = arg_options(args, 6);
    let ops = match fetch_ops(algo.as_bytes()) {
        Some(o) if o.is_crypto => o,
        _ => {
            return throw(
                vm,
                b"ValueError",
                "hash_pbkdf2(): Argument #1 ($algo) must be a valid cryptographic hashing algorithm",
            );
        }
    };
    if salt.as_bytes().len() > i32::MAX as usize - 4 {
        return throw(
            vm,
            b"ValueError",
            "hash_pbkdf2(): Argument #3 ($salt) must be less than or equal to INT_MAX - 4 bytes",
        );
    }
    if iterations <= 0 {
        return throw(
            vm,
            b"ValueError",
            "hash_pbkdf2(): Argument #4 ($iterations) must be greater than 0",
        );
    }
    if length < 0 {
        return throw(
            vm,
            b"ValueError",
            "hash_pbkdf2(): Argument #5 ($length) must be greater than or equal to 0",
        );
    }
    // ops->hash_init(context, args): only for its side effects (deprecations)
    let _ = new_ctx(vm, ops, opts.as_ref(), "hash_pbkdf2");
    pending_exception(vm)?;

    let k1 = hmac_prep_key(ops, pass.as_bytes());
    let mut k2 = k1.clone();
    xor_6a(&mut k2);

    if length == 0 {
        length = ops.digest_size as i64;
        if !raw {
            length *= 2;
        }
    }
    let digest_length = if raw {
        length
    } else {
        ((length as f32) / 2.0).ceil() as i64
    };
    let loops = ((digest_length as f32) / (ops.digest_size as f32)).ceil() as i64;

    let mut result = Vec::with_capacity(loops as usize * ops.digest_size);
    let mut computed_salt = salt.as_bytes().to_vec();
    let salt_len = computed_salt.len();
    computed_salt.extend_from_slice(&[0; 4]);
    for i in 1..=loops {
        computed_salt[salt_len..].copy_from_slice(&(i as u32).to_be_bytes());
        let mut digest = hmac_round(ops, &k1, &computed_salt);
        digest = hmac_round(ops, &k2, &digest);
        let mut temp = digest.clone();
        for _ in 1..iterations {
            digest = hmac_round(ops, &k1, &digest);
            digest = hmac_round(ops, &k2, &digest);
            for (t, d) in temp.iter_mut().zip(digest.iter()) {
                *t ^= d;
            }
        }
        result.extend_from_slice(&temp);
    }
    let length = length as usize;
    let out = if raw {
        result.truncate(length);
        result
    } else {
        let h = hex(&result[..(digest_length as usize).min(result.len())]);
        let mut s = h.to_php_string().as_bytes().to_vec();
        s.truncate(length);
        s
    };
    Ok(Value::String(PhpString::from_vec(out)))
}

fn hash_equals_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let known = arg(args, 0);
    let user = arg(args, 1);
    let Value::String(k) = &known else {
        let msg = format!(
            "hash_equals(): Argument #1 ($known_string) must be of type string, {} given",
            zval_value_name(&known)
        );
        return throw(vm, b"TypeError", &msg);
    };
    let Value::String(u) = &user else {
        let msg = format!(
            "hash_equals(): Argument #2 ($user_string) must be of type string, {} given",
            zval_value_name(&user)
        );
        return throw(vm, b"TypeError", &msg);
    };
    // php_safe_bcmp(): constant time for equal lengths
    let (a, b) = (k.as_bytes(), u.as_bytes());
    if a.len() != b.len() {
        return Ok(Value::False);
    }
    let mut r = 0u8;
    for (x, y) in a.iter().zip(b) {
        r |= x ^ y;
    }
    Ok(if r == 0 { Value::True } else { Value::False })
}
