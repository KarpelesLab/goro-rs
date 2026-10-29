//! The Zend OPcache extension's user-visible surface (PHP manual: OPcache
//! functions and runtime configuration). goro compiles scripts per
//! request and has no shared code cache, optimizer or JIT, so the cache
//! is reported per request: a script counts as cached once it was
//! compiled in this request while the cache is enabled.

use goro_runtime::{Key, PhpArray, PhpStr, Value};

use crate::func::Args;
use crate::registry::Module;
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "Zend OPcache",
    functions: &[
        (
            "function opcache_compile_file(string $filename): bool",
            opcache_compile_file,
        ),
        (
            "function opcache_get_configuration(): array|false",
            opcache_get_configuration,
        ),
        (
            "function opcache_get_status(bool $include_scripts = true): array|false",
            opcache_get_status,
        ),
        (
            "function opcache_invalidate(string $filename, bool $force = false): bool",
            opcache_invalidate,
        ),
        (
            "function opcache_is_script_cached(string $filename): bool",
            opcache_is_script_cached,
        ),
        ("function opcache_reset(): bool", opcache_reset),
        (
            "function opcache_jit_blacklist(Closure $closure): void",
            opcache_jit_blacklist,
        ),
    ],
    // PHP manual: OPcache runtime configuration (defaults).
    ini: &[
        ("opcache.enable", "1"),
        ("opcache.enable_cli", "0"),
        ("opcache.memory_consumption", "128"),
        ("opcache.interned_strings_buffer", "8"),
        ("opcache.max_accelerated_files", "10000"),
        ("opcache.max_wasted_percentage", "5"),
        ("opcache.use_cwd", "1"),
        ("opcache.validate_timestamps", "1"),
        ("opcache.revalidate_freq", "2"),
        ("opcache.revalidate_path", "0"),
        ("opcache.save_comments", "1"),
        ("opcache.record_warnings", "0"),
        ("opcache.enable_file_override", "0"),
        ("opcache.optimization_level", "0x7FFEBFFF"),
        ("opcache.dups_fix", "0"),
        ("opcache.blacklist_filename", ""),
        ("opcache.max_file_size", "0"),
        ("opcache.consistency_checks", "0"),
        ("opcache.force_restart_timeout", "180"),
        ("opcache.error_log", ""),
        ("opcache.log_verbosity_level", "1"),
        ("opcache.preferred_memory_model", ""),
        ("opcache.protect_memory", "0"),
        ("opcache.restrict_api", ""),
        ("opcache.file_update_protection", "2"),
        ("opcache.huge_code_pages", "0"),
        ("opcache.lockfile_path", "/tmp"),
        ("opcache.opt_debug_level", "0"),
        ("opcache.file_cache", ""),
        ("opcache.file_cache_only", "0"),
        ("opcache.file_cache_consistency_checks", "1"),
        ("opcache.file_cache_read_only", "0"),
        ("opcache.validate_permission", "0"),
        ("opcache.validate_root", "0"),
        ("opcache.preload", ""),
        ("opcache.preload_user", ""),
        ("opcache.jit", "disable"),
        ("opcache.jit_buffer_size", "64M"),
        ("opcache.jit_debug", "0"),
        ("opcache.jit_bisect_limit", "0"),
        ("opcache.jit_prof_threshold", "0.005"),
        ("opcache.jit_max_root_traces", "1024"),
        ("opcache.jit_max_side_traces", "128"),
        ("opcache.jit_max_exit_counters", "8192"),
        ("opcache.jit_hot_loop", "64"),
        ("opcache.jit_hot_func", "127"),
        ("opcache.jit_hot_return", "8"),
        ("opcache.jit_hot_side_exit", "8"),
        ("opcache.jit_blacklist_root_trace", "16"),
        ("opcache.jit_blacklist_side_trace", "8"),
        ("opcache.jit_max_loop_unrolls", "8"),
        ("opcache.jit_max_recursive_calls", "2"),
        ("opcache.jit_max_recursive_returns", "2"),
        ("opcache.jit_max_polymorphic_calls", "2"),
        ("opcache.jit_max_trace_length", "1024"),
    ],
    listed: true,
    ..Module::EMPTY
};

/// Whether the cache works for this request (opcache.enable, and
/// opcache.enable_cli in the CLI).
fn enabled(vm: &Vm) -> bool {
    let on = |k: &[u8]| {
        vm.get_ini(k).is_some_and(|v| {
            let l = v.to_ascii_lowercase();
            !matches!(&l[..], b"" | b"0" | b"off" | b"no" | b"false" | b"none")
        })
    };
    on(b"opcache.enable") && on(b"opcache.enable_cli")
}

fn opcache_compile_file(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.str(0);
    let p = vm.fs_path(&f);
    match std::fs::read(&p) {
        Ok(src) => match vm.compile_source(&src, f.as_bytes(), false) {
            Ok(_) => Ok(Value::Bool(true)),
            Err(e) => Err(vm.compile_error(e, f.as_bytes(), false)),
        },
        Err(_) => {
            vm.warn(&format!(
                "opcache_compile_file(): Failed opening required '{}'",
                f.to_string_lossy()
            ))?;
            Ok(Value::Bool(false))
        }
    }
}

fn opcache_get_configuration(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let mut directives = PhpArray::new();
    for (k, _) in MODULE.ini {
        let v = vm.get_ini(k.as_bytes()).unwrap_or_default();
        directives.insert(Key::from(*k), Value::Str(v));
    }
    let mut version = PhpArray::new();
    let v = vm
        .constants
        .get(&PhpStr::from("PHP_VERSION"))
        .cloned()
        .unwrap_or_default();
    version.insert(Key::from("version"), v);
    version.insert(
        Key::from("opcache_product_name"),
        Value::Str(PhpStr::from("Zend OPcache")),
    );
    let mut out = PhpArray::new();
    out.insert(Key::from("directives"), Value::Array(directives));
    out.insert(Key::from("version"), Value::Array(version));
    out.insert(Key::from("blacklist"), Value::Array(PhpArray::new()));
    Ok(Value::Array(out))
}

fn opcache_get_status(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !enabled(vm) {
        return Ok(Value::Bool(false));
    }
    let with_scripts = a.bool_or(0, true);
    let int = |n: i64| Value::Int(n);
    let mut st = PhpArray::new();
    st.insert(Key::from("opcache_enabled"), Value::Bool(true));
    st.insert(Key::from("cache_full"), Value::Bool(false));
    st.insert(Key::from("restart_pending"), Value::Bool(false));
    st.insert(Key::from("restart_in_progress"), Value::Bool(false));
    let mut mem = PhpArray::new();
    mem.insert(Key::from("used_memory"), int(0));
    mem.insert(Key::from("free_memory"), int(128 << 20));
    mem.insert(Key::from("wasted_memory"), int(0));
    mem.insert(Key::from("current_wasted_percentage"), Value::Float(0.0));
    st.insert(Key::from("memory_usage"), Value::Array(mem));
    let scripts: Vec<PhpStr> = vm.included.clone();
    let mut stats = PhpArray::new();
    stats.insert(Key::from("num_cached_scripts"), int(scripts.len() as i64));
    stats.insert(Key::from("num_cached_keys"), int(scripts.len() as i64));
    stats.insert(Key::from("max_cached_keys"), int(16229));
    stats.insert(Key::from("hits"), int(0));
    stats.insert(Key::from("start_time"), int(0));
    stats.insert(Key::from("last_restart_time"), int(0));
    stats.insert(Key::from("oom_restarts"), int(0));
    stats.insert(Key::from("hash_restarts"), int(0));
    stats.insert(Key::from("manual_restarts"), int(0));
    stats.insert(Key::from("misses"), int(scripts.len() as i64));
    stats.insert(Key::from("blacklist_misses"), int(0));
    stats.insert(Key::from("blacklist_miss_ratio"), Value::Float(0.0));
    stats.insert(Key::from("opcache_hit_rate"), Value::Float(0.0));
    st.insert(Key::from("opcache_statistics"), Value::Array(stats));
    let mut jit = PhpArray::new();
    jit.insert(Key::from("enabled"), Value::Bool(false));
    jit.insert(Key::from("on"), Value::Bool(false));
    jit.insert(Key::from("kind"), int(0));
    jit.insert(Key::from("opt_level"), int(0));
    jit.insert(Key::from("opt_flags"), int(0));
    jit.insert(Key::from("buffer_size"), int(0));
    jit.insert(Key::from("buffer_free"), int(0));
    st.insert(Key::from("jit"), Value::Array(jit));
    if with_scripts {
        let mut list = PhpArray::new();
        for s in scripts {
            let mut e = PhpArray::new();
            e.insert(Key::from("full_path"), Value::Str(s.clone()));
            e.insert(Key::from("hits"), int(0));
            e.insert(Key::from("memory_consumption"), int(0));
            list.insert(Key::Str(s), Value::Array(e));
        }
        st.insert(Key::from("scripts"), Value::Array(list));
    }
    Ok(Value::Array(st))
}

fn opcache_invalidate(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.str(0);
    Ok(Value::Bool(enabled(vm) && vm.fs_path(&f).exists()))
}

fn opcache_is_script_cached(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !enabled(vm) {
        return Ok(Value::Bool(false));
    }
    let f = a.str(0);
    let p = vm.fs_path(&f);
    let full = std::fs::canonicalize(&p).unwrap_or(p);
    use std::os::unix::ffi::OsStrExt;
    let key = PhpStr::new(full.as_os_str().as_bytes());
    Ok(Value::Bool(vm.included.contains(&key)))
}

fn opcache_reset(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Bool(enabled(vm)))
}

fn opcache_jit_blacklist(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Null)
}
