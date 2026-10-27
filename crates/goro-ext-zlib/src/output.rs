//! zlib.c's output compression: the "zlib output compression" and
//! "ob_gzhandler" output handlers, the legacy ob_gzhandler() function,
//! zlib_get_coding_type() and the zlib.output_* INI settings.

use crate::codec::{PHP_ZLIB_ENCODING_DEFLATE, PHP_ZLIB_ENCODING_GZIP, buffer_size_guess};
use goro_core::array::ArrayKey;
use goro_core::ob::*;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use goro_ext_standard::misc::sapi_add_header;
use goro_zlib::*;
use std::cell::RefCell;

const PHP_ZLIB_OUTPUT_HANDLER_NAME: &[u8] = b"zlib output compression";
const PHP_OUTPUT_HANDLER_DEFAULT_SIZE: i64 = 0x4000;

/// php_zlib_context (the parts the output handler uses)
#[derive(Default)]
struct ObCtx {
    z: Option<Box<Deflate>>,
    buffer: Vec<u8>,
}

/// zend_zlib_globals
#[derive(Default)]
struct Globals {
    output_compression: i64,
    output_compression_default: i64,
    handler_registered: bool,
    compression_coding: i64,
    /// HTTP_ACCEPT_ENCODING of the request's server variables
    /// (PG(http_globals)[TRACK_VARS_SERVER]: not the script's $_SERVER)
    accept_encoding: Option<Vec<u8>>,
    ini_loaded: bool,
    /// ZLIBG(ob_gzhandler): context of the legacy ob_gzhandler() function
    legacy: Option<ObCtx>,
    /// Context of the running internal handler.
    handler: ObCtx,
}

thread_local! {
    static ZLIBG: RefCell<Globals> = RefCell::new(Globals::default());
}

fn ini_str(vm: &Vm, key: &[u8]) -> String {
    vm.constants
        .get(key)
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_default()
}

/// OnUpdate_zlib_output_compression()'s value parsing.
fn parse_output_compression(v: &str) -> i64 {
    if v.eq_ignore_ascii_case("off") {
        0
    } else if v.eq_ignore_ascii_case("on") {
        1
    } else {
        let (n, _) = goro_core::ini_quantity::parse_quantity(v.as_bytes(), true);
        n as i64
    }
}

/// The INI settings at startup (PHP_INI_STAGE_STARTUP).
fn ensure_ini(vm: &Vm) {
    if ZLIBG.with(|g| g.borrow().ini_loaded) {
        return;
    }
    let v = parse_output_compression(&ini_str(vm, b"zlib.output_compression"));
    ZLIBG.with(|g| {
        let mut g = g.borrow_mut();
        g.ini_loaded = true;
        g.output_compression_default = v;
        g.output_compression = v;
    });
}

/// php_zlib_output_encoding()
fn output_encoding(vm: &mut Vm) -> i64 {
    let cur = ZLIBG.with(|g| g.borrow().compression_coding);
    if cur != 0 {
        return cur;
    }
    let _ = vm;
    let mut coding = 0;
    let enc = ZLIBG.with(|g| g.borrow().accept_encoding.clone());
    {
        if let Some(e) = enc {
            let e = e.as_slice();
            let has = |n: &[u8]| e.windows(n.len()).any(|w| w == n);
            if has(b"gzip") {
                coding = PHP_ZLIB_ENCODING_GZIP;
            } else if has(b"deflate") {
                coding = PHP_ZLIB_ENCODING_DEFLATE;
            }
        }
    }
    ZLIBG.with(|g| g.borrow_mut().compression_coding = coding);
    coding
}

fn new_deflate(vm: &Vm) -> Option<Box<Deflate>> {
    let level = ini_str(vm, b"zlib.output_compression_level")
        .trim()
        .parse::<i64>()
        .unwrap_or(-1);
    let coding = ZLIBG.with(|g| g.borrow().compression_coding);
    Deflate::new(
        level as i32,
        Z_DEFLATED,
        coding as i32,
        MAX_MEM_LEVEL,
        Z_DEFAULT_STRATEGY,
    )
    .ok()
}

/// php_zlib_output_handler_ex(): Ok(output) or Err(()) for FAILURE.
fn output_handler_ex(vm: &Vm, ctx: &mut ObCtx, input: &[u8], op: i64) -> Result<Vec<u8>, ()> {
    let mut flags = Z_SYNC_FLUSH;
    if op & OB_OP_START != 0 {
        ctx.z = Some(new_deflate(vm).ok_or(())?);
    }
    if op & OB_OP_CLEAN != 0 {
        ctx.z = None;
        if op & OB_OP_FINAL != 0 {
            return Ok(Vec::new());
        }
        ctx.z = Some(new_deflate(vm).ok_or(())?);
        ctx.buffer.clear();
        return Ok(Vec::new());
    }
    ctx.buffer.extend_from_slice(input);
    let out_size = buffer_size_guess(input.len());
    let mut out = vec![0u8; out_size];
    if op & OB_OP_FINAL != 0 {
        flags = Z_FINISH;
    } else if op & OB_OP_FLUSH != 0 {
        flags = Z_FULL_FLUSH;
    }
    let Some(z) = ctx.z.as_mut() else {
        return Err(());
    };
    let buffer = std::mem::take(&mut ctx.buffer);
    let mut io = Io::new(&buffer, &mut out);
    let status = z.deflate(&mut io, flags);
    let (consumed, produced) = (io.in_pos, io.out_pos);
    match status {
        Z_OK if flags == Z_FINISH => {
            ctx.z = None;
            return Err(());
        }
        Z_OK | Z_STREAM_END => {
            ctx.buffer = buffer[consumed..].to_vec();
            out.truncate(produced);
        }
        _ => {
            ctx.z = None;
            return Err(());
        }
    }
    if op & OB_OP_FINAL != 0 {
        ctx.z = None;
    }
    Ok(out)
}

/// Flags of the running handler (PHP_OUTPUT_HANDLER_HOOK_GET_FLAGS).
fn running_flags(vm: &Vm) -> Option<i64> {
    let id = vm.ob_running?;
    vm.ob_handlers.iter().find(|h| h.id == id).map(|h| h.flags)
}

/// php_zlib_output_handler()
fn output_handler(vm: &mut Vm, input: &[u8], op: i64) -> Option<Vec<u8>> {
    if output_encoding(vm) == 0 {
        if op & OB_OP_START != 0 && op != (OB_OP_START | OB_OP_CLEAN | OB_OP_FINAL) {
            sapi_add_header(b"Vary: Accept-Encoding", false);
        }
        return None;
    }
    let mut ctx = ZLIBG.with(|g| std::mem::take(&mut g.borrow_mut().handler));
    let r = output_handler_ex(vm, &mut ctx, input, op);
    let out = match r {
        Ok(o) => o,
        Err(()) => {
            ZLIBG.with(|g| g.borrow_mut().handler = ctx);
            return None;
        }
    };
    if op & OB_OP_CLEAN == 0 || (op & OB_OP_START != 0 && op & OB_OP_FINAL == 0) {
        let flags = running_flags(vm).unwrap_or(0);
        if flags & OBH_STARTED == 0 {
            let oc = ZLIBG.with(|g| g.borrow().output_compression);
            if vm.headers_sent || oc == 0 {
                ctx.z = None;
                ZLIBG.with(|g| g.borrow_mut().handler = ctx);
                return None;
            }
            match ZLIBG.with(|g| g.borrow().compression_coding) {
                PHP_ZLIB_ENCODING_GZIP => sapi_add_header(b"Content-Encoding: gzip", true),
                PHP_ZLIB_ENCODING_DEFLATE => sapi_add_header(b"Content-Encoding: deflate", true),
                _ => {
                    ctx.z = None;
                    ZLIBG.with(|g| g.borrow_mut().handler = ctx);
                    return None;
                }
            }
            sapi_add_header(b"Vary: Accept-Encoding", false);
        }
    }
    ZLIBG.with(|g| g.borrow_mut().handler = ctx);
    Some(out)
}

/// php_zlib_output_handler_init(): the alias constructor for
/// "ob_gzhandler" and the "zlib output compression" handler.
fn output_handler_init(
    vm: &mut Vm,
    name: &[u8],
    chunk_size: usize,
    flags: i64,
) -> Option<ObHandler> {
    ensure_ini(vm);
    ZLIBG.with(|g| {
        let mut g = g.borrow_mut();
        if g.output_compression == 0 {
            g.output_compression = if chunk_size != 0 {
                chunk_size as i64
            } else {
                PHP_OUTPUT_HANDLER_DEFAULT_SIZE
            };
        }
        g.handler_registered = true;
        g.handler = ObCtx::default();
    });
    Some(Vm::ob_make_handler(
        name.to_vec(),
        ObKind::InternalFallible(output_handler),
        chunk_size,
        flags,
    ))
}

/// php_output_handler_conflict()
fn handler_conflict(vm: &mut Vm, new: &[u8], set: &[u8]) -> bool {
    if vm.ob_handler_started(set) {
        let msg = if new != set {
            format!(
                "Output handler '{}' conflicts with '{}'",
                String::from_utf8_lossy(new),
                String::from_utf8_lossy(set)
            )
        } else {
            format!(
                "Output handler '{}' cannot be used twice",
                String::from_utf8_lossy(new)
            )
        };
        match vm.current_builtin_name() {
            Some(f) => vm.emit_warning(&format!("{}(): {}", f, msg)),
            None => vm.emit_warning(&msg),
        }
        return true;
    }
    false
}

/// php_zlib_output_conflict_check()
fn output_conflict_check(vm: &mut Vm, name: &[u8]) -> bool {
    if vm.ob_level() > 0 {
        for set in [
            PHP_ZLIB_OUTPUT_HANDLER_NAME,
            b"ob_gzhandler",
            b"mb_output_handler",
            b"URL-Rewriter",
        ] {
            if handler_conflict(vm, name, set) {
                return false;
            }
        }
    }
    true
}

/// php_zlib_output_compression_start()
fn output_compression_start(vm: &mut Vm) {
    let oc = ZLIBG.with(|g| g.borrow().output_compression);
    if oc == 0 {
        return;
    }
    if oc == 1 {
        ZLIBG.with(|g| g.borrow_mut().output_compression = PHP_OUTPUT_HANDLER_DEFAULT_SIZE);
    }
    if output_encoding(vm) == 0 {
        return;
    }
    let chunk = ZLIBG.with(|g| g.borrow().output_compression) as usize;
    let h = output_handler_init(vm, PHP_ZLIB_OUTPUT_HANDLER_NAME, chunk, OBH_STDFLAGS);
    if goro_ext_standard::ob::output_handler_start(vm, h) {
        let oh = ini_str(vm, b"zlib.output_handler");
        if !oh.is_empty() {
            let cb = Value::String(PhpString::from_string(oh));
            if let Ok(name) = callable_name(vm, &cb) {
                let h = Vm::ob_make_handler(name, ObKind::User(cb), chunk, OBH_STDFLAGS);
                goro_ext_standard::ob::output_handler_start(vm, Some(h));
            }
        }
    }
}

fn callable_name(vm: &Vm, cb: &Value) -> Result<Vec<u8>, ()> {
    match cb {
        Value::String(s) => {
            let l = s.as_bytes().to_ascii_lowercase();
            if vm.functions.contains_key(&l) || vm.user_functions.contains_key(&l) {
                Ok(s.as_bytes().to_vec())
            } else {
                Err(())
            }
        }
        _ => Err(()),
    }
}

/// PHP_RINIT_FUNCTION(zlib)
fn request_startup(vm: &mut Vm) {
    ensure_ini(vm);
    let enc = match vm.get_global(b"_SERVER").map(|v| v.deref()) {
        Some(Value::Array(srv)) => srv
            .borrow()
            .get(&ArrayKey::String(PhpString::from_bytes(
                b"HTTP_ACCEPT_ENCODING",
            )))
            .map(|v| v.deref().to_php_string().as_bytes().to_vec()),
        _ => None,
    };
    let registered = ZLIBG.with(|g| {
        let mut g = g.borrow_mut();
        g.compression_coding = 0;
        g.accept_encoding = enc;
        g.handler_registered
    });
    if !registered {
        ZLIBG.with(|g| {
            let mut g = g.borrow_mut();
            g.output_compression = g.output_compression_default;
        });
        output_compression_start(vm);
    }
}

/// OnUpdate_zlib_output_compression() at runtime.
fn ini_output_compression(vm: &mut Vm, _key: &[u8], value: &Value) -> bool {
    ensure_ini(vm);
    let v = value.to_php_string().to_string_lossy();
    let int_value = parse_output_compression(&v);
    let oh = ini_str(vm, b"output_handler");
    let prefix = |vm: &Vm| {
        vm.current_builtin_name()
            .map(|f| format!("{}(): ", f))
            .unwrap_or_default()
    };
    if !oh.is_empty() && int_value != 0 {
        let p = prefix(vm);
        let line = vm.current_line;
        vm.emit_fatal_at(
            &format!(
                "{}Cannot use both zlib.output_compression and output_handler together!!",
                p
            ),
            line,
        );
        return false;
    }
    if vm.headers_sent {
        let p = prefix(vm);
        vm.emit_warning(&format!(
            "{}Cannot change zlib.output_compression - headers already sent",
            p
        ));
        return false;
    }
    ZLIBG.with(|g| {
        let mut g = g.borrow_mut();
        g.output_compression_default = int_value;
        g.output_compression = int_value;
    });
    if int_value != 0 && !vm.ob_handler_started(PHP_ZLIB_OUTPUT_HANDLER_NAME) {
        output_compression_start(vm);
    }
    true
}

/// OnUpdate_zlib_output_handler()
fn ini_output_handler(vm: &mut Vm, _key: &[u8], _value: &Value) -> bool {
    if vm.headers_sent {
        let p = vm
            .current_builtin_name()
            .map(|f| format!("{}(): ", f))
            .unwrap_or_default();
        vm.emit_warning(&format!(
            "{}Cannot change zlib.output_handler - headers already sent",
            p
        ));
        return false;
    }
    true
}

/// PHP_FUNCTION(ob_gzhandler): the legacy ob_gzhandler() function.
pub(crate) fn ob_gzhandler(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.len() != 2 {
        let msg = format!(
            "ob_gzhandler() expects exactly 2 arguments, {} given",
            args.len()
        );
        return Err(crate::codec::throw(vm, b"ArgumentCountError", &msg));
    }
    let data = crate::codec::zpp_str(vm, &args[0].deref(), "ob_gzhandler", 1, "data")?;
    let flags = crate::codec::zpp_long(vm, &args[1].deref(), "ob_gzhandler", 2, "flags")?;
    let encoding = output_encoding(vm);
    if encoding == 0 {
        return Ok(Value::False);
    }
    if flags & OB_OP_START != 0 {
        match encoding {
            PHP_ZLIB_ENCODING_GZIP => sapi_add_header(b"Content-Encoding: gzip", true),
            PHP_ZLIB_ENCODING_DEFLATE => sapi_add_header(b"Content-Encoding: deflate", true),
            _ => {}
        }
        sapi_add_header(b"Vary: Accept-Encoding", false);
    }
    let mut ctx = ZLIBG
        .with(|g| g.borrow_mut().legacy.take())
        .unwrap_or_default();
    match output_handler_ex(vm, &mut ctx, data.as_bytes(), flags) {
        Ok(out) => {
            ZLIBG.with(|g| g.borrow_mut().legacy = Some(ctx));
            Ok(Value::String(PhpString::from_vec(out)))
        }
        Err(()) => Ok(Value::False),
    }
}

/// PHP_FUNCTION(zlib_get_coding_type)
pub(crate) fn zlib_get_coding_type(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(match ZLIBG.with(|g| g.borrow().compression_coding) {
        PHP_ZLIB_ENCODING_GZIP => Value::String(PhpString::from_bytes(b"gzip")),
        PHP_ZLIB_ENCODING_DEFLATE => Value::String(PhpString::from_bytes(b"deflate")),
        _ => Value::False,
    })
}

/// MINIT parts: INI entries, the handler alias and conflict checks.
pub(crate) fn register(vm: &mut Vm) {
    for (k, d) in [
        (&b"zlib.output_compression"[..], &b"0"[..]),
        (b"zlib.output_compression_level", b"-1"),
        (b"zlib.output_handler", b""),
    ] {
        vm.constants
            .entry(k.to_vec())
            .or_insert_with(|| Value::String(PhpString::from_bytes(d)));
    }
    vm.ini_set_hooks
        .insert(b"zlib.output_compression".to_vec(), ini_output_compression);
    vm.ini_set_hooks
        .insert(b"zlib.output_handler".to_vec(), ini_output_handler);
    goro_ext_standard::ob::register_output_handler_alias(b"ob_gzhandler", output_handler_init);
    goro_ext_standard::ob::register_output_handler_conflict(b"ob_gzhandler", output_conflict_check);
    goro_ext_standard::ob::register_output_handler_conflict(
        PHP_ZLIB_OUTPUT_HANDLER_NAME,
        output_conflict_check,
    );
    vm.request_startup_hooks.push(request_startup);
}
