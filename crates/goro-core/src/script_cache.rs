//! The in-memory script cache of ext/opcache (ZendAccelerator.c's
//! persistent scripts): compiled files keyed by their resolved path, reused
//! by include/require while their timestamp is unchanged. The opcache
//! extension configures it (`CONFIG`) and implements the opcache_*() API
//! over it; the VM consults it when it compiles a plain file.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::object::ClassEntry;
use crate::opcode::OpArray;
use crate::value::Value;

/// A diagnostic raised while compiling a script, replayed on a cache hit
/// when opcache.record_warnings is on (zend_emit_recorded_errors()).
#[derive(Clone, Debug)]
pub enum Diag {
    /// A scanner diagnostic: (is deprecation, message, line).
    Lex(bool, String, u32),
    /// A compiler diagnostic (emit_compile_diagnostic()).
    Compile(String, u32),
}

/// The compiled form of a script, as include/require executes it.
pub struct Compiled {
    pub op_array: OpArray,
    pub classes: Vec<ClassEntry>,
    pub diags: Vec<Diag>,
}

/// zend_persistent_script
pub struct Script {
    pub full_path: String,
    /// None for the main script, whose compilation the SAPI does itself:
    /// it is recorded so that it reports as cached.
    pub compiled: Option<Rc<Compiled>>,
    /// The file's mtime when it was compiled.
    pub timestamp: i64,
    pub hits: i64,
    pub memory_consumption: i64,
    pub last_used: i64,
    /// dynamic_members.revalidate
    pub revalidate: i64,
    /// Set by opcache_invalidate(): the entry is kept (and counted) but
    /// no longer used.
    pub corrupted: bool,
}

/// The opcache directives the cache depends on.
#[derive(Clone, Debug)]
pub struct Config {
    /// ZCG(accelerator_enabled): the cache is in use.
    pub enabled: bool,
    pub validate_timestamps: bool,
    pub revalidate_freq: i64,
    pub file_update_protection: i64,
    pub record_warnings: bool,
    pub max_file_size: i64,
    /// opcache.log_verbosity_level
    pub log_verbosity: i64,
    /// opcache.error_log (empty: stderr)
    pub error_log: String,
    /// The opcache.blacklist_filename entries.
    pub blacklist: Vec<String>,
    /// opcache.revalidate_path: scripts are keyed by their real path (a
    /// symlink swapped later points to another script).
    pub revalidate_path: bool,
    /// opcache.file_cache (empty: no file cache).
    pub file_cache: String,
    /// opcache.file_cache_read_only
    pub file_cache_read_only: bool,
    /// The opcache system id (the file cache's subdirectory).
    pub system_id: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            enabled: false,
            validate_timestamps: true,
            revalidate_freq: 2,
            file_update_protection: 2,
            record_warnings: false,
            max_file_size: 0,
            log_verbosity: 1,
            error_log: String::new(),
            blacklist: Vec::new(),
            revalidate_path: false,
            file_cache: String::new(),
            file_cache_read_only: false,
            system_id: String::new(),
        }
    }
}

/// Cache statistics (ZCSG(hits), ZCSG(misses), ...).
#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub hits: i64,
    pub misses: i64,
    pub blacklist_misses: i64,
    pub manual_restarts: i64,
    pub start_time: i64,
    pub last_restart_time: i64,
    pub restart_pending: bool,
}

thread_local! {
    pub static CONFIG: RefCell<Config> = RefCell::new(Config::default());
    pub static STATS: RefCell<Stats> = RefCell::new(Stats::default());
    static SCRIPTS: RefCell<HashMap<String, Script>> = RefCell::new(HashMap::new());
    /// Insertion order of SCRIPTS (the hash table's iteration order).
    static ORDER: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

pub const LOG_FATAL: i64 = 0;
pub const LOG_ERROR: i64 = 1;
pub const LOG_WARNING: i64 = 2;
pub const LOG_INFO: i64 = 3;
pub const LOG_DEBUG: i64 = 4;

/// Whether a message of `level` is logged (opcache.log_verbosity_level).
pub fn log_enabled(level: i64) -> bool {
    CONFIG.with(|c| level <= c.borrow().log_verbosity)
}

/// zend_accel_error(): "<time> (<pid>): <Level> <message>" to the opcache
/// log (stderr unless opcache.error_log names a file).
pub fn log(level: i64, msg: &str) {
    let (verbosity, error_log) = CONFIG.with(|c| {
        let c = c.borrow();
        (c.log_verbosity, c.error_log.clone())
    });
    if level > verbosity {
        return;
    }
    let time = unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        let mut buf = [0 as libc::c_char; 64];
        let p = libc::asctime_r(&tm, buf.as_mut_ptr());
        if p.is_null() {
            String::new()
        } else {
            let s = std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned();
            s.trim_end_matches('\n').to_string()
        }
    };
    let kind = match level {
        LOG_FATAL => "Fatal Error ",
        LOG_ERROR => "Error ",
        LOG_WARNING => "Warning ",
        LOG_INFO => "Message ",
        _ => "Debug ",
    };
    let line = format!("{} ({}): {}{}\n", time, std::process::id(), kind, msg);
    if !error_log.is_empty() && error_log != "stderr" {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&error_log)
        {
            let _ = f.write_all(line.as_bytes());
            return;
        }
    }
    unsafe {
        libc::write(2, line.as_ptr() as *const libc::c_void, line.len());
    }
}

pub fn enabled() -> bool {
    CONFIG.with(|c| c.borrow().enabled)
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn file_mtime(path: &str) -> Option<i64> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|m| m.mtime())
}

/// validate_timestamp_and_record(): whether a cached script is still
/// valid for `path`.
fn still_valid(script: &mut Script, cfg: &Config) -> bool {
    if !cfg.validate_timestamps {
        return true;
    }
    let now = now();
    if cfg.revalidate_freq > 0 && script.revalidate >= now {
        return true;
    }
    match file_mtime(&script.full_path) {
        Some(t) if t == script.timestamp => {
            script.revalidate = now + cfg.revalidate_freq;
            true
        }
        _ => false,
    }
}

/// Looks `path` up for include/require: a cache hit returns the compiled
/// script (and counts the hit); a miss is counted by store_compiled().
pub fn lookup(path: &str) -> Option<Rc<Compiled>> {
    let cfg = CONFIG.with(|c| c.borrow().clone());
    if !cfg.enabled {
        return None;
    }
    let key = cache_key(path, &cfg);
    let path = key.as_str();
    let hit = SCRIPTS.with(|s| {
        let mut s = s.borrow_mut();
        let script = s.get_mut(path)?;
        if script.corrupted {
            return None;
        }
        let compiled = script.compiled.clone()?;
        if !still_valid(script, &cfg) {
            return None;
        }
        script.hits += 1;
        script.last_used = now();
        Some(compiled)
    });
    if hit.is_some() {
        STATS.with(|s| s.borrow_mut().hits += 1);
    }
    hit
}

/// Whether `path` would be cached at all: file_update_protection keeps
/// files modified in the last seconds out of the cache.
fn cacheable(path: &str, cfg: &Config) -> Option<i64> {
    let meta = std::fs::metadata(path).ok()?;
    use std::os::unix::fs::MetadataExt;
    let mtime = meta.mtime();
    if cfg.max_file_size > 0 && meta.len() as i64 > cfg.max_file_size {
        return None;
    }
    if cfg.file_update_protection > 0 && now() - mtime < cfg.file_update_protection {
        return None;
    }
    Some(mtime)
}

fn insert(path: &str, compiled: Option<Rc<Compiled>>, timestamp: i64, size: i64) {
    let cfg = CONFIG.with(|c| c.borrow().clone());
    let script = Script {
        full_path: path.to_string(),
        compiled,
        timestamp,
        hits: 0,
        memory_consumption: size,
        last_used: now(),
        revalidate: now() + cfg.revalidate_freq,
        corrupted: false,
    };
    let fresh = SCRIPTS.with(|s| s.borrow_mut().insert(path.to_string(), script).is_none());
    if fresh {
        ORDER.with(|o| o.borrow_mut().push(path.to_string()));
    }
}

/// A blacklist entry matched against the start of a path
/// (zend_accel_blacklist_update_regexp(): `?` is `[^/]`, `*` is `[^/]*`
/// and `**` is `.*`, anchored at the start only).
fn blacklist_match(pat: &[u8], path: &[u8]) -> bool {
    match pat.first() {
        None => true,
        Some(b'?') => {
            path.first().is_some_and(|&c| c != b'/') && blacklist_match(&pat[1..], &path[1..])
        }
        Some(b'*') => {
            let (rest, any) = if pat.get(1) == Some(&b'*') {
                (&pat[2..], true)
            } else {
                (&pat[1..], false)
            };
            let mut i = 0;
            loop {
                if blacklist_match(rest, &path[i..]) {
                    return true;
                }
                if i >= path.len() || (!any && path[i] == b'/') {
                    return false;
                }
                i += 1;
            }
        }
        Some(&c) => path.first() == Some(&c) && blacklist_match(&pat[1..], &path[1..]),
    }
}

/// zend_accel_blacklist_is_blacklisted()
pub fn is_blacklisted(path: &str) -> bool {
    CONFIG.with(|c| {
        c.borrow()
            .blacklist
            .iter()
            .any(|p| blacklist_match(p.as_bytes(), path.as_bytes()))
    })
}

/// The file cache entry of a script (zend_file_cache_get_bin_file_path()):
/// `<file_cache>/<system id><full path>.bin`.
pub fn file_cache_bin_path(path: &str) -> Option<String> {
    CONFIG.with(|c| {
        let c = c.borrow();
        if c.file_cache.is_empty() || !path.starts_with('/') {
            return None;
        }
        Some(format!("{}/{}{}.bin", c.file_cache, c.system_id, path))
    })
}

/// zend_file_cache_script_store(): goro keeps no serialized opcodes, the
/// entry records the script's timestamp and size (the script is compiled
/// again when it is loaded).
pub fn file_cache_store(path: &str) {
    let read_only = CONFIG.with(|c| c.borrow().file_cache_read_only);
    let Some(bin) = file_cache_bin_path(path) else {
        return;
    };
    if read_only {
        return;
    }
    use std::os::unix::fs::MetadataExt;
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    if let Some(dir) = std::path::Path::new(&bin).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let data = format!("OPCACHE\0{}\0{}\0", meta.mtime(), meta.len());
    if let Err(e) = std::fs::write(&bin, data) {
        log(
            LOG_WARNING,
            &format!("opcache cannot write to file '{}': {}\n", bin, e),
        );
    }
}

/// Whether the file cache holds an entry for `path` that is still valid.
pub fn in_file_cache(path: &str) -> bool {
    let Some(bin) = file_cache_bin_path(path) else {
        return false;
    };
    let Ok(data) = std::fs::read(&bin) else {
        return false;
    };
    let validate = CONFIG.with(|c| c.borrow().validate_timestamps);
    if !validate {
        return data.starts_with(b"OPCACHE\0");
    }
    use std::os::unix::fs::MetadataExt;
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    data == format!("OPCACHE\0{}\0{}\0", meta.mtime(), meta.len()).into_bytes()
}

/// The key of an included file: the path as resolved by the include
/// (without following symlinks), or its real path with
/// opcache.revalidate_path.
fn cache_key(path: &str, cfg: &Config) -> String {
    if cfg.revalidate_path {
        if let Ok(p) = std::fs::canonicalize(path) {
            return p.to_string_lossy().into_owned();
        }
    }
    path.to_string()
}

/// Records a freshly compiled script (a cache miss).
pub fn store_compiled(path: &str, compiled: Compiled) {
    let cfg = CONFIG.with(|c| c.borrow().clone());
    if !cfg.enabled {
        return;
    }
    let key = cache_key(path, &cfg);
    let path = key.as_str();
    STATS.with(|s| s.borrow_mut().misses += 1);
    if is_blacklisted(path) {
        STATS.with(|s| s.borrow_mut().blacklist_misses += 1);
        return;
    }
    let Some(mtime) = cacheable(path, &cfg) else {
        return;
    };
    let size = estimate_size(&compiled.op_array);
    insert(path, Some(Rc::new(compiled)), mtime, size);
    log(LOG_INFO, &format!("Cached script '{}'", path));
}

/// Records the main script (compiled by the SAPI).
pub fn store_main(path: &str) {
    let cfg = CONFIG.with(|c| c.borrow().clone());
    if !cfg.enabled {
        return;
    }
    STATS.with(|s| s.borrow_mut().misses += 1);
    if is_blacklisted(path) {
        STATS.with(|s| s.borrow_mut().blacklist_misses += 1);
        return;
    }
    let Some(mtime) = cacheable(path, &cfg) else {
        return;
    };
    let size = std::fs::metadata(path).map(|m| m.len() as i64).unwrap_or(0) * 8 + 1024;
    insert(path, None, mtime, size);
    log(LOG_INFO, &format!("Cached script '{}'", path));
}

fn estimate_size(op: &OpArray) -> i64 {
    let mut n = 512 + op.ops.len() as i64 * 32 + op.literals.len() as i64 * 16;
    for c in &op.child_functions {
        n += estimate_size(c);
    }
    n
}

/// filename_is_in_cache()
pub fn is_cached(path: &str) -> bool {
    let cfg = CONFIG.with(|c| c.borrow().clone());
    SCRIPTS.with(|s| {
        let mut s = s.borrow_mut();
        match s.get_mut(path) {
            Some(script) if !script.corrupted => still_valid(script, &cfg),
            _ => false,
        }
    })
}

/// zend_accel_invalidate(): discards the script when forced, when
/// timestamps are not validated, or when its file changed (or
/// disappeared). Returns whether the script was cached.
pub fn invalidate(path: &str, force: bool) -> bool {
    let validate = CONFIG.with(|c| c.borrow().validate_timestamps);
    SCRIPTS.with(|s| {
        let mut s = s.borrow_mut();
        let Some(script) = s.get_mut(path) else {
            return false;
        };
        if script.corrupted {
            return false;
        }
        if force || !validate || file_mtime(path) != Some(script.timestamp) {
            script.corrupted = true;
        }
        true
    })
}

/// opcache_reset(): the cache is emptied (at the next request in PHP; the
/// CLI has a single request, so the scripts are dropped for the rest of it).
pub fn reset() {
    SCRIPTS.with(|s| s.borrow_mut().clear());
    ORDER.with(|o| o.borrow_mut().clear());
    STATS.with(|s| {
        let mut s = s.borrow_mut();
        s.manual_restarts += 1;
        s.restart_pending = true;
    });
}

/// The cached scripts, in insertion order:
/// (full_path, hits, memory_consumption, last_used, timestamp, revalidate).
pub fn scripts() -> Vec<(String, i64, i64, i64, i64, i64)> {
    let order = ORDER.with(|o| o.borrow().clone());
    SCRIPTS.with(|s| {
        let s = s.borrow();
        order
            .iter()
            .filter_map(|p| s.get(p))
            .map(|sc| {
                (
                    sc.full_path.clone(),
                    sc.hits,
                    sc.memory_consumption,
                    sc.last_used,
                    sc.timestamp,
                    sc.revalidate,
                )
            })
            .collect()
    })
}

pub fn num_scripts() -> usize {
    SCRIPTS.with(|s| s.borrow().len())
}

pub fn used_memory() -> i64 {
    SCRIPTS.with(|s| s.borrow().values().map(|v| v.memory_consumption).sum())
}

impl crate::vm::Vm {
    /// Records the compilation of an included file in the cache.
    #[inline(never)]
    pub(crate) fn cache_compiled_script(
        &mut self,
        path: &str,
        op_array: &OpArray,
        classes: &[ClassEntry],
        lex_diags: &[(bool, String, u32)],
        warnings: &[(String, u32)],
    ) {
        if crate::preload::active() {
            crate::preload::note_compiled(path, op_array, classes);
            return;
        }
        file_cache_store(path);
        if !enabled() {
            return;
        }
        if log_enabled(LOG_INFO) {
            self.flush_cli_output();
        }
        let record = CONFIG.with(|c| c.borrow().record_warnings);
        let mut diags = Vec::new();
        if record {
            for (d, m, l) in lex_diags {
                diags.push(Diag::Lex(*d, m.clone(), *l));
            }
            for (m, l) in warnings {
                diags.push(Diag::Compile(m.clone(), *l));
            }
        }
        store_compiled(
            path,
            Compiled {
                op_array: op_array.clone(),
                classes: classes.to_vec(),
                diags,
            },
        );
    }

    /// zend_emit_recorded_errors(): the diagnostics of a cached script's
    /// compilation (recorded with opcache.record_warnings).
    #[inline(never)]
    pub(crate) fn replay_cached_diags(&mut self, diags: &[Diag]) {
        for d in diags {
            match d {
                Diag::Lex(true, m, l) => self.emit_deprecated_at(m, *l),
                Diag::Lex(false, m, l) => self.emit_warning_at(m, *l),
                Diag::Compile(m, l) => self.emit_compile_diagnostic(m, *l),
            }
        }
    }
}

/// The outcome of compiling a file without executing it.
pub enum CompileOnly {
    Compiled,
    /// The file could not be opened (the error message of the open).
    OpenFailed(String),
    /// A ParseError was thrown (set as the VM's current exception).
    ParseError,
    /// A compile error was reported.
    CompileError,
}

impl crate::vm::Vm {
    /// Compiles `path` (a resolved path) without executing it
    /// (ZEND_COMPILE_WITHOUT_EXECUTION), recording it in the cache:
    /// opcache_compile_file() and preloading.
    #[inline(never)]
    pub fn compile_file_only(&mut self, path: &str) -> CompileOnly {
        let source = match std::fs::read(path) {
            Ok(s) => s,
            Err(e) => {
                let s = e.to_string();
                let s = match s.find(" (os error") {
                    Some(i) => s[..i].to_string(),
                    None => s,
                };
                return CompileOnly::OpenFailed(s);
            }
        };
        crate::value::alloc_resource_id();
        let script = goro_parser::multibyte::lex_script(&source, true);
        let mut parser = goro_parser::Parser::new(script.tokens).with_source(&script.source);
        let parsed = parser.parse();
        let lex_diags = goro_parser::order_lexer_diagnostics(
            &goro_parser::take_lexer_deprecations(),
            &goro_parser::take_lexer_warnings(),
        );
        let program = match parsed {
            Ok(p) => p,
            Err(e) => {
                let msg = if crate::vm::is_php_parse_message(&e.message) {
                    e.message.clone()
                } else {
                    "syntax error".to_string()
                };
                let exc = self.create_exception(b"ParseError", &msg, e.span.line);
                if let Value::Object(ref obj) = exc {
                    let mut ob = obj.borrow_mut();
                    ob.set_property(
                        b"file".to_vec(),
                        Value::String(crate::string::PhpString::from_string(path.to_string())),
                    );
                    ob.set_property(b"line".to_vec(), Value::Long(e.span.line as i64));
                }
                self.current_exception = Some(exc);
                return CompileOnly::ParseError;
            }
        };
        let mut compiler = crate::compiler::Compiler::new();
        compiler.source_file = path.as_bytes().to_vec();
        let saved_file = std::mem::replace(&mut self.current_file, path.into());
        let r = match compiler.compile(&program) {
            Ok((mut op_array, classes, warnings)) => {
                let implicit_ret = op_array
                    .ops
                    .last()
                    .is_some_and(|l| l.opcode == crate::opcode::OpCode::Return && l.line == 0);
                if implicit_ret {
                    let one = op_array.add_literal(Value::Long(1));
                    if let Some(l) = op_array.ops.last_mut() {
                        l.op1 = crate::opcode::OperandType::Const(one);
                    }
                }
                for (deprecation, msg, wline) in &lex_diags {
                    if *deprecation {
                        self.emit_deprecated_at(msg, *wline);
                    } else {
                        self.emit_warning_at(msg, *wline);
                    }
                }
                for (msg, wline) in &warnings {
                    self.emit_compile_diagnostic(msg, *wline);
                }
                if crate::preload::active() {
                    match self.preload_compiled_only(path, &program, &op_array, &classes) {
                        Ok(()) => CompileOnly::Compiled,
                        Err(_) => CompileOnly::CompileError,
                    }
                } else {
                    self.cache_compiled_script(path, &op_array, &classes, &lex_diags, &warnings);
                    CompileOnly::Compiled
                }
            }
            Err(e) => {
                for (kind, msg, wline) in crate::compiler::take_pending_compile_warnings() {
                    self.display_error(kind, &msg, path, wline);
                }
                self.display_error("Fatal error", &e.message, path, e.line);
                CompileOnly::CompileError
            }
        };
        self.current_file = saved_file;
        r
    }
}
