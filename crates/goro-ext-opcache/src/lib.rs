//! php-src's ext/opcache ("Zend OPcache"): the INI directives with their
//! on-update handlers (zend_accelerator_module.c), the startup decisions of
//! accel_startup()/accel_post_startup() (ZendAccelerator.c), the JIT
//! configuration state (zend_jit.c's zend_jit_config()), the opcache_*()
//! API and preloading. The script cache itself lives in goro-core
//! (`script_cache`), which the VM consults when it includes a file.
//!
//! goro has no optimizer and no JIT: the directives are accepted and
//! reported as php-src does, but scripts run on goro's own VM.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::script_cache::{self, CompileOnly, LOG_DEBUG, LOG_FATAL, LOG_WARNING};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

mod preload;

/// The module name (`extension_loaded('Zend OPcache')`).
pub const NAME: &[u8] = b"zend opcache";
const PRODUCT_NAME: &str = "Zend OPcache";

type R = Result<Value, VmError>;

const FUNCTIONS: &[(&str, goro_core::vm::BuiltinFn, u8)] = &[
    ("opcache_reset", opcache_reset, 3),
    ("opcache_get_status", opcache_get_status, 3),
    ("opcache_compile_file", opcache_compile_file, 3),
    ("opcache_invalidate", opcache_invalidate, 3),
    ("opcache_jit_blacklist", opcache_jit_blacklist, 3),
    ("opcache_get_configuration", opcache_get_configuration, 3),
    ("opcache_is_script_cached", opcache_is_script_cached, 3),
    (
        "opcache_is_script_cached_in_file_cache",
        opcache_is_script_cached_in_file_cache,
        3,
    ),
];

/// How an INI entry may be changed.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    System,
    All,
}

/// The on-update handler of an INI entry.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Bool,
    Long,
    Str,
    /// OnUpdateStringUnempty
    StrUnempty,
    Real,
    Enable,
    MemoryConsumption,
    InternedStringsBuffer,
    MaxAcceleratedFiles,
    MaxWastedPercentage,
    FileCache,
    Jit,
    JitDebug,
    Counter,
    UnrollC,
    UnrollR,
    UnrollL,
    MaxTraceLength,
}

/// ZEND_INI_BEGIN() of zend_accelerator_module.c, in order.
const INI: &[(&str, &str, Mode, Kind)] = &[
    ("opcache.enable", "1", Mode::All, Kind::Enable),
    ("opcache.use_cwd", "1", Mode::System, Kind::Bool),
    ("opcache.validate_timestamps", "1", Mode::All, Kind::Bool),
    ("opcache.validate_permission", "0", Mode::System, Kind::Bool),
    ("opcache.validate_root", "0", Mode::System, Kind::Bool),
    ("opcache.dups_fix", "0", Mode::All, Kind::Bool),
    ("opcache.revalidate_path", "0", Mode::All, Kind::Bool),
    ("opcache.log_verbosity_level", "1", Mode::System, Kind::Long),
    (
        "opcache.memory_consumption",
        "128",
        Mode::System,
        Kind::MemoryConsumption,
    ),
    (
        "opcache.interned_strings_buffer",
        "8",
        Mode::System,
        Kind::InternedStringsBuffer,
    ),
    (
        "opcache.max_accelerated_files",
        "10000",
        Mode::System,
        Kind::MaxAcceleratedFiles,
    ),
    (
        "opcache.max_wasted_percentage",
        "5",
        Mode::System,
        Kind::MaxWastedPercentage,
    ),
    (
        "opcache.force_restart_timeout",
        "180",
        Mode::System,
        Kind::Long,
    ),
    ("opcache.revalidate_freq", "2", Mode::All, Kind::Long),
    ("opcache.file_update_protection", "2", Mode::All, Kind::Long),
    (
        "opcache.preferred_memory_model",
        "",
        Mode::System,
        Kind::StrUnempty,
    ),
    ("opcache.blacklist_filename", "", Mode::System, Kind::Str),
    ("opcache.max_file_size", "0", Mode::System, Kind::Long),
    ("opcache.protect_memory", "0", Mode::System, Kind::Bool),
    ("opcache.save_comments", "1", Mode::System, Kind::Bool),
    ("opcache.record_warnings", "0", Mode::System, Kind::Bool),
    (
        "opcache.optimization_level",
        "0x7FFEBFFF",
        Mode::System,
        Kind::Long,
    ),
    ("opcache.opt_debug_level", "0", Mode::System, Kind::Long),
    (
        "opcache.enable_file_override",
        "0",
        Mode::System,
        Kind::Bool,
    ),
    ("opcache.enable_cli", "0", Mode::System, Kind::Bool),
    ("opcache.error_log", "", Mode::System, Kind::Str),
    ("opcache.restrict_api", "", Mode::System, Kind::Str),
    ("opcache.lockfile_path", "/tmp", Mode::System, Kind::Str),
    ("opcache.file_cache", "", Mode::System, Kind::FileCache),
    (
        "opcache.file_cache_read_only",
        "0",
        Mode::System,
        Kind::Bool,
    ),
    ("opcache.file_cache_only", "0", Mode::System, Kind::Bool),
    (
        "opcache.file_cache_consistency_checks",
        "1",
        Mode::System,
        Kind::Bool,
    ),
    ("opcache.huge_code_pages", "0", Mode::System, Kind::Bool),
    ("opcache.preload", "", Mode::System, Kind::StrUnempty),
    ("opcache.preload_user", "", Mode::System, Kind::StrUnempty),
    ("opcache.jit", "disable", Mode::All, Kind::Jit),
    ("opcache.jit_buffer_size", "64M", Mode::System, Kind::Long),
    ("opcache.jit_debug", "0", Mode::All, Kind::JitDebug),
    ("opcache.jit_bisect_limit", "0", Mode::All, Kind::Long),
    ("opcache.jit_prof_threshold", "0.005", Mode::All, Kind::Real),
    (
        "opcache.jit_max_root_traces",
        "1024",
        Mode::System,
        Kind::Long,
    ),
    (
        "opcache.jit_max_side_traces",
        "128",
        Mode::System,
        Kind::Long,
    ),
    (
        "opcache.jit_max_exit_counters",
        "8192",
        Mode::System,
        Kind::Long,
    ),
    ("opcache.jit_hot_loop", "61", Mode::System, Kind::Counter),
    ("opcache.jit_hot_func", "127", Mode::System, Kind::Counter),
    ("opcache.jit_hot_return", "8", Mode::System, Kind::Counter),
    ("opcache.jit_hot_side_exit", "8", Mode::All, Kind::Counter),
    (
        "opcache.jit_blacklist_root_trace",
        "16",
        Mode::All,
        Kind::Counter,
    ),
    (
        "opcache.jit_blacklist_side_trace",
        "8",
        Mode::All,
        Kind::Counter,
    ),
    (
        "opcache.jit_max_loop_unrolls",
        "8",
        Mode::All,
        Kind::UnrollL,
    ),
    (
        "opcache.jit_max_recursive_calls",
        "2",
        Mode::All,
        Kind::UnrollC,
    ),
    (
        "opcache.jit_max_recursive_returns",
        "2",
        Mode::All,
        Kind::UnrollR,
    ),
    (
        "opcache.jit_max_polymorphic_calls",
        "2",
        Mode::All,
        Kind::Long,
    ),
    (
        "opcache.jit_max_trace_length",
        "1024",
        Mode::All,
        Kind::MaxTraceLength,
    ),
];

/// The names of the INI entries (for Reflection and ini_get_all()).
pub fn ini_entry_names() -> Vec<&'static str> {
    INI.iter().map(|e| e.0).collect()
}

/// MAX_INTERNED_STRINGS_BUFFER_SIZE on 64-bit platforms.
const MAX_INTERNED_STRINGS_BUFFER_SIZE: i64 = 32767;
const ZEND_JIT_ON_HOT_TRACE: i64 = 5;
const ZEND_JIT_ON_SCRIPT_LOAD: i64 = 0;
const ZEND_JIT_LEVEL_OPT_FUNCS: i64 = 4;
const ZEND_JIT_LEVEL_OPT_SCRIPT: i64 = 5;
const ZEND_JIT_REG_ALLOC_GLOBAL: i64 = 1 << 1;
const ZEND_JIT_CPU_AVX: i64 = 1 << 2;
const ZEND_JIT_DEBUG_PERSISTENT: i64 = 0x1f0 << 16;

/// zend_jit_globals (the configuration part)
#[derive(Clone, Default)]
struct Jit {
    enabled: bool,
    on: bool,
    trigger: i64,
    opt_level: i64,
    opt_flags: i64,
    debug: i64,
    /// The JIT buffer was set up (dasm_buf).
    buffer: i64,
}

#[derive(Default)]
struct State {
    /// accel_startup_ok
    startup_ok: bool,
    /// zps_api_failure_reason
    failure: Option<&'static str>,
    /// ZCG(enabled)
    enabled: bool,
    /// ZCG(accelerator_enabled)
    accelerator_enabled: bool,
    file_cache_only: bool,
    jit: Jit,
    /// opcache.memory_consumption in bytes, as accepted.
    memory_consumption: i64,
    /// opcache.max_wasted_percentage as a fraction.
    max_wasted_percentage: f64,
    interned_strings_buffer: i64,
    max_accelerated_files: i64,
    /// The blacklist (opcache.blacklist_filename's entries).
    blacklist: Vec<String>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State {
        enabled: true,
        memory_consumption: 128 * 1024 * 1024,
        max_wasted_percentage: 0.05,
        interned_strings_buffer: 8,
        max_accelerated_files: 10000,
        ..State::default()
    });
}

fn with<T>(f: impl FnOnce(&mut State) -> T) -> T {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

/// MINIT
pub fn register(vm: &mut Vm) {
    if vm.loaded_extensions.contains(NAME) {
        return;
    }
    vm.register_extension(NAME);
    vm.phpinfo_hooks.push(minfo);
    for &(name, f, flags) in FUNCTIONS {
        let params: Vec<&[u8]> = goro_core::stubs::find_function(name.as_bytes())
            .map(|s| s.args.iter().map(|a| a.name.as_bytes()).collect())
            .unwrap_or_default();
        vm.register_function_with_params(name.as_bytes(), f, &params);
        vm.zpp_extension_verified
            .insert(name.as_bytes().to_vec(), flags);
    }
    for &(name, default, mode, kind) in INI {
        vm.constants.insert(
            name.as_bytes().to_vec(),
            Value::String(PhpString::from_bytes(default.as_bytes())),
        );
        if mode == Mode::System {
            goro_core::reflection::register_ini_access(name, 4);
            vm.ini_set_hooks.insert(name.as_bytes().to_vec(), reject);
        } else {
            let hook: fn(&mut Vm, &[u8], &Value) -> bool = match kind {
                Kind::Enable => on_enable_runtime,
                Kind::Jit => on_jit_runtime,
                Kind::JitDebug => on_jit_debug_runtime,
                Kind::Counter | Kind::UnrollC | Kind::UnrollR | Kind::UnrollL => on_numeric_runtime,
                Kind::MaxTraceLength => on_numeric_runtime,
                _ => accept,
            };
            vm.ini_set_hooks.insert(name.as_bytes().to_vec(), hook);
        }
    }
    vm.startup_hooks.push(startup);
}

fn reject(_vm: &mut Vm, _name: &[u8], _value: &Value) -> bool {
    false
}

fn accept(_vm: &mut Vm, _name: &[u8], _value: &Value) -> bool {
    true
}

/// zend_ini_parse_bool()
fn parse_bool(s: &[u8]) -> bool {
    let t = s.trim_ascii();
    if t.eq_ignore_ascii_case(b"true")
        || t.eq_ignore_ascii_case(b"yes")
        || t.eq_ignore_ascii_case(b"on")
    {
        return true;
    }
    atoi(t) != 0
}

/// ZEND_ATOL / atoi: leading integer, 0 when none.
fn atoi(s: &[u8]) -> i64 {
    let s = s.trim_ascii_start();
    let (neg, digits) = match s.first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    let mut n: i64 = 0;
    for &c in digits {
        if !c.is_ascii_digit() {
            break;
        }
        n = n.wrapping_mul(10).wrapping_add((c - b'0') as i64);
    }
    if neg { -n } else { n }
}

/// zend_ini_parse_quantity_warn(): the value and the warning, if any.
fn quantity(name: &str, value: &[u8]) -> (i64, Option<String>) {
    let (v, err) = goro_core::ini_quantity::parse_quantity(value, true);
    (
        v as i64,
        err.map(|e| format!("Invalid \"{}\" setting. {}", name, e)),
    )
}

fn ini_raw(vm: &Vm, name: &str) -> Vec<u8> {
    vm.constants
        .get(name.as_bytes())
        .map(|v| v.to_php_string().as_bytes().to_vec())
        .unwrap_or_default()
}

fn ini_bool(vm: &Vm, name: &str) -> bool {
    match vm.constants.get(name.as_bytes()) {
        Some(Value::String(s)) => parse_bool(s.as_bytes()),
        Some(v) => v.is_truthy(),
        None => false,
    }
}

/// OnUpdateLong (zend_ini_parse_quantity_warn() without the warning).
fn ini_long(vm: &Vm, name: &str) -> i64 {
    quantity(name, &ini_raw(vm, name)).0
}

fn ini_real(vm: &Vm, name: &str) -> f64 {
    let s = String::from_utf8_lossy(&ini_raw(vm, name)).into_owned();
    goro_core::value::Value::String(PhpString::from_string(s)).to_double()
}

fn ini_str(vm: &Vm, name: &str) -> String {
    String::from_utf8_lossy(&ini_raw(vm, name)).into_owned()
}

/// A warning raised during startup ("in Unknown on line 0").
fn startup_warning(vm: &mut Vm, msg: &str) {
    if vm.error_reporting & 2 == 0 {
        return;
    }
    let display = ini_raw(vm, "display_startup_errors");
    let display_errors = ini_raw(vm, "display_errors");
    let off = |v: &[u8]| {
        v.is_empty() || v == b"0" || v.eq_ignore_ascii_case(b"off") || v.eq_ignore_ascii_case(b"no")
    };
    if off(&display) && !display.is_empty() || off(&display_errors) && !display_errors.is_empty() {
        return;
    }
    vm.write_output(format!("\nWarning: {} in Unknown on line 0\n", msg).as_bytes());
}

/// Restores the default of an entry whose startup value was rejected.
fn restore_default(vm: &mut Vm, name: &str) {
    if let Some(&(_, default, _, _)) = INI.iter().find(|e| e.0 == name) {
        vm.constants.insert(
            name.as_bytes().to_vec(),
            Value::String(PhpString::from_bytes(default.as_bytes())),
        );
    }
}

/// zend_jit_parse_config_num()
fn jit_parse_config_num(jit: &mut Jit, mut n: i64) -> bool {
    if n == 0 {
        jit.on = false;
        return true;
    }
    if n < 0 {
        return false;
    }
    if n % 10 == 0 || n % 10 > 5 {
        return false;
    }
    let opt_level = n % 10;
    n /= 10;
    if n % 10 > 5 || n % 10 == 4 {
        return false;
    }
    let trigger = n % 10;
    n /= 10;
    if n % 10 > 2 {
        return false;
    }
    let mut opt_flags = n % 10;
    n /= 10;
    if n % 10 > 1 {
        return false;
    }
    if n % 10 != 0 {
        opt_flags |= ZEND_JIT_CPU_AVX;
    }
    if n / 10 != 0 {
        return false;
    }
    jit.opt_level = opt_level;
    jit.trigger = trigger;
    jit.opt_flags = opt_flags;
    jit.on = true;
    true
}

/// zend_jit_config(): Err(Some(warning)) / Err(None) on failure.
fn jit_config(jit: &mut Jit, value: &[u8], startup: bool) -> Result<(), Option<&'static str>> {
    if !startup && !jit.enabled {
        return Err(Some(
            "Cannot change opcache.jit setting at run-time (JIT is disabled)",
        ));
    }
    let v = value;
    let is = |s: &str| v.eq_ignore_ascii_case(s.as_bytes());
    if is("disable") {
        jit.enabled = false;
        jit.on = false;
        return Ok(());
    }
    if v.is_empty() || is("0") || is("off") || is("no") || is("false") {
        jit.enabled = true;
        jit.on = false;
        return Ok(());
    }
    if is("1") || is("on") || is("yes") || is("true") || is("tracing") {
        jit.enabled = true;
        jit.on = true;
        jit.opt_level = ZEND_JIT_LEVEL_OPT_FUNCS;
        jit.trigger = ZEND_JIT_ON_HOT_TRACE;
        jit.opt_flags = ZEND_JIT_REG_ALLOC_GLOBAL | ZEND_JIT_CPU_AVX;
        return Ok(());
    }
    if is("function") {
        jit.enabled = true;
        jit.on = true;
        jit.opt_level = ZEND_JIT_LEVEL_OPT_SCRIPT;
        jit.trigger = ZEND_JIT_ON_SCRIPT_LOAD;
        jit.opt_flags = ZEND_JIT_REG_ALLOC_GLOBAL | ZEND_JIT_CPU_AVX;
        return Ok(());
    }
    // ZEND_STRTOL over the whole string
    let s = std::str::from_utf8(v).unwrap_or("");
    let num = s.parse::<i64>().ok();
    if let Some(num) = num {
        if jit_parse_config_num(jit, num) {
            jit.enabled = true;
            return Ok(());
        }
    }
    jit.enabled = false;
    jit.on = false;
    Err(Some(
        "Invalid \"opcache.jit\" setting. Should be \"disable\", \"on\", \"off\", \"tracing\", \"function\" or 4-digit number",
    ))
}

/// Validates the numeric JIT counters (OnUpdateCounter, OnUpdateUnroll*,
/// OnUpdateMaxTraceLength): the warning when the value is out of range.
fn numeric_check(name: &str, kind: Kind, value: &[u8]) -> (Option<String>, Option<String>) {
    let (v, qwarn) = quantity(name, value);
    let err = match kind {
        Kind::Counter if !(0..256).contains(&v) => Some(format!(
            "Invalid \"{}\" setting; using default value instead. Should be between 0 and 255",
            name
        )),
        Kind::UnrollC if !(v > 0 && v < 10) => Some(format!(
            "Invalid \"{}\" setting. Should be between 1 and 10",
            name
        )),
        Kind::UnrollR if !(0..4).contains(&v) => Some(format!(
            "Invalid \"{}\" setting. Should be between 0 and 4",
            name
        )),
        Kind::UnrollL if !(v > 0 && v < 10) => Some(format!(
            "Invalid \"{}\" setting. Should be between 1 and 10",
            name
        )),
        Kind::MaxTraceLength if !(v > 3 && v <= 1024) => Some(format!(
            "Invalid \"{}\" setting. Should be between 4 and 1024",
            name
        )),
        _ => None,
    };
    (qwarn, err)
}

fn on_numeric_runtime(vm: &mut Vm, name: &[u8], value: &Value) -> bool {
    let name = String::from_utf8_lossy(name).into_owned();
    let Some(&(_, _, _, kind)) = INI.iter().find(|e| e.0 == name) else {
        return true;
    };
    let (qwarn, err) = numeric_check(&name, kind, value.to_php_string().as_bytes());
    if let Some(w) = qwarn {
        vm.emit_warning(&w);
    }
    if let Some(e) = err {
        vm.emit_warning(&e);
        return false;
    }
    true
}

/// OnEnable at run-time: OPcache may only be disabled.
fn on_enable_runtime(vm: &mut Vm, _name: &[u8], value: &Value) -> bool {
    let new = match value {
        Value::String(s) => parse_bool(s.as_bytes()),
        v => v.is_truthy(),
    };
    if new {
        if with(|s| s.enabled) {
            return true;
        }
        vm.emit_warning(&format!(
            "{} can't be temporarily enabled (it may be only disabled until the end of request)",
            PRODUCT_NAME
        ));
        return false;
    }
    with(|s| {
        s.enabled = false;
        s.accelerator_enabled = false;
    });
    script_cache::CONFIG.with(|c| c.borrow_mut().enabled = false);
    true
}

/// OnUpdateJit at run-time.
fn on_jit_runtime(vm: &mut Vm, _name: &[u8], value: &Value) -> bool {
    let v = value.to_php_string().as_bytes().to_vec();
    let r = with(|s| {
        let mut jit = s.jit.clone();
        let r = jit_config(&mut jit, &v, false);
        // A failed parse disables the JIT as zend_jit_config() does; a
        // rejected run-time change leaves it as it was.
        if r.is_ok() || jit.enabled != s.jit.enabled || !s.jit.enabled {
            if r.is_ok() || s.jit.enabled {
                s.jit.enabled = jit.enabled;
                s.jit.on = jit.on;
                s.jit.trigger = jit.trigger;
                s.jit.opt_level = jit.opt_level;
                s.jit.opt_flags = jit.opt_flags;
            }
        }
        r
    });
    match r {
        Ok(()) => true,
        Err(w) => {
            if let Some(w) = w {
                vm.emit_warning(w);
            }
            false
        }
    }
}

/// OnUpdateJitDebug at run-time.
fn on_jit_debug_runtime(vm: &mut Vm, name: &[u8], value: &Value) -> bool {
    let name = String::from_utf8_lossy(name).into_owned();
    let (v, qwarn) = quantity(&name, value.to_php_string().as_bytes());
    if let Some(w) = qwarn {
        vm.emit_warning(&w);
    }
    let old = with(|s| s.jit.debug);
    if ((old ^ v) & ZEND_JIT_DEBUG_PERSISTENT) != 0 {
        vm.emit_warning("Some opcache.jit_debug bits cannot be changed after startup");
        return false;
    }
    with(|s| s.jit.debug = v);
    true
}

/// The startup-stage on-update handlers of the entries set at startup.
fn startup_ini(vm: &mut Vm) {
    script_cache::CONFIG.with(|c| {
        let mut c = c.borrow_mut();
        c.log_verbosity = atoi(&ini_raw(vm, "opcache.log_verbosity_level"));
        c.error_log = ini_str(vm, "opcache.error_log");
    });
    for &(name, _, _, kind) in INI {
        let raw = ini_raw(vm, name);
        let ok = match kind {
            Kind::Enable => {
                let b = parse_bool(&raw);
                with(|s| s.enabled = b);
                true
            }
            Kind::MemoryConsumption => {
                let n = atoi(&raw);
                if n < 8 {
                    script_cache::log(
                        LOG_WARNING,
                        "opcache.memory_consumption is set below the required 8MB.\n",
                    );
                    false
                } else {
                    let bytes = if n > i64::MAX / (1024 * 1024) {
                        i64::MAX & !(1024 * 1024 - 1)
                    } else {
                        n * 1024 * 1024
                    };
                    with(|s| s.memory_consumption = bytes);
                    true
                }
            }
            Kind::InternedStringsBuffer => {
                let (n, w) = quantity(name, &raw);
                if let Some(w) = w {
                    startup_warning(vm, &w);
                }
                if n < 0 {
                    script_cache::log(
                        LOG_WARNING,
                        &format!(
                            "opcache.interned_strings_buffer must be greater than or equal to 0, {} given.\n",
                            n
                        ),
                    );
                    false
                } else if n > MAX_INTERNED_STRINGS_BUFFER_SIZE {
                    script_cache::log(
                        LOG_WARNING,
                        &format!(
                            "opcache.interned_strings_buffer must be less than or equal to {}, {} given.\n",
                            MAX_INTERNED_STRINGS_BUFFER_SIZE, n
                        ),
                    );
                    false
                } else {
                    with(|s| s.interned_strings_buffer = n);
                    true
                }
            }
            Kind::MaxAcceleratedFiles => {
                let n = atoi(&raw);
                if n < 200 {
                    script_cache::log(
                        LOG_WARNING,
                        "opcache.max_accelerated_files is set below the required minimum (200).\n",
                    );
                    false
                } else if n > 1_000_000 {
                    script_cache::log(
                        LOG_WARNING,
                        "opcache.max_accelerated_files is set above the limit (1000000).\n",
                    );
                    false
                } else {
                    with(|s| s.max_accelerated_files = n);
                    true
                }
            }
            Kind::MaxWastedPercentage => {
                let n = atoi(&raw);
                if n <= 0 || n > 50 {
                    script_cache::log(
                        LOG_WARNING,
                        "opcache.max_wasted_percentage must be set between 1 and 50.\n",
                    );
                    false
                } else {
                    with(|s| s.max_wasted_percentage = n as f64 / 100.0);
                    true
                }
            }
            Kind::Jit => {
                let r = with(|s| jit_config(&mut s.jit, &raw, true));
                match r {
                    Ok(()) => true,
                    Err(w) => {
                        if let Some(w) = w {
                            startup_warning(vm, w);
                        }
                        false
                    }
                }
            }
            Kind::JitDebug => {
                let (n, w) = quantity(name, &raw);
                if let Some(w) = w {
                    startup_warning(vm, &w);
                }
                with(|s| s.jit.debug = n);
                true
            }
            Kind::Counter
            | Kind::UnrollC
            | Kind::UnrollR
            | Kind::UnrollL
            | Kind::MaxTraceLength => {
                let (qw, err) = numeric_check(name, kind, &raw);
                if let Some(w) = qw {
                    startup_warning(vm, &w);
                }
                if let Some(e) = err {
                    startup_warning(vm, &e);
                    false
                } else {
                    true
                }
            }
            Kind::Long => {
                let (_, w) = quantity(name, &raw);
                if let Some(w) = w {
                    startup_warning(vm, &w);
                }
                true
            }
            _ => true,
        };
        if !ok {
            restore_default(vm, name);
        }
    }
}

/// zend_system_id: an md5 over the PHP version and build, as 32 hex digits.
fn system_id(vm: &Vm) -> String {
    let version = vm
        .constants
        .get(b"PHP_VERSION".as_slice())
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_default();
    let sapi = vm
        .constants
        .get(b"PHP_SAPI".as_slice())
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_default();
    // (goro's: the build and the SAPI, which compiled scripts depend on,
    // see goro_core::file_cache)
    let build = goro_core::file_cache::build_id();
    let mut h: u128 = 0x6c62272e07bb014262b821756295c58d;
    for b in format!("{}API{}goro{}{:x}", version, "20250925", sapi, build).bytes() {
        h ^= b as u128;
        h = h.wrapping_mul(0x0000000001000000000000000000013B);
    }
    format!("{:032x}", h)
}

/// zend_accel_error_noreturn(ACCEL_LOG_FATAL, ...): logged (whatever the
/// verbosity), then the process exits.
fn fatal(vm: &mut Vm, msg: &str) -> ! {
    script_cache::log(LOG_FATAL, msg);
    let out = vm.take_output();
    unsafe {
        libc::write(1, out.as_ptr() as *const libc::c_void, out.len());
        libc::_exit(254);
    }
}

/// accel_startup() + accel_post_startup() + RINIT
fn startup(vm: &mut Vm) {
    startup_ini(vm);
    let enable_cli = ini_bool(vm, "opcache.enable_cli");
    // accel_find_sapi(): the SAPIs opcache supports (goro's web server
    // SAPI among them); the CLI ones only with opcache.enable_cli.
    let sapi = match vm.constants.get(b"PHP_SAPI".as_slice()) {
        Some(Value::String(s)) => s.as_bytes().to_vec(),
        _ => b"cli".to_vec(),
    };
    let is_cli = matches!(sapi.as_slice(), b"cli" | b"phpdbg");
    const SUPPORTED: &[&[u8]] = &[
        b"apache",
        b"fastcgi",
        b"cli-server",
        b"cgi-fcgi",
        b"fpm-fcgi",
        b"fpmi-fcgi",
        b"apache2handler",
        b"litespeed",
        b"uwsgi",
        b"frankenphp",
        b"goro-httpd",
    ];
    if is_cli && !enable_cli {
        with(|s| {
            s.startup_ok = false;
            s.failure = Some("Opcode Caching is disabled for CLI");
        });
        return;
    }
    if !is_cli && !SUPPORTED.contains(&sapi.as_slice()) {
        with(|s| {
            s.startup_ok = false;
            s.failure = Some(
                "Opcode Caching is only supported in Apache, FPM, FastCGI, FrankenPHP, LiteSpeed and uWSGI SAPIs",
            );
        });
        return;
    }
    if !with(|s| s.enabled) {
        return;
    }
    let file_cache_only = ini_bool(vm, "opcache.file_cache_only");
    let file_cache = ini_str(vm, "opcache.file_cache");
    if file_cache_only && file_cache.is_empty() {
        fatal(
            vm,
            "opcache.file_cache_only is set without a proper setting of opcache.file_cache",
        );
    }
    if !file_cache_only {
        // zend_accel_init_shm(): the interned strings buffer is allocated
        // from the shared memory segment.
        let (interned, mem) = with(|s| (s.interned_strings_buffer, s.memory_consumption));
        let want = interned * 1024 * 1024;
        if interned > 0 && want >= mem {
            fatal(
                vm,
                &format!(
                    "Insufficient shared memory for interned strings buffer! (tried to allocate {} bytes)",
                    want
                ),
            );
        }
    }
    let jit_buffer = ini_long(vm, "opcache.jit_buffer_size");
    let replaced_execute_ex = ini_bool(vm, "zend_test.replace_zend_execute_ex")
        && vm.loaded_extensions.contains(b"zend_test".as_slice());
    let mut jit_warning = None;
    with(|s| {
        s.file_cache_only = file_cache_only;
        if file_cache_only {
            s.jit.enabled = false;
            s.jit.on = false;
        } else if s.jit.enabled && jit_buffer != 0 {
            // zend_jit_check_support()
            if replaced_execute_ex {
                jit_warning = Some(
                    "JIT is incompatible with third party extensions that override zend_execute_ex(). JIT disabled.",
                );
                s.jit.enabled = false;
                s.jit.on = false;
            } else if jit_buffer > 2 * 1024 * 1024 * 1024 {
                jit_warning =
                    Some("JIT on x86_64 doesn't support opcache.jit_buffer_size above 2G.");
                s.jit.enabled = false;
                s.jit.on = false;
            }
        }
        if s.jit.enabled && jit_buffer == 0 {
            s.jit.enabled = false;
            s.jit.on = false;
        }
        if s.jit.enabled {
            // The page-aligned buffer, less the JIT's own header.
            let page = 4096;
            s.jit.buffer = (jit_buffer + page - 1) / page * page;
        }
        s.startup_ok = true;
        s.accelerator_enabled = !file_cache_only;
    });
    if let Some(w) = jit_warning {
        startup_warning(vm, w);
    }
    let blacklist = ini_str(vm, "opcache.blacklist_filename");
    if !blacklist.is_empty() {
        let entries = load_blacklist(&blacklist);
        script_cache::CONFIG.with(|c| c.borrow_mut().blacklist = entries.clone());
        with(|s| s.blacklist = entries);
    }
    script_cache::CONFIG.with(|c| {
        let mut c = c.borrow_mut();
        c.enabled = false;
        c.validate_timestamps = ini_bool(vm, "opcache.validate_timestamps");
        c.revalidate_freq = ini_long(vm, "opcache.revalidate_freq");
        c.file_update_protection = ini_long(vm, "opcache.file_update_protection");
        c.record_warnings = ini_bool(vm, "opcache.record_warnings");
        c.revalidate_path = ini_bool(vm, "opcache.revalidate_path");
        c.file_cache = file_cache.trim_end_matches('/').to_string();
        c.file_cache_read_only = ini_bool(vm, "opcache.file_cache_read_only");
        c.system_id = system_id(vm);
        c.max_file_size = ini_long(vm, "opcache.max_file_size");
    });
    script_cache::STATS.with(|s| s.borrow_mut().start_time = script_cache::now());
    let preload = ini_str(vm, "opcache.preload");
    if !preload.is_empty() && !preload::run(vm, &preload, file_cache_only) {
        // Preloading failed: PHP does not start (the request is not run).
        vm.exit_requested = true;
        vm.exit_code = 255;
        return;
    }
    script_cache::CONFIG.with(|c| c.borrow_mut().enabled = !file_cache_only);
    if !file_cache_only {
        script_cache::init_shared();
    }
    // The main script is compiled through the cache.
    let main = vm.current_file.to_string();
    if !main.is_empty() && !main.contains("://") && main != "Standard input code" {
        let path = std::fs::canonicalize(&main)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or(main);
        script_cache::file_cache_store(&path);
        if !file_cache_only && script_cache::store_main(&path) {
            vm.run_optimizer_passes();
        }
    }
}

/// zend_accel_blacklist_load(): the files matching the glob, one path per
/// line (';' starts a comment, a quoted path is unquoted), relative paths
/// being relative to the blacklist file's directory.
fn load_blacklist(spec: &str) -> Vec<String> {
    let mut out = Vec::new();
    let files = glob_paths(spec);
    if files.is_empty() {
        script_cache::log(
            LOG_WARNING,
            &format!("No blacklist file found matching: {}\n", spec),
        );
    }
    for f in files {
        let Ok(data) = std::fs::read(&f) else {
            script_cache::log(LOG_WARNING, &format!("Cannot load blacklist file: {}\n", f));
            continue;
        };
        let dir = std::fs::canonicalize(&f)
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_string_lossy().into_owned()));
        for line in data.split(|&c| c == b'\n') {
            let mut l = line;
            if l.last() == Some(&b'\r') {
                l = &l[..l.len() - 1];
            }
            while l.first() == Some(&b'\r') {
                l = &l[1..];
            }
            if l.len() >= 2 && l[0] == b'"' && l[l.len() - 1] == b'"' {
                l = &l[1..l.len() - 1];
            }
            if l.is_empty() || l[0] == b';' {
                continue;
            }
            let l = String::from_utf8_lossy(l).into_owned();
            out.push(expand_filepath(&l, dir.as_deref()));
        }
    }
    out
}

/// expand_filepath_ex(): an absolute, normalized path (no symlinks
/// resolved).
fn expand_filepath(path: &str, relative_to: Option<&str>) -> String {
    let base = if path.starts_with('/') {
        String::new()
    } else {
        match relative_to {
            Some(d) => d.to_string(),
            None => std::env::current_dir()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
        }
    };
    let full = format!("{}/{}", base, path);
    let mut parts: Vec<&str> = Vec::new();
    for seg in full.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    format!("/{}", parts.join("/"))
}

fn glob_paths(pattern: &str) -> Vec<String> {
    use std::ffi::{CStr, CString};
    let Ok(c) = CString::new(pattern) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    unsafe {
        let mut g: libc::glob_t = std::mem::zeroed();
        if libc::glob(c.as_ptr(), 0, None, &mut g) == 0 {
            for i in 0..g.gl_pathc {
                let p = *g.gl_pathv.add(i);
                if !p.is_null() {
                    out.push(CStr::from_ptr(p).to_string_lossy().into_owned());
                }
            }
        }
        libc::globfree(&mut g);
    }
    out
}

fn assoc(arr: &mut PhpArray, k: &str, v: Value) {
    arr.set(ArrayKey::String(PhpString::from_bytes(k.as_bytes())), v);
}

fn s(v: &str) -> Value {
    Value::String(PhpString::from_bytes(v.as_bytes()))
}

fn b(v: bool) -> Value {
    if v { Value::True } else { Value::False }
}

fn arr(a: PhpArray) -> Value {
    Value::Array(Rc::new(RefCell::new(a)))
}

/// validate_api_restriction()
fn validate_api_restriction(vm: &mut Vm) -> bool {
    let restrict = ini_str(vm, "opcache.restrict_api");
    if restrict.is_empty() {
        return true;
    }
    let script = std::fs::canonicalize(&vm.current_file)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| vm.current_file.to_string());
    if !script.starts_with(&restrict) {
        vm.emit_warning(&format!(
            "{} API is restricted by \"restrict_api\" configuration directive",
            PRODUCT_NAME
        ));
        return false;
    }
    true
}

/// accel_make_persistent_key() for a user-supplied name: the resolved
/// path (zend_resolve_path() through the include_path).
fn resolve(vm: &Vm, name: &str) -> Option<String> {
    if name.is_empty() {
        return None;
    }
    let p = if name.starts_with('/') {
        name.to_string()
    } else {
        vm.resolve_include_path(name)
    };
    std::fs::canonicalize(&p)
        .ok()
        .map(|c| c.to_string_lossy().into_owned())
}

fn opcache_reset(vm: &mut Vm, _args: &[Value]) -> R {
    if !validate_api_restriction(vm) {
        return Ok(Value::False);
    }
    if !with(|s| s.enabled && s.startup_ok && s.accelerator_enabled) {
        return Ok(Value::False);
    }
    if script_cache::log_enabled(LOG_DEBUG) {
        vm.flush_cli_output();
        script_cache::log(LOG_DEBUG, "Restart Scheduled! Reason: user");
    }
    script_cache::reset();
    Ok(Value::True)
}

fn opcache_invalidate(vm: &mut Vm, args: &[Value]) -> R {
    let name = args
        .first()
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_default();
    let force = args.get(1).is_some_and(|v| v.is_truthy());
    if !validate_api_restriction(vm) {
        return Ok(Value::False);
    }
    if !with(|s| s.accelerator_enabled) {
        return Ok(Value::False);
    }
    let (key, found) = match resolve(vm, &name) {
        Some(p) => (p, true),
        None => (name, false),
    };
    // zend_file_cache_invalidate()
    if let Some(bin) = script_cache::file_cache_bin_path(&key) {
        let _ = std::fs::remove_file(bin);
    }
    let cached = script_cache::invalidate(&key, force);
    Ok(b(found || cached))
}

fn opcache_is_script_cached(vm: &mut Vm, args: &[Value]) -> R {
    let name = args
        .first()
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_default();
    if !validate_api_restriction(vm) {
        return Ok(Value::False);
    }
    if !with(|s| s.accelerator_enabled) {
        return Ok(Value::False);
    }
    Ok(b(
        resolve(vm, &name).is_some_and(|p| script_cache::is_cached(&p))
    ))
}

fn opcache_is_script_cached_in_file_cache(vm: &mut Vm, args: &[Value]) -> R {
    let name = args
        .first()
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_default();
    if !validate_api_restriction(vm) {
        return Ok(Value::False);
    }
    if !with(|s| s.accelerator_enabled || s.file_cache_only) {
        return Ok(Value::False);
    }
    if ini_str(vm, "opcache.file_cache").is_empty() {
        return Ok(Value::False);
    }
    Ok(b(
        resolve(vm, &name).is_some_and(|p| script_cache::in_file_cache(&p))
    ))
}

fn opcache_jit_blacklist(_vm: &mut Vm, _args: &[Value]) -> R {
    Ok(Value::Null)
}

fn opcache_compile_file(vm: &mut Vm, args: &[Value]) -> R {
    let name = args
        .first()
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_default();
    if !with(|s| s.startup_ok) && !preload::active() {
        let line = vm.current_line;
        vm.emit_notice_at(
            &format!(
                "{} has not been properly started, can't compile file",
                PRODUCT_NAME
            ),
            line,
        );
        return Ok(Value::False);
    }
    let path = if name.starts_with('/') {
        name.clone()
    } else {
        let cur = vm.current_file.clone();
        vm.resolve_include_path_from(&name, &cur)
    };
    let path = std::fs::canonicalize(&path)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or(path);
    match vm.compile_file_only(&path) {
        CompileOnly::Compiled => Ok(Value::True),
        CompileOnly::OpenFailed(reason) => {
            vm.emit_warning(&format!(
                "opcache_compile_file({}): Failed to open stream: {}",
                name, reason
            ));
            let include_path = ini_str(vm, "include_path");
            vm.emit_warning(&format!(
                "opcache_compile_file(): Failed opening '{}' for inclusion (include_path='{}')",
                name, include_path
            ));
            Ok(Value::False)
        }
        CompileOnly::ParseError => Err(VmError {
            message: String::new(),
            line: vm.current_line,
        }),
        CompileOnly::CompileError if preload::active() => {
            // During preloading, a failure fails the whole preloading.
            vm.fatal_emitted = true;
            Err(VmError {
                message: String::new(),
                line: vm.current_line,
            })
        }
        CompileOnly::CompileError => {
            vm.emit_warning(&format!("{} could not compile file {}", PRODUCT_NAME, name));
            Ok(Value::False)
        }
    }
}

fn opcache_get_status(vm: &mut Vm, args: &[Value]) -> R {
    let fetch_scripts = args.first().is_none_or(|v| v.is_truthy());
    if !validate_api_restriction(vm) {
        return Ok(Value::False);
    }
    if !with(|s| s.startup_ok) {
        return Ok(Value::False);
    }
    let mut r = PhpArray::new();
    let (accel, file_cache_only, mem, jit) = with(|s| {
        (
            s.accelerator_enabled,
            s.file_cache_only,
            s.memory_consumption,
            s.jit.clone(),
        )
    });
    assoc(&mut r, "opcache_enabled", b(accel));
    let file_cache = ini_str(vm, "opcache.file_cache");
    if !file_cache.is_empty() {
        assoc(&mut r, "file_cache", s(&file_cache));
    }
    if file_cache_only {
        assoc(&mut r, "file_cache_only", Value::True);
        return Ok(arr(r));
    }
    let stats = script_cache::STATS.with(|s| s.borrow().clone());
    assoc(&mut r, "cache_full", Value::False);
    assoc(&mut r, "restart_pending", b(stats.restart_pending));
    assoc(&mut r, "restart_in_progress", Value::False);

    // The shared memory segment holds the interned strings buffer and the
    // JIT buffer besides the scripts.
    let interned = with(|s| s.interned_strings_buffer) * 1024 * 1024;
    let base_used = 1_048_576 + interned;
    let used = (base_used + script_cache::used_memory()).min(mem);
    let mut mu = PhpArray::new();
    assoc(&mut mu, "used_memory", Value::Long(used));
    assoc(&mut mu, "free_memory", Value::Long(mem - used));
    assoc(&mut mu, "wasted_memory", Value::Long(0));
    assoc(&mut mu, "current_wasted_percentage", Value::Double(0.0));
    assoc(&mut r, "memory_usage", arr(mu));

    if interned > 0 {
        let isu_used = (interned / 4).min(interned);
        let mut isu = PhpArray::new();
        assoc(&mut isu, "buffer_size", Value::Long(interned));
        assoc(&mut isu, "used_memory", Value::Long(isu_used));
        assoc(&mut isu, "free_memory", Value::Long(interned - isu_used));
        assoc(&mut isu, "number_of_strings", Value::Long(9000));
        assoc(&mut r, "interned_strings_usage", arr(isu));
    }

    let max_files = with(|s| s.max_accelerated_files);
    // zend_accel_hash_init(): the next prime above the configured number.
    let max_keys = next_prime(max_files);
    let mut st = PhpArray::new();
    let n = script_cache::num_scripts() as i64;
    assoc(&mut st, "num_cached_scripts", Value::Long(n));
    assoc(&mut st, "num_cached_keys", Value::Long(n));
    assoc(&mut st, "max_cached_keys", Value::Long(max_keys));
    assoc(&mut st, "hits", Value::Long(stats.hits));
    assoc(&mut st, "start_time", Value::Long(stats.start_time));
    assoc(
        &mut st,
        "last_restart_time",
        Value::Long(stats.last_restart_time),
    );
    assoc(&mut st, "oom_restarts", Value::Long(0));
    assoc(&mut st, "hash_restarts", Value::Long(0));
    assoc(
        &mut st,
        "manual_restarts",
        Value::Long(stats.manual_restarts),
    );
    assoc(
        &mut st,
        "misses",
        Value::Long(stats.misses - stats.blacklist_misses),
    );
    assoc(
        &mut st,
        "blacklist_misses",
        Value::Long(stats.blacklist_misses),
    );
    let reqs = stats.hits + stats.misses;
    assoc(
        &mut st,
        "blacklist_miss_ratio",
        Value::Double(if reqs > 0 {
            stats.blacklist_misses as f64 / reqs as f64 * 100.0
        } else {
            0.0
        }),
    );
    assoc(
        &mut st,
        "opcache_hit_rate",
        Value::Double(if reqs > 0 {
            stats.hits as f64 / reqs as f64 * 100.0
        } else {
            0.0
        }),
    );
    assoc(&mut r, "opcache_statistics", arr(st));

    if let Some(p) = preload::statistics() {
        assoc(&mut r, "preload_statistics", p);
    }

    if fetch_scripts && accel {
        let validate = ini_bool(vm, "opcache.validate_timestamps");
        let mut scripts = PhpArray::new();
        for (path, hits, mem, last_used, ts, reval) in script_cache::scripts() {
            let mut e = PhpArray::new();
            assoc(&mut e, "full_path", s(&path));
            assoc(&mut e, "hits", Value::Long(hits));
            assoc(&mut e, "memory_consumption", Value::Long(mem));
            assoc(&mut e, "last_used", s(&asctime(last_used)));
            assoc(&mut e, "last_used_timestamp", Value::Long(last_used));
            if validate {
                assoc(&mut e, "timestamp", Value::Long(ts));
            }
            assoc(&mut e, "revalidate", Value::Long(reval));
            scripts.set(
                ArrayKey::String(PhpString::from_bytes(path.as_bytes())),
                arr(e),
            );
        }
        assoc(&mut r, "scripts", arr(scripts));
    }

    // zend_jit_status()
    let mut j = PhpArray::new();
    assoc(&mut j, "enabled", b(jit.enabled));
    assoc(&mut j, "on", b(jit.on));
    assoc(&mut j, "kind", Value::Long(jit.trigger));
    assoc(&mut j, "opt_level", Value::Long(jit.opt_level));
    assoc(&mut j, "opt_flags", Value::Long(jit.opt_flags));
    if jit.enabled && jit.buffer > 0 {
        let size = jit.buffer - 16;
        assoc(&mut j, "buffer_size", Value::Long(size));
        assoc(&mut j, "buffer_free", Value::Long(size - 2448.min(size)));
    } else {
        assoc(&mut j, "buffer_size", Value::Long(0));
        assoc(&mut j, "buffer_free", Value::Long(0));
    }
    assoc(&mut r, "jit", arr(j));
    Ok(arr(r))
}

fn next_prime(n: i64) -> i64 {
    // zend_accel_hash.c's prime_numbers table
    const PRIMES: &[i64] = &[
        5, 11, 19, 53, 107, 223, 463, 983, 1979, 3907, 7963, 16229, 32531, 65407, 130987, 262237,
        524521, 1048793,
    ];
    for &p in PRIMES {
        if p >= n {
            return p;
        }
    }
    *PRIMES.last().unwrap()
}

/// asctime(localtime(t)) without the trailing newline.
fn asctime(t: i64) -> String {
    unsafe {
        let tt: libc::time_t = t as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&tt, &mut tm);
        let mut buf = [0 as libc::c_char; 64];
        let p = libc::asctime_r(&tm, buf.as_mut_ptr());
        if p.is_null() {
            return String::new();
        }
        let s = std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned();
        s.trim_end_matches('\n').to_string()
    }
}

fn opcache_get_configuration(vm: &mut Vm, _args: &[Value]) -> R {
    if !validate_api_restriction(vm) {
        return Ok(Value::False);
    }
    let st = with(|s| {
        (
            s.enabled,
            s.memory_consumption,
            s.interned_strings_buffer,
            s.max_accelerated_files,
            s.max_wasted_percentage,
            s.jit.debug,
            s.blacklist.clone(),
        )
    });
    let mut d = PhpArray::new();
    let bo = |vm: &Vm, n: &str| b(ini_bool(vm, n));
    let lo = |vm: &Vm, n: &str| Value::Long(ini_long(vm, n));
    let st_ = |vm: &Vm, n: &str| s(&ini_str(vm, n));
    assoc(&mut d, "opcache.enable", b(st.0));
    for n in [
        "opcache.enable_cli",
        "opcache.use_cwd",
        "opcache.validate_timestamps",
        "opcache.validate_permission",
        "opcache.validate_root",
        "opcache.dups_fix",
        "opcache.revalidate_path",
    ] {
        assoc(&mut d, n, bo(vm, n));
    }
    assoc(
        &mut d,
        "opcache.log_verbosity_level",
        lo(vm, "opcache.log_verbosity_level"),
    );
    assoc(&mut d, "opcache.memory_consumption", Value::Long(st.1));
    assoc(&mut d, "opcache.interned_strings_buffer", Value::Long(st.2));
    assoc(&mut d, "opcache.max_accelerated_files", Value::Long(st.3));
    assoc(&mut d, "opcache.max_wasted_percentage", Value::Double(st.4));
    assoc(
        &mut d,
        "opcache.force_restart_timeout",
        lo(vm, "opcache.force_restart_timeout"),
    );
    assoc(
        &mut d,
        "opcache.revalidate_freq",
        lo(vm, "opcache.revalidate_freq"),
    );
    assoc(
        &mut d,
        "opcache.preferred_memory_model",
        st_(vm, "opcache.preferred_memory_model"),
    );
    assoc(
        &mut d,
        "opcache.blacklist_filename",
        st_(vm, "opcache.blacklist_filename"),
    );
    assoc(
        &mut d,
        "opcache.max_file_size",
        lo(vm, "opcache.max_file_size"),
    );
    assoc(&mut d, "opcache.error_log", st_(vm, "opcache.error_log"));
    for n in [
        "opcache.protect_memory",
        "opcache.save_comments",
        "opcache.record_warnings",
        "opcache.enable_file_override",
    ] {
        assoc(&mut d, n, bo(vm, n));
    }
    assoc(
        &mut d,
        "opcache.optimization_level",
        lo(vm, "opcache.optimization_level"),
    );
    assoc(
        &mut d,
        "opcache.lockfile_path",
        st_(vm, "opcache.lockfile_path"),
    );
    assoc(&mut d, "opcache.file_cache", st_(vm, "opcache.file_cache"));
    for n in [
        "opcache.file_cache_read_only",
        "opcache.file_cache_only",
        "opcache.file_cache_consistency_checks",
    ] {
        assoc(&mut d, n, bo(vm, n));
    }
    assoc(
        &mut d,
        "opcache.file_update_protection",
        lo(vm, "opcache.file_update_protection"),
    );
    assoc(
        &mut d,
        "opcache.opt_debug_level",
        lo(vm, "opcache.opt_debug_level"),
    );
    assoc(
        &mut d,
        "opcache.restrict_api",
        st_(vm, "opcache.restrict_api"),
    );
    assoc(
        &mut d,
        "opcache.huge_code_pages",
        bo(vm, "opcache.huge_code_pages"),
    );
    assoc(&mut d, "opcache.preload", st_(vm, "opcache.preload"));
    assoc(
        &mut d,
        "opcache.preload_user",
        st_(vm, "opcache.preload_user"),
    );
    assoc(&mut d, "opcache.jit", st_(vm, "opcache.jit"));
    assoc(
        &mut d,
        "opcache.jit_buffer_size",
        lo(vm, "opcache.jit_buffer_size"),
    );
    assoc(&mut d, "opcache.jit_debug", Value::Long(st.5));
    for n in [
        "opcache.jit_bisect_limit",
        "opcache.jit_blacklist_root_trace",
        "opcache.jit_blacklist_side_trace",
        "opcache.jit_hot_func",
        "opcache.jit_hot_loop",
        "opcache.jit_hot_return",
        "opcache.jit_hot_side_exit",
        "opcache.jit_max_exit_counters",
        "opcache.jit_max_loop_unrolls",
        "opcache.jit_max_polymorphic_calls",
        "opcache.jit_max_recursive_calls",
        "opcache.jit_max_recursive_returns",
        "opcache.jit_max_root_traces",
        "opcache.jit_max_side_traces",
    ] {
        assoc(&mut d, n, lo(vm, n));
    }
    assoc(
        &mut d,
        "opcache.jit_prof_threshold",
        Value::Double(ini_real(vm, "opcache.jit_prof_threshold")),
    );
    assoc(
        &mut d,
        "opcache.jit_max_trace_length",
        lo(vm, "opcache.jit_max_trace_length"),
    );

    let mut r = PhpArray::new();
    assoc(&mut r, "directives", arr(d));
    let mut v = PhpArray::new();
    let version = vm
        .constants
        .get(b"PHP_VERSION".as_slice())
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_default();
    assoc(&mut v, "version", s(&version));
    assoc(&mut v, "opcache_product_name", s(PRODUCT_NAME));
    assoc(&mut r, "version", arr(v));
    let mut bl = PhpArray::new();
    for p in st.6 {
        bl.push(s(&p));
    }
    assoc(&mut r, "blacklist", arr(bl));
    Ok(arr(r))
}

/// zend_accel_info()
fn minfo(vm: &mut Vm) -> String {
    let (accel, file_cache_only, enabled, startup_ok, failure, jit) = with(|s| {
        (
            s.accelerator_enabled,
            s.file_cache_only,
            s.enabled,
            s.startup_ok,
            s.failure,
            s.jit.clone(),
        )
    });
    let mut out = String::from("\nZend OPcache\n\n");
    let row = |out: &mut String, k: &str, v: &str| {
        out.push_str(&format!("{} => {}\n", k, v));
    };
    row(
        &mut out,
        "Opcode Caching",
        if accel || file_cache_only {
            "Up and Running"
        } else {
            "Disabled"
        },
    );
    row(
        &mut out,
        "Optimization",
        if enabled && startup_ok && ini_long(vm, "opcache.optimization_level") != 0 {
            "Enabled"
        } else {
            "Disabled"
        },
    );
    row(
        &mut out,
        "SHM Cache",
        if !file_cache_only {
            "Enabled"
        } else {
            "Disabled"
        },
    );
    row(
        &mut out,
        "File Cache",
        if !ini_str(vm, "opcache.file_cache").is_empty() {
            "Enabled"
        } else {
            "Disabled"
        },
    );
    row(
        &mut out,
        "JIT",
        if jit.enabled {
            if jit.on { "On" } else { "Off" }
        } else {
            "Disabled"
        },
    );
    if file_cache_only || enabled {
        if !startup_ok || failure.is_some() {
            row(&mut out, "Startup Failed", failure.unwrap_or(""));
        } else {
            row(&mut out, "Startup", "OK");
            if !file_cache_only {
                let stats = script_cache::STATS.with(|s| s.borrow().clone());
                row(&mut out, "Shared memory model", "mmap");
                row(&mut out, "Cache hits", &stats.hits.to_string());
                row(&mut out, "Cache misses", &stats.misses.to_string());
                let n = script_cache::num_scripts().to_string();
                row(&mut out, "Cached scripts", &n);
                row(&mut out, "Cached keys", &n);
            }
        }
    }
    out.push_str("\nDirective => Local Value => Master Value\n");
    let mut names: Vec<&str> = INI.iter().map(|e| e.0).collect();
    names.sort();
    for n in names {
        let local = ini_str(vm, n);
        let master = match vm.ini_originals.get(n.as_bytes()) {
            Some(Some(v)) => v.to_php_string().to_string_lossy(),
            Some(None) => String::new(),
            None => local.clone(),
        };
        let kind = INI.iter().find(|e| e.0 == n).map(|e| e.3);
        let disp = |v: &str| -> String {
            if matches!(kind, Some(Kind::Bool) | Some(Kind::Enable)) {
                if parse_bool(v.as_bytes()) {
                    "On".into()
                } else {
                    "Off".into()
                }
            } else if v.is_empty() {
                "no value".into()
            } else {
                v.to_string()
            }
        };
        out.push_str(&format!("{} => {} => {}\n", n, disp(&local), disp(&master)));
    }
    out
}
