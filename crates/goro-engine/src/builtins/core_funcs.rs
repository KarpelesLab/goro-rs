//! Core functions (PHP manual: Function handling, Classes/Objects, Error
//! handling, Misc., PHP options/info).

use crate::class::TypeName;
use std::rc::Rc;

use goro_runtime::{Key, KeyRef, PhpArray, PhpStr, Value};

use crate::class::{Class, cflags, lower, mflags};
use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::errors::*;
use crate::vm::{R, Unwind, Vm};

pub static MODULE: Module = Module {
    name: "Core",
    functions: &[
        // function handling
        (
            "function call_user_func(callable $callback, mixed ...$args): mixed",
            call_user_func,
        ),
        (
            "function call_user_func_array(callable $callback, array $args): mixed",
            call_user_func_array,
        ),
        (
            "function forward_static_call(callable $callback, mixed ...$args): mixed",
            forward_static_call,
        ),
        (
            "function forward_static_call_array(callable $callback, array $args): mixed",
            forward_static_call_array,
        ),
        ("function func_get_args(): array", func_get_args),
        ("function func_get_arg(int $position): mixed", func_get_arg),
        ("function func_num_args(): int", func_num_args),
        (
            "function function_exists(string $function): bool",
            function_exists,
        ),
        (
            "function register_shutdown_function(callable $callback, mixed ...$args): void",
            register_shutdown_function,
        ),
        (
            "function get_defined_functions(bool $exclude_disabled = true): array",
            get_defined_functions,
        ),
        ("function get_defined_vars(): array", get_defined_vars),
        ("function compact($var_name, ...$var_names): array", compact),
        (
            "function extract(array &$array, int $flags = EXTR_OVERWRITE, string $prefix = \"\"): int",
            extract,
        ),
        // constants
        (
            "function define(string $constant_name, mixed $value, bool $case_insensitive = false): bool",
            define,
        ),
        ("function defined(string $constant_name): bool", defined),
        ("function constant(string $name): mixed", constant),
        (
            "function get_defined_constants(bool $categorize = false): array",
            get_defined_constants,
        ),
        // classes and objects
        (
            "function class_exists(string $class, bool $autoload = true): bool",
            class_exists,
        ),
        (
            "function interface_exists(string $interface, bool $autoload = true): bool",
            interface_exists,
        ),
        (
            "function trait_exists(string $trait, bool $autoload = true): bool",
            trait_exists,
        ),
        (
            "function enum_exists(string $enum, bool $autoload = true): bool",
            enum_exists,
        ),
        (
            "function get_class(object $object = UNKNOWN): string",
            get_class,
        ),
        (
            "function get_parent_class(object|string $object_or_class = UNKNOWN): string|false",
            get_parent_class,
        ),
        ("function get_called_class(): string", get_called_class),
        (
            "function method_exists($object_or_class, string $method): bool",
            method_exists,
        ),
        (
            "function property_exists($object_or_class, string $property): bool",
            property_exists,
        ),
        (
            "function get_object_vars(object $object): array",
            get_object_vars,
        ),
        (
            "function get_mangled_object_vars(object $object): array",
            get_mangled_object_vars,
        ),
        (
            "function get_class_vars(string $class): array",
            get_class_vars,
        ),
        (
            "function get_class_methods(object|string $object_or_class): array",
            get_class_methods,
        ),
        (
            "function is_a(mixed $object_or_class, string $class, bool $allow_string = false): bool",
            is_a,
        ),
        (
            "function is_subclass_of(mixed $object_or_class, string $class, bool $allow_string = true): bool",
            is_subclass_of,
        ),
        (
            "function class_implements($object_or_class, bool $autoload = true): array|false",
            class_implements,
        ),
        (
            "function class_parents($object_or_class, bool $autoload = true): array|false",
            class_parents,
        ),
        (
            "function class_uses($object_or_class, bool $autoload = true): array|false",
            class_uses,
        ),
        (
            "function class_alias(string $class, string $alias, bool $autoload = true): bool",
            class_alias,
        ),
        (
            "function get_declared_classes(): array",
            get_declared_classes,
        ),
        (
            "function get_declared_interfaces(): array",
            get_declared_interfaces,
        ),
        ("function get_declared_traits(): array", get_declared_traits),
        ("function spl_object_id(object $object): int", spl_object_id),
        (
            "function spl_object_hash(object $object): string",
            spl_object_hash,
        ),
        (
            "function iterator_to_array(Traversable|array $iterator, bool $preserve_keys = true): array",
            iterator_to_array,
        ),
        (
            "function iterator_count(Traversable|array $iterator): int",
            iterator_count,
        ),
        (
            "function iterator_apply(Traversable $iterator, callable $callback, ?array $args = null): int",
            iterator_apply,
        ),
        // errors
        (
            "function error_reporting(?int $error_level = null): int",
            error_reporting,
        ),
        (
            "function set_error_handler(?callable $callback, int $error_levels = E_ALL)",
            set_error_handler,
        ),
        (
            "function restore_error_handler(): true",
            restore_error_handler,
        ),
        (
            "function set_exception_handler(?callable $callback)",
            set_exception_handler,
        ),
        (
            "function restore_exception_handler(): true",
            restore_exception_handler,
        ),
        ("function get_error_handler(): ?callable", get_error_handler),
        (
            "function get_exception_handler(): ?callable",
            get_exception_handler,
        ),
        (
            "function trigger_error(string $message, int $error_level = E_USER_NOTICE): true",
            trigger_error,
        ),
        (
            "function user_error(string $message, int $error_level = E_USER_NOTICE): true",
            trigger_error,
        ),
        (
            "function error_log(string $message, int $message_type = 0, ?string $destination = null, ?string $additional_headers = null): bool",
            error_log,
        ),
        ("function error_get_last(): ?array", error_get_last),
        ("function error_clear_last(): void", error_clear_last),
        (
            "function debug_backtrace(int $options = DEBUG_BACKTRACE_PROVIDE_OBJECT, int $limit = 0): array",
            debug_backtrace,
        ),
        (
            "function debug_print_backtrace(int $options = 0, int $limit = 0): void",
            debug_print_backtrace,
        ),
        (
            "function assert(mixed $assertion, Throwable|string|null $description = null): bool",
            assert,
        ),
        // ini
        ("function ini_get(string $option): string|false", ini_get),
        (
            "function ini_set(string $option, string|int|float|bool|null $value): string|false",
            ini_set,
        ),
        (
            "function ini_alter(string $option, string|int|float|bool|null $value): string|false",
            ini_set,
        ),
        ("function ini_restore(string $option): void", ini_restore),
        (
            "function ini_get_all(?string $extension = null, bool $details = true): array|false",
            ini_get_all,
        ),
        (
            "function set_include_path(string $include_path): string|false",
            set_include_path,
        ),
        (
            "function get_include_path(): string|false",
            get_include_path,
        ),
        (
            "function get_cfg_var(string $option): string|array|false",
            get_cfg_var,
        ),
        (
            "function php_ini_loaded_file(): string|false",
            php_ini_loaded_file,
        ),
        (
            "function php_ini_scanned_files(): string|false",
            php_ini_loaded_file,
        ),
        // runtime
        ("function gc_collect_cycles(): int", gc_collect_cycles),
        ("function gc_enable(): void", gc_enable),
        ("function gc_disable(): void", gc_disable),
        ("function gc_enabled(): bool", gc_enabled),
        ("function gc_mem_caches(): int", gc_mem_caches),
        ("function gc_status(): array", gc_status),
        (
            "function memory_get_usage(bool $real_usage = false): int",
            memory_get_usage,
        ),
        (
            "function memory_get_peak_usage(bool $real_usage = false): int",
            memory_get_usage,
        ),
        ("function memory_reset_peak_usage(): void", noop_void),
        ("function php_sapi_name(): string|false", php_sapi_name),
        (
            "function phpversion(?string $extension = null): string|false",
            phpversion,
        ),
        ("function zend_version(): string", zend_version),
        (
            "function php_uname(string $mode = \"a\"): string",
            php_uname,
        ),
        (
            "function extension_loaded(string $extension): bool",
            extension_loaded,
        ),
        (
            "function get_loaded_extensions(bool $zend_extensions = false): array",
            get_loaded_extensions,
        ),
        (
            "function get_extension_funcs(string $extension): array|false",
            get_extension_funcs,
        ),
        ("function dl(string $extension_filename): bool", dl),
        ("function getmypid(): int|false", getmypid),
        ("function getmyuid(): int|false", getmyuid),
        ("function getmygid(): int|false", getmygid),
        (
            "function getenv(?string $name = null, bool $local_only = false): array|string|false",
            getenv,
        ),
        ("function putenv(string $assignment): bool", putenv),
        (
            "function set_time_limit(int $seconds): bool",
            set_time_limit,
        ),
        (
            "function ignore_user_abort(?bool $enable = null): int",
            ignore_user_abort,
        ),
        ("function connection_status(): int", zero_int),
        ("function connection_aborted(): int", zero_int),
        ("function sys_get_temp_dir(): string", sys_get_temp_dir),
        (
            "function microtime(bool $as_float = false): string|float",
            microtime,
        ),
        (
            "function hrtime(bool $as_number = false): array|int|float|false",
            hrtime,
        ),
        ("function usleep(int $microseconds): void", usleep),
        ("function sleep(int $seconds): int", sleep),
        (
            "function time_nanosleep(int $seconds, int $nanoseconds): array|bool",
            time_nanosleep,
        ),
        (
            "function uniqid(string $prefix = \"\", bool $more_entropy = false): string",
            uniqid,
        ),
        (
            "function headers_sent(&$filename = null, &$line = null): bool",
            headers_sent,
        ),
        (
            "function header(string $header, bool $replace = true, int $response_code = 0): void",
            noop_void,
        ),
        (
            "function header_remove(?string $name = null): void",
            noop_void,
        ),
        ("function headers_list(): array", empty_array),
        (
            "function http_response_code(int $response_code = 0): int|bool",
            http_response_code,
        ),
        (
            "function setlocale(int $category, $locales, ...$rest): string|false",
            setlocale,
        ),
        ("function get_include_files(): array", get_included_files),
        ("function get_included_files(): array", get_included_files),
        ("function get_required_files(): array", get_included_files),
        ("function gethostname(): string|false", gethostname),
        ("function zend_thread_id(): int", zero_int),
        ("function get_current_user(): string", get_current_user),
        ("function getlastmod(): int|false", getlastmod),
        ("function getmyinode(): int|false", getmyinode),
        (
            "function get_resources(?string $type = null): array",
            get_resources,
        ),
        (
            "function clone(object $object, array $withProperties = []): object",
            clone_fn,
        ),
    ],
    constants: &[
        ("PHP_EOL", ConstVal::Str("\n")),
        ("INF", ConstVal::Float(f64::INFINITY)),
        ("NAN", ConstVal::Float(f64::NAN)),
        ("PHP_INT_MAX", ConstVal::Int(i64::MAX)),
        ("PHP_INT_MIN", ConstVal::Int(i64::MIN)),
        ("PHP_INT_SIZE", ConstVal::Int(8)),
        ("PHP_FLOAT_EPSILON", ConstVal::Float(f64::EPSILON)),
        ("PHP_FLOAT_MAX", ConstVal::Float(f64::MAX)),
        ("PHP_FLOAT_MIN", ConstVal::Float(f64::MIN_POSITIVE)),
        ("PHP_FLOAT_DIG", ConstVal::Int(15)),
        ("PHP_VERSION", ConstVal::Str("8.5.4")),
        ("PHP_MAJOR_VERSION", ConstVal::Int(8)),
        ("PHP_MINOR_VERSION", ConstVal::Int(5)),
        ("PHP_RELEASE_VERSION", ConstVal::Int(4)),
        ("PHP_EXTRA_VERSION", ConstVal::Str("")),
        ("PHP_VERSION_ID", ConstVal::Int(80504)),
        ("PHP_OS", ConstVal::Str("Linux")),
        ("PHP_OS_FAMILY", ConstVal::Str("Linux")),
        ("PHP_DEBUG", ConstVal::Int(0)),
        ("PHP_ZTS", ConstVal::Int(0)),
        ("PHP_MAXPATHLEN", ConstVal::Int(4096)),
        ("PHP_SHLIB_SUFFIX", ConstVal::Str("so")),
        ("PHP_FD_SETSIZE", ConstVal::Int(1024)),
        ("DEFAULT_INCLUDE_PATH", ConstVal::Str(".:/usr/share/php")),
        ("PEAR_INSTALL_DIR", ConstVal::Str("/usr/share/php")),
        ("PEAR_EXTENSION_DIR", ConstVal::Str("/usr/lib/php/20240924")),
        ("PHP_EXTENSION_DIR", ConstVal::Str("/usr/lib/php/20240924")),
        ("PHP_PREFIX", ConstVal::Str("/usr")),
        ("PHP_BINDIR", ConstVal::Str("/usr/bin")),
        ("PHP_LIBDIR", ConstVal::Str("/usr/lib/php")),
        ("PHP_DATADIR", ConstVal::Str("/usr/share/php")),
        ("PHP_SYSCONFDIR", ConstVal::Str("/etc")),
        ("PHP_LOCALSTATEDIR", ConstVal::Str("/var")),
        ("PHP_CONFIG_FILE_PATH", ConstVal::Str("/etc/php")),
        ("PHP_CONFIG_FILE_SCAN_DIR", ConstVal::Str("")),
        ("PHP_MANDIR", ConstVal::Str("/usr/share/man")),
        ("PHP_SAPI", ConstVal::Str("cli")),
        ("PHP_OUTPUT_HANDLER_START", ConstVal::Int(1)),
        ("PHP_OUTPUT_HANDLER_WRITE", ConstVal::Int(0)),
        ("PHP_OUTPUT_HANDLER_FLUSH", ConstVal::Int(4)),
        ("PHP_OUTPUT_HANDLER_CLEAN", ConstVal::Int(2)),
        ("PHP_OUTPUT_HANDLER_FINAL", ConstVal::Int(8)),
        ("PHP_OUTPUT_HANDLER_CONT", ConstVal::Int(0)),
        ("PHP_OUTPUT_HANDLER_END", ConstVal::Int(8)),
        ("PHP_OUTPUT_HANDLER_CLEANABLE", ConstVal::Int(16)),
        ("PHP_OUTPUT_HANDLER_FLUSHABLE", ConstVal::Int(32)),
        ("PHP_OUTPUT_HANDLER_REMOVABLE", ConstVal::Int(64)),
        ("PHP_OUTPUT_HANDLER_STDFLAGS", ConstVal::Int(112)),
        ("PHP_OUTPUT_HANDLER_STARTED", ConstVal::Int(4096)),
        ("PHP_OUTPUT_HANDLER_DISABLED", ConstVal::Int(8192)),
        ("PHP_OUTPUT_HANDLER_PROCESSED", ConstVal::Int(16384)),
        ("E_ERROR", ConstVal::Int(E_ERROR)),
        ("E_WARNING", ConstVal::Int(E_WARNING)),
        ("E_PARSE", ConstVal::Int(E_PARSE)),
        ("E_NOTICE", ConstVal::Int(E_NOTICE)),
        ("E_CORE_ERROR", ConstVal::Int(E_CORE_ERROR)),
        ("E_CORE_WARNING", ConstVal::Int(E_CORE_WARNING)),
        ("E_COMPILE_ERROR", ConstVal::Int(E_COMPILE_ERROR)),
        ("E_COMPILE_WARNING", ConstVal::Int(E_COMPILE_WARNING)),
        ("E_USER_ERROR", ConstVal::Int(E_USER_ERROR)),
        ("E_USER_WARNING", ConstVal::Int(E_USER_WARNING)),
        ("E_USER_NOTICE", ConstVal::Int(E_USER_NOTICE)),
        ("E_STRICT", ConstVal::Int(E_STRICT)),
        ("E_RECOVERABLE_ERROR", ConstVal::Int(E_RECOVERABLE_ERROR)),
        ("E_DEPRECATED", ConstVal::Int(E_DEPRECATED)),
        ("E_USER_DEPRECATED", ConstVal::Int(E_USER_DEPRECATED)),
        ("E_ALL", ConstVal::Int(E_ALL)),
        ("DEBUG_BACKTRACE_PROVIDE_OBJECT", ConstVal::Int(1)),
        ("DEBUG_BACKTRACE_IGNORE_ARGS", ConstVal::Int(2)),
        ("EXTR_OVERWRITE", ConstVal::Int(0)),
        ("EXTR_SKIP", ConstVal::Int(1)),
        ("EXTR_PREFIX_SAME", ConstVal::Int(2)),
        ("EXTR_PREFIX_ALL", ConstVal::Int(3)),
        ("EXTR_PREFIX_INVALID", ConstVal::Int(4)),
        ("EXTR_PREFIX_IF_EXISTS", ConstVal::Int(5)),
        ("EXTR_IF_EXISTS", ConstVal::Int(6)),
        ("EXTR_REFS", ConstVal::Int(256)),
        ("UPLOAD_ERR_OK", ConstVal::Int(0)),
        ("ZEND_THREAD_SAFE", ConstVal::Bool(false)),
        ("ZEND_DEBUG_BUILD", ConstVal::Bool(false)),
        ("PHP_WINDOWS_VERSION_MAJOR_UNUSED", ConstVal::Null),
        ("LC_CTYPE", ConstVal::Int(0)),
        ("LC_NUMERIC", ConstVal::Int(1)),
        ("LC_TIME", ConstVal::Int(2)),
        ("LC_COLLATE", ConstVal::Int(3)),
        ("LC_MONETARY", ConstVal::Int(4)),
        ("LC_MESSAGES", ConstVal::Int(5)),
        ("LC_ALL", ConstVal::Int(6)),
    ],
    ini: &[
        ("error_reporting", ""),
        ("display_errors", "1"),
        ("display_startup_errors", "1"),
        ("log_errors", "0"),
        ("html_errors", "0"),
        ("precision", "14"),
        ("serialize_precision", "-1"),
        ("zend.assertions", "1"),
        ("assert.active", "1"),
        ("assert.exception", "1"),
        ("assert.warning", "1"),
        ("assert.bail", "0"),
        ("assert.callback", ""),
        ("memory_limit", "128M"),
        ("include_path", ".:/usr/share/php"),
        ("ignore_user_abort", "0"),
        ("max_execution_time", "0"),
        ("output_buffering", "0"),
        ("output_handler", ""),
        ("implicit_flush", "1"),
        ("short_open_tag", "1"),
        ("error_log", ""),
        ("error_prepend_string", ""),
        ("error_append_string", ""),
        ("docref_root", ""),
        ("docref_ext", ""),
        ("ignore_repeated_errors", "0"),
        ("ignore_repeated_source", "0"),
        ("report_memleaks", "1"),
        ("track_errors", "0"),
        ("xmlrpc_errors", "0"),
        ("zend.exception_ignore_args", "0"),
        ("zend.exception_string_param_max_len", "15"),
        ("zend.enable_gc", "1"),
        ("arg_separator.output", "&"),
        ("arg_separator.input", "&"),
        ("auto_detect_line_endings", "0"),
        ("default_charset", "UTF-8"),
        ("input_encoding", ""),
        ("output_encoding", ""),
        ("internal_encoding", ""),
        ("variables_order", "EGPCS"),
        ("request_order", ""),
        ("register_argc_argv", "1"),
        ("auto_globals_jit", "1"),
        ("open_basedir", ""),
        ("disable_functions", ""),
        ("disable_classes", ""),
        ("auto_prepend_file", ""),
        ("auto_append_file", ""),
        ("user_dir", ""),
        ("extension_dir", "/usr/lib/php/20240924"),
        ("sys_temp_dir", ""),
        ("enable_dl", "1"),
        ("file_uploads", "1"),
        ("upload_max_filesize", "2M"),
        ("post_max_size", "8M"),
        ("max_input_time", "-1"),
        ("max_input_vars", "1000"),
        ("max_input_nesting_level", "64"),
        ("default_socket_timeout", "60"),
        ("allow_url_fopen", "1"),
        ("allow_url_include", "0"),
        ("user_agent", ""),
        ("realpath_cache_size", "4096K"),
        ("realpath_cache_ttl", "120"),
        ("expose_php", "1"),
        ("hard_timeout", "2"),
        ("zend.multibyte", "0"),
        ("zend.script_encoding", ""),
        ("zend.detect_unicode", "1"),
        ("zend.signal_check", "0"),
        ("zend.max_allowed_stack_size", "0"),
        ("zend.reserved_stack_size", "0"),
        ("fatal_error_backtraces", "1"),
        ("unserialize_callback_func", ""),
        ("unserialize_max_depth", "4096"),
        ("max_file_uploads", "20"),
        ("mail.add_x_header", "0"),
        ("sendmail_path", "/usr/sbin/sendmail -t -i"),
        ("SMTP", "localhost"),
        ("smtp_port", "25"),
        ("date.timezone", "UTC"),
        ("session.save_path", ""),
        ("highlight.comment", "#FF8000"),
        ("highlight.default", "#0000BB"),
        ("highlight.html", "#000000"),
        ("highlight.keyword", "#007700"),
        ("highlight.string", "#DD0000"),
        ("syslog.facility", "LOG_USER"),
        ("syslog.ident", "php"),
        ("syslog.filter", "no-ctrl"),
        ("error_log_mode", "0644"),
        ("cli.pager", ""),
        ("cli.prompt", "\\b \\> "),
        ("cli_server.color", "0"),
    ],
    listed: false,
    init: Some(core_init),
    ..Module::EMPTY
};

// ---------------------------------------------------------------- functions

fn call_user_func(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.val(0);
    let c = vm.resolve_callable(&f, true)?;
    let args: Vec<Value> = a.vals[1..].iter().map(|v| v.deref()).collect();
    if let Some(named) = a.named.take() {
        let mut arr = PhpArray::from_values(args);
        for (k, v) in named.iter() {
            arr.insert(k.to_owned(), v.clone());
        }
        return vm.call_callee_named(c, arr);
    }
    vm.call_callee(c, args)
}

fn call_user_func_array(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.val(0);
    let c = vm.resolve_callable(&f, true)?;
    let args = a.array(1);
    vm.call_callee_named(c, args)
}

fn forward_static_call(vm: &mut Vm, a: &mut Args) -> R<Value> {
    // Test class_object/forward_static_call_002.phpt.
    if vm.scope_ptr().is_null() {
        return vm.throw(
            "Error",
            "Cannot call forward_static_call() when no class scope is active",
        );
    }
    call_user_func(vm, a)
}

fn forward_static_call_array(vm: &mut Vm, a: &mut Args) -> R<Value> {
    // Observed on PHP 8.5: no scope check here, unlike forward_static_call().
    call_user_func_array(vm, a)
}

fn caller_is_function(vm: &Vm) -> bool {
    vm.has_frame() && vm.frame().code().flags & crate::bytecode::fflags::PSEUDO_MAIN == 0
}

fn func_get_args(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    if !caller_is_function(vm) {
        return vm.throw(
            "Error",
            "func_get_args() cannot be called from the global scope",
        );
    }
    Ok(Value::Array(vm.func_get_args()))
}

fn func_get_arg(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !caller_is_function(vm) {
        return vm.throw(
            "Error",
            "func_get_arg() cannot be called from the global scope",
        );
    }
    let n = a.int(0);
    if n < 0 {
        return vm.value_error(
            "func_get_arg(): Argument #1 ($position) must be greater than or equal to 0",
        );
    }
    let args = vm.func_get_args();
    match args.get_int(n) {
        Some(v) => Ok(v.clone()),
        None => vm.value_error("func_get_arg(): Argument #1 ($position) must be less than the number of the arguments passed to the currently executed function"),
    }
}

fn func_num_args(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    if !caller_is_function(vm) {
        return vm.throw(
            "Error",
            "func_num_args() must be called from a function context",
        );
    }
    Ok(Value::Int(vm.frame().argc as i64))
}

fn function_exists(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = a.str(0);
    let n = n.strip_prefix(b"\\").unwrap_or(&n);
    Ok(Value::Bool(vm.functions.contains_key(&lower(n))))
}

fn register_shutdown_function(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.val(0);
    let args = a.vals[1..].to_vec();
    vm.shutdown_functions.push((f, args));
    Ok(Value::Null)
}

fn get_defined_functions(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let mut internal: Vec<PhpStr> = vm
        .functions
        .iter()
        .filter(|(_, f)| f.is_native())
        .map(|(k, _)| k.clone())
        .collect();
    internal.sort();
    let mut out = PhpArray::new();
    out.insert(
        Key::from("internal"),
        Value::Array(PhpArray::from_values(internal.into_iter().map(Value::Str))),
    );
    out.insert(
        Key::from("user"),
        Value::Array(PhpArray::from_values(
            vm.user_functions.iter().cloned().map(Value::Str),
        )),
    );
    Ok(Value::Array(out))
}

fn get_defined_vars(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Array(vm.defined_vars()))
}

fn compact(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let mut out = PhpArray::new();
    let vals = a.vals.clone();
    fn add(vm: &mut Vm, out: &mut PhpArray, v: &Value, depth: usize, argn: usize) -> R<()> {
        match v {
            Value::Str(n) => {
                if n.as_bytes() == b"this" {
                    if let Some(t) = vm.this_obj() {
                        out.insert(Key::Str(n.clone()), Value::Object(t));
                    }
                    return Ok(());
                }
                let val = vm.fetch_var_var(&Value::Str(n.clone()), true)?;
                let exists = vm.var_exists(n);
                if exists {
                    out.insert(Key::from_php_str(n), val);
                } else {
                    vm.warn(&format!(
                        "compact(): Undefined variable ${}",
                        n.to_string_lossy()
                    ))?;
                }
            }
            Value::Array(arr) => {
                if depth > 64 {
                    vm.throw::<()>("Error", "Recursion detected")?;
                }
                for (_, x) in arr.iter() {
                    add(vm, out, &x.deref(), depth + 1, argn)?;
                }
            }
            Value::Ref(r) => add(vm, out, &r.get(), depth, argn)?,
            other => {
                vm.warn(&format!(
                    "compact(): Argument #{} must be string or array of strings, {} given",
                    argn,
                    Vm::type_name_given(other)
                ))?;
            }
        }
        Ok(())
    }
    for (i, v) in vals.iter().enumerate() {
        add(vm, &mut out, v, 0, i + 1)?;
    }
    Ok(Value::Array(out))
}

fn extract(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let arr = a.array(0);
    let flags = a.int_or(1, 0);
    let refs = flags & 256 != 0;
    let mode = flags & 255;
    let prefix = a.str(2);
    if mode > 6 {
        return vm.value_error("extract(): Argument #2 ($flags) must be a valid extract type");
    }
    if (2..=5).contains(&mode) && a.len() < 3 {
        return vm.value_error(
            "extract(): Argument #3 ($prefix) is required when using this extract type",
        );
    }
    // Observed on PHP 8.5: a prefix must itself start a valid name.
    if (2..=5).contains(&mode) && !prefix.is_empty() && !valid_var_name(&prefix) {
        return vm.value_error("extract(): Argument #3 ($prefix) must be a valid identifier");
    }
    let mut count = 0;
    let src_ref = a.ref_at(0);
    for (k, v) in arr.iter() {
        let KeyRef::Str(name) = k else {
            if mode == 3 || mode == 4 {
                let n = format!(
                    "{}_{}",
                    prefix.to_string_lossy(),
                    match k {
                        KeyRef::Int(i) => i,
                        _ => 0,
                    }
                );
                let nv = PhpStr::from(n);
                if valid_var_name(&nv) {
                    vm.assign_var_var(&Value::Str(nv), v.deref())?;
                    count += 1;
                }
            }
            continue;
        };
        let exists = vm.var_exists(name);
        let target: Option<PhpStr> = match mode {
            0 => Some(name.clone()),
            1 => (!exists).then(|| name.clone()),
            2 => Some(if exists {
                prefixed(&prefix, name)
            } else {
                name.clone()
            }),
            // Observed on PHP 8.5: EXTR_PREFIX_ALL skips an empty key
            // (EXTR_PREFIX_INVALID makes it "prefix_").
            3 => (!name.is_empty()).then(|| prefixed(&prefix, name)),
            4 => Some(if valid_var_name(name) {
                name.clone()
            } else {
                prefixed(&prefix, name)
            }),
            5 => exists.then(|| prefixed(&prefix, name)),
            6 => exists.then(|| name.clone()),
            _ => None,
        };
        let Some(t) = target else { continue };
        if !valid_var_name(&t) {
            continue;
        }
        if t.as_bytes() == b"this" {
            return vm.throw("Error", "Cannot re-assign $this");
        }
        if t.as_bytes() == b"GLOBALS" {
            continue;
        }
        if refs {
            // Bind to the array element by reference.
            if let Some(sr) = &src_ref {
                let r = {
                    let mut b = sr.borrow_mut();
                    match &mut *b {
                        Value::Array(arr) => Some(arr.entry(Key::Str(name.clone())).make_ref()),
                        _ => None,
                    }
                };
                if let Some(r) = r {
                    let p = vm.var_var_place(&t)?;
                    let old = unsafe { std::mem::replace(&mut *p, Value::Ref(r)) };
                    drop(old);
                    count += 1;
                    continue;
                }
            }
        }
        vm.assign_var_var(&Value::Str(t), v.deref())?;
        count += 1;
    }
    Ok(Value::Int(count))
}

fn prefixed(prefix: &[u8], name: &[u8]) -> PhpStr {
    let mut v = prefix.to_vec();
    v.push(b'_');
    v.extend_from_slice(name);
    PhpStr::from(v)
}

fn valid_var_name(n: &[u8]) -> bool {
    match n.first() {
        Some(&c) if c == b'_' || c.is_ascii_alphabetic() || c >= 0x80 => {}
        _ => return false,
    }
    n.iter()
        .all(|&c| c == b'_' || c.is_ascii_alphanumeric() || c >= 0x80)
}

impl Vm {
    /// Whether a variable of the current scope exists (is set, even null).
    pub fn var_exists(&mut self, name: &PhpStr) -> bool {
        let vars = self.defined_vars();
        vars.get_str(name).is_some()
    }
}

// ---------------------------------------------------------------- constants

fn define(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let name = a.str(0);
    let v = a.val(1);
    if a.bool_or(2, false) {
        vm.warn("define(): Argument #3 ($case_insensitive) is ignored since declaration of case-insensitive constants is no longer supported")?;
    }
    if let Some(pos) = name.windows(2).position(|w| w == b"::") {
        let _ = pos;
        return vm.value_error("define(): Argument #1 ($constant_name) cannot be a class constant");
    }
    let key = crate::vm::include::normalize_const_name(&name);
    if vm.constants.contains_key(&key)
        || matches!(&name.to_ascii_lowercase()[..], b"true" | b"false" | b"null")
    {
        vm.warn(&format!(
            "Constant {} already defined, this will be an error in PHP 9",
            name.to_string_lossy()
        ))?;
        return Ok(Value::Bool(false));
    }
    if let Value::Object(_) = &v {
        // Objects are allowed as constant values since PHP 8.1.
    }
    vm.constants.insert(key.clone(), v);
    super::reflection::note_constant(vm, &key);
    vm.user_constants.push(key);
    Ok(Value::Bool(true))
}

fn defined(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let name = a.str(0);
    if let Some(pos) = name.windows(2).position(|w| w == b"::") {
        let cls = &name[..pos];
        let cn = PhpStr::new(&name[pos + 2..]);
        return Ok(Value::Bool(match vm.find_class(cls, true)? {
            Some(c) => c.find_const(&cn).is_some(),
            None => false,
        }));
    }
    Ok(Value::Bool(vm.lookup_constant(&name)?.is_some()))
}

fn constant(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let name = a.str(0);
    if let Some(pos) = name.windows(2).position(|w| w == b"::") {
        let cls = &name[..pos];
        let cn = PhpStr::new(&name[pos + 2..]);
        let c = match lower(cls).as_bytes() {
            b"self" | b"static" | b"parent" => {
                let spec = match lower(cls).as_bytes() {
                    b"self" => crate::bytecode::ClassSpec::SelfRef,
                    b"static" => crate::bytecode::ClassSpec::Static,
                    _ => crate::bytecode::ClassSpec::Parent,
                };
                vm.resolve_class_spec(spec, true)?
            }
            _ => match vm.find_class(cls, true)? {
                Some(c) => c,
                None => {
                    return vm.throw(
                        "Error",
                        &format!("Class \"{}\" not found", String::from_utf8_lossy(cls)),
                    );
                }
            },
        };
        return vm.class_constant(&c, &cn);
    }
    match vm.lookup_constant(&name)? {
        Some(v) => {
            vm.const_deprecated(&name)?;
            Ok(v)
        }
        None => vm.throw(
            "Error",
            &format!("Undefined constant \"{}\"", name.to_string_lossy()),
        ),
    }
}

fn get_defined_constants(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let mut out = PhpArray::new();
    if a.bool_or(0, false) {
        let mut core = PhpArray::new();
        let mut keys: Vec<&PhpStr> = vm.constants.keys().collect();
        keys.sort();
        for k in keys {
            if !vm.user_constants.contains(k) {
                core.insert(Key::Str(k.clone()), vm.constants[k].clone());
            }
        }
        out.insert(Key::from("Core"), Value::Array(core));
        let mut user = PhpArray::new();
        for k in &vm.user_constants {
            if let Some(v) = vm.constants.get(k) {
                user.insert(Key::Str(k.clone()), v.clone());
            }
        }
        if !user.is_empty() {
            out.insert(Key::from("user"), Value::Array(user));
        }
        return Ok(Value::Array(out));
    }
    let mut keys: Vec<PhpStr> = vm
        .constants
        .keys()
        .filter(|k| !vm.user_constants.contains(k))
        .cloned()
        .collect();
    keys.sort();
    for k in keys {
        out.insert(Key::Str(k.clone()), vm.constants[&k].clone());
    }
    for k in &vm.user_constants {
        if let Some(v) = vm.constants.get(k) {
            out.insert(Key::Str(k.clone()), v.clone());
        }
    }
    Ok(Value::Array(out))
}

// ---------------------------------------------------------------- classes

fn kind_exists(vm: &mut Vm, a: &mut Args, pred: fn(&Class) -> bool) -> R<Value> {
    let n = a.str(0);
    let autoload = a.bool_or(1, true);
    Ok(Value::Bool(match vm.find_class(&n, autoload)? {
        Some(c) => pred(&c),
        None => false,
    }))
}

fn class_exists(vm: &mut Vm, a: &mut Args) -> R<Value> {
    // Enums are classes too (test Zend/tests/bug18556.phpt walks
    // get_declared_classes(), which lists RoundingMode).
    kind_exists(vm, a, |c| {
        c.kind == crate::class::ClassKind::Class || c.is_enum()
    })
}
fn interface_exists(vm: &mut Vm, a: &mut Args) -> R<Value> {
    kind_exists(vm, a, |c| c.is_interface())
}
fn trait_exists(vm: &mut Vm, a: &mut Args) -> R<Value> {
    kind_exists(vm, a, |c| c.is_trait())
}
fn enum_exists(vm: &mut Vm, a: &mut Args) -> R<Value> {
    kind_exists(vm, a, |c| c.is_enum())
}

fn get_class(vm: &mut Vm, a: &mut Args) -> R<Value> {
    match a.get(0) {
        Some(Value::Object(o)) => Ok(Value::Str(vm.class_of(o).name.clone())),
        Some(_) => Ok(Value::Bool(false)),
        None => {
            let p = vm.scope_ptr();
            if p.is_null() {
                return vm.throw(
                    "Error",
                    "get_class() without arguments must be called from within a class",
                );
            }
            vm.deprecated("Calling get_class() without arguments is deprecated")?;
            Ok(Value::Str(unsafe { (*p).name.clone() }))
        }
    }
}

fn class_arg(vm: &mut Vm, v: &Value, autoload: bool) -> R<Option<Rc<Class>>> {
    match v {
        Value::Object(o) => Ok(Some(vm.class_of(o))),
        Value::Str(s) => vm.find_class(s, autoload),
        _ => Ok(None),
    }
}

fn get_parent_class(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = if a.has(0) {
        let v = a.val(0);
        let c = class_arg(vm, &v, true)?;
        if c.is_none() {
            return vm.type_error(&format!(
                "get_parent_class(): Argument #1 ($object_or_class) must be an object or a valid class name, {} given",
                v.tname()
            ));
        }
        c
    } else {
        let p = vm.scope_ptr();
        (!p.is_null()).then(|| vm.rc_class(p))
    };
    Ok(match c.and_then(|c| c.parent.clone()) {
        Some(p) => Value::Str(p.name.clone()),
        None => Value::Bool(false),
    })
}

fn get_called_class(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let p = vm.static_class_ptr();
    if p.is_null() {
        return vm.throw(
            "Error",
            "get_called_class() must be called from within a class",
        );
    }
    Ok(Value::Str(unsafe { (*p).name.clone() }))
}

fn method_exists(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let v = a.val(0);
    let c = match &v {
        Value::Object(_) | Value::Str(_) => class_arg(vm, &v, true)?,
        other => {
            return vm.type_error(&format!(
                "method_exists(): Argument #1 ($object_or_class) must be of type object|string, {} given",
                other.tname()
            ));
        }
    };
    let m = lower(&a.str(1));
    Ok(Value::Bool(match c {
        Some(c) => {
            c.find_method_str(&m).is_some()
                || (c.lname.as_bytes() == b"closure" && m.as_bytes() == b"__invoke")
        }
        None => false,
    }))
}

fn property_exists(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let v = a.val(0);
    let c = match &v {
        Value::Object(_) | Value::Str(_) => class_arg(vm, &v, true)?,
        other => {
            return vm.type_error(&format!(
                "property_exists(): Argument #1 ($object_or_class) must be of type object|string, {} given",
                other.tname()
            ));
        }
    };
    let p = a.str(1);
    let Some(c) = c else {
        return Ok(Value::Bool(false));
    };
    // Private properties of ancestors do not count (observed on PHP 8.5).
    // For objects, a private property of the calling scope's class is
    // found too (observed on PHP 8.5; test Zend/tests/inheritance/bug50810).
    let scope = if matches!(v, Value::Object(_)) {
        vm.scope_ptr()
    } else {
        std::ptr::null()
    };
    let own_or_visible = |flags: u32, decl: *const crate::class::Class| {
        flags & crate::class::mflags::PRIVATE == 0
            || std::ptr::eq(decl, Rc::as_ptr(&c))
            || std::ptr::eq(decl, scope)
    };
    let declared = {
        let map = c.prop_map.borrow();
        map.get(&p)
            .map(|idxs| {
                let pi = c.props.borrow()[idxs[0] as usize].clone();
                own_or_visible(pi.flags, pi.class)
            })
            .unwrap_or(false)
    };
    if declared
        || c.find_static(&p)
            .is_some_and(|s| own_or_visible(s.flags, s.class))
    {
        return Ok(Value::Bool(true));
    }
    if let Value::Object(o) = &v {
        return Ok(Value::Bool(o.obj().props.borrow().get_str(&p).is_some()));
    }
    Ok(Value::Bool(false))
}

fn get_object_vars(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(o) = a.obj(0) else {
        return Ok(Value::Null);
    };
    let mut out = PhpArray::new();
    for (k, v) in vm.visible_props(&o) {
        let key = match k {
            Value::Int(i) => Key::Int(i),
            Value::Str(s) => Key::from_php_str(&s),
            _ => continue,
        };
        out.insert(key, v);
    }
    Ok(Value::Array(out))
}

fn get_mangled_object_vars(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(o) = a.obj(0) else {
        return Ok(Value::Null);
    };
    Ok(Value::Array(vm.object_props_mangled(&o)))
}

fn get_class_vars(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = a.str(0);
    let Some(c) = vm.find_class(&n, true)? else {
        return Ok(Value::Bool(false));
    };
    let scope = vm.scope_ptr();
    let defaults = vm.class_defaults(&c)?;
    let mut out = PhpArray::new();
    for p in c.props.borrow().iter() {
        if p.flags & crate::class::mflags::HIDDEN != 0 {
            continue;
        }
        if let crate::vm::objects::PropLookup::Found(f) = vm.lookup_prop(&c, &p.name, scope)
            && Rc::ptr_eq(&f, p)
        {
            let v = &defaults[p.slot as usize];
            if vm.is_uninit(v) {
                continue;
            }
            out.insert(Key::Str(p.name.clone()), v.clone());
        }
    }
    vm.ensure_statics(&c)?;
    for sp in c.static_props.borrow().iter() {
        let visible = sp.flags & mflags::PUBLIC != 0
            || std::ptr::eq(sp.class, scope)
            || (!scope.is_null() && sp.flags & mflags::PROTECTED != 0);
        if visible {
            let v = sp.cell.borrow().get();
            if !vm.is_uninit(&v) {
                out.insert(Key::Str(sp.name.clone()), v);
            }
        }
    }
    Ok(Value::Array(out))
}

fn get_class_methods(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let v = a.val(0);
    let Some(c) = class_arg(vm, &v, true)? else {
        return vm.type_error(&format!(
            "get_class_methods(): Argument #1 ($object_or_class) must be an object or a valid class name, {} given",
            v.tname()
        ));
    };
    let scope = vm.scope_ptr();
    let mut out = PhpArray::new();
    let order = c.method_order.borrow().clone();
    // Own methods first in declaration order (PHP lists the class's own
    // methods, then inherited ones).
    for ln in order {
        let f = c.methods.borrow().get(&ln).cloned().unwrap();
        if vm.method_visible(&f, scope) {
            let _ = out.push(Value::Str(f.name.clone()));
        }
    }
    Ok(Value::Array(out))
}

fn is_a_impl(vm: &mut Vm, a: &mut Args, allow_string_default: bool, strict_sub: bool) -> R<Value> {
    let v = a.val(0);
    let cn = a.str(1);
    let allow_string = a.bool_or(2, allow_string_default);
    let c = match &v {
        Value::Object(o) => vm.class_of(o),
        Value::Str(s) if allow_string => match vm.find_class(s, true)? {
            Some(c) => c,
            None => return Ok(Value::Bool(false)),
        },
        _ => return Ok(Value::Bool(false)),
    };
    let lname = lower(cn.strip_prefix(b"\\").unwrap_or(&cn));
    if strict_sub && c.lname == lname {
        return Ok(Value::Bool(false));
    }
    Ok(Value::Bool(vm.instance_of_lname(&c, &lname)))
}

fn is_a(vm: &mut Vm, a: &mut Args) -> R<Value> {
    is_a_impl(vm, a, false, false)
}

fn is_subclass_of(vm: &mut Vm, a: &mut Args) -> R<Value> {
    is_a_impl(vm, a, true, true)
}

fn class_list_arg(vm: &mut Vm, a: &mut Args, fname: &str) -> R<Option<Rc<Class>>> {
    let v = a.val(0);
    let autoload = a.bool_or(1, true);
    match &v {
        Value::Object(o) => Ok(Some(vm.class_of(o))),
        Value::Str(s) => match vm.find_class(s, autoload)? {
            Some(c) => Ok(Some(c)),
            None => {
                vm.warn(&format!(
                    "{}(): Class {} does not exist{}",
                    fname,
                    s.to_string_lossy(),
                    if autoload {
                        " and could not be loaded"
                    } else {
                        ""
                    }
                ))?;
                Ok(None)
            }
        },
        other => vm.type_error(&format!(
            "{}(): Argument #1 ($object_or_class) must be of type object|string, {} given",
            fname,
            Vm::type_name_given(other)
        )),
    }
}

fn class_implements(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(c) = class_list_arg(vm, a, "class_implements")? else {
        return Ok(Value::Bool(false));
    };
    let mut out = PhpArray::new();
    for i in c.interfaces.borrow().iter() {
        out.insert(Key::Str(i.name.clone()), Value::Str(i.name.clone()));
    }
    Ok(Value::Array(out))
}

fn class_parents(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(c) = class_list_arg(vm, a, "class_parents")? else {
        return Ok(Value::Bool(false));
    };
    let mut out = PhpArray::new();
    let mut p = c.parent.clone();
    while let Some(x) = p {
        out.insert(Key::Str(x.name.clone()), Value::Str(x.name.clone()));
        p = x.parent.clone();
    }
    Ok(Value::Array(out))
}

fn class_uses(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(c) = class_list_arg(vm, a, "class_uses")? else {
        return Ok(Value::Bool(false));
    };
    let mut out = PhpArray::new();
    for t in c.traits.borrow().iter() {
        out.insert(Key::Str(t.name.clone()), Value::Str(t.name.clone()));
    }
    Ok(Value::Array(out))
}

fn class_alias(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let orig = a.str(0);
    let alias = a.str(1);
    let autoload = a.bool_or(2, true);
    let Some(c) = vm.find_class(&orig, autoload)? else {
        vm.warn(&format!("Class \"{}\" not found", orig.to_string_lossy()))?;
        return Ok(Value::Bool(false));
    };
    if c.has_flag(cflags::INTERNAL) {
        return vm.value_error("class_alias(): Argument #1 ($class) must be a user-defined class name, internal class name given");
    }
    let la = lower(alias.strip_prefix(b"\\").unwrap_or(&alias));
    if vm.classes.contains_key(&la) {
        vm.warn(&format!(
            "Cannot declare class {}, because the name is already in use",
            alias.to_string_lossy()
        ))?;
        return Ok(Value::Bool(false));
    }
    vm.classes.insert(la, c);
    Ok(Value::Bool(true))
}

fn declared(vm: &mut Vm, pred: fn(&Class) -> bool) -> PhpArray {
    // Internal classes are all declared (force lazy ones).
    let lazy: Vec<PhpStr> = vm.lazy_classes.keys().cloned().collect();
    for l in lazy {
        let _ = vm.find_class(&l, false);
    }
    let mut out = PhpArray::new();
    for n in vm.class_order.clone() {
        if let Some(c) = vm.classes.get(&n)
            && pred(c)
            && c.lname == n
        {
            let _ = out.push(Value::Str(c.name.clone()));
        }
    }
    out
}

fn get_declared_classes(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Array(declared(vm, |c| {
        c.kind == crate::class::ClassKind::Class || c.is_enum()
    })))
}
fn get_declared_interfaces(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Array(declared(vm, |c| c.is_interface())))
}
fn get_declared_traits(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Array(declared(vm, |c| c.is_trait())))
}

fn spl_object_id(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Int(a.obj(0).map(|o| o.handle() as i64).unwrap_or(0)))
}

fn spl_object_hash(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let h = a.obj(0).map(|o| o.handle()).unwrap_or(0);
    Ok(Value::Str(PhpStr::from(format!("{:032x}", h))))
}

fn iterator_to_array(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let v = a.val(0);
    let keys = a.bool_or(1, true);
    if let Value::Array(arr) = &v {
        if keys {
            return Ok(Value::Array(arr.clone()));
        }
        return Ok(Value::Array(PhpArray::from_values(
            arr.iter().map(|(_, x)| x.deref()),
        )));
    }
    let items = vm.iterate_to_vec(&v, keys)?;
    let mut out = PhpArray::new();
    for (k, x) in items {
        if keys {
            let key = match &k {
                Value::Int(i) => Key::Int(*i),
                Value::Str(s) => Key::from_php_str(s),
                Value::Null => Key::Str(PhpStr::empty()),
                Value::Bool(b) => Key::Int(*b as i64),
                Value::Float(f) => Key::Int(*f as i64),
                _ => {
                    return vm.type_error("Cannot access offset of type array on array");
                }
            };
            out.insert(key, x);
        } else {
            let _ = out.push(x);
        }
    }
    Ok(Value::Array(out))
}

fn iterator_count(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let v = a.val(0);
    if let Value::Array(arr) = &v {
        return Ok(Value::Int(arr.len() as i64));
    }
    let items = vm.iterate_to_vec(&v, false)?;
    Ok(Value::Int(items.len() as i64))
}

fn iterator_apply(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let v = a.val(0);
    let f = a.val(1);
    let args: Vec<Value> = match a.val(2) {
        Value::Array(arr) => arr.iter().map(|(_, x)| x.clone()).collect(),
        _ => Vec::new(),
    };
    let Value::Object(o) = &v else {
        return Ok(Value::Int(0));
    };
    let it = vm.get_iterator_of(o)?;
    let mut n = 0;
    vm.call_method(&it, b"rewind", Vec::new())?;
    loop {
        let valid = vm.call_method(&it, b"valid", Vec::new())?;
        if !vm.to_bool(&valid) {
            break;
        }
        n += 1;
        let r = vm.call_value(&f, args.clone())?;
        if !vm.to_bool(&r) {
            break;
        }
        vm.call_method(&it, b"next", Vec::new())?;
    }
    Ok(Value::Int(n))
}

// ---------------------------------------------------------------- errors

fn error_reporting(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let old = vm.ini.error_reporting;
    if let Some(n) = a.opt_int(0) {
        vm.ini.error_reporting = n;
        vm.ini.values.insert(
            PhpStr::intern(b"error_reporting"),
            PhpStr::from(n.to_string()),
        );
    }
    Ok(Value::Int(old))
}

fn set_error_handler(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let h = a.val(0);
    if !h.is_null() && !vm.is_callable_value(&h, false) {
        let name = vm.callable_name(&h);
        let d = describe_bad_callable(vm, &h, &name);
        return vm.type_error(&format!(
            "set_error_handler(): Argument #1 ($callback) must be a valid callback or null, {}",
            d
        ));
    }
    let mask = a.int_or(1, E_ALL);
    let old = vm
        .err
        .handlers
        .last()
        .map(|(h, _)| h.clone())
        .unwrap_or(Value::Null);
    vm.err.handlers.push((h, mask));
    Ok(old)
}

fn describe_bad_callable(vm: &mut Vm, v: &Value, name: &PhpStr) -> String {
    match v {
        Value::Str(_) => {
            if name.windows(2).any(|w| w == b"::") {
                format!(
                    "class \"{}\" not found",
                    name.split(|&c| c == b':')
                        .next()
                        .map(String::from_utf8_lossy)
                        .unwrap_or_default()
                )
            } else {
                format!(
                    "function \"{}\" not found or invalid function name",
                    name.to_string_lossy()
                )
            }
        }
        Value::Array(_) => "array callback must have exactly two members".to_string(),
        _ => {
            let _ = vm;
            "no array or string given".to_string()
        }
    }
}

fn restore_error_handler(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    vm.err.handlers.pop();
    Ok(Value::Bool(true))
}

fn set_exception_handler(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let h = a.val(0);
    if !h.is_null() && !vm.is_callable_value(&h, false) {
        let name = vm.callable_name(&h);
        let d = describe_bad_callable(vm, &h, &name);
        return vm.type_error(&format!(
            "set_exception_handler(): Argument #1 ($callback) must be a valid callback or null, {}",
            d
        ));
    }
    let old = vm
        .err
        .exception_handlers
        .last()
        .cloned()
        .unwrap_or(Value::Null);
    vm.err.exception_handlers.push(h);
    Ok(old)
}

fn restore_exception_handler(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    vm.err.exception_handlers.pop();
    Ok(Value::Bool(true))
}

fn get_error_handler(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(vm
        .err
        .handlers
        .last()
        .map(|(h, _)| h.clone())
        .unwrap_or(Value::Null))
}

fn get_exception_handler(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(vm
        .err
        .exception_handlers
        .last()
        .cloned()
        .unwrap_or(Value::Null))
}

fn trigger_error(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let msg = a.str(0);
    let level = a.int_or(1, E_USER_NOTICE);
    match level {
        E_USER_ERROR => {
            vm.deprecated("Passing E_USER_ERROR to trigger_error() is deprecated since 8.4, throw an exception or call exit with a string message instead")?;
            // A user error handler may take it; otherwise it's fatal.
            let handled = vm.err.handlers.last().is_some_and(|(h, m)| !h.is_null() && m & E_USER_ERROR != 0);
            if handled {
                vm.raise(E_USER_ERROR, &msg.to_string_lossy())?;
                return Ok(Value::Bool(true));
            }
            let file = vm.cur_file();
            let line = vm.cur_line();
            vm.err.last = Some((E_USER_ERROR, msg.clone(), file.clone(), line as i64));
            if vm.ini.error_reporting & E_USER_ERROR != 0 {
                vm.display_error(E_USER_ERROR, &msg.to_string_lossy(), &file, line);
            }
            vm.exit_status = 255;
            Err(Unwind::Fatal)
        }
        E_USER_WARNING | E_USER_NOTICE | E_USER_DEPRECATED => {
            vm.raise(level, &msg.to_string_lossy())?;
            Ok(Value::Bool(true))
        }
        _ => vm.value_error("trigger_error(): Argument #2 ($error_level) must be one of E_USER_ERROR, E_USER_WARNING, E_USER_NOTICE, or E_USER_DEPRECATED"),
    }
}

fn error_log(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let msg = a.str(0);
    let ty = a.int_or(1, 0);
    match ty {
        3 => {
            let dest = a.opt_str(2).unwrap_or_default();
            use std::io::Write;
            use std::os::unix::ffi::OsStrExt;
            let p = std::path::Path::new(std::ffi::OsStr::from_bytes(&dest));
            let ok = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(p)
                .and_then(|mut f| f.write_all(&msg))
                .is_ok();
            Ok(Value::Bool(ok))
        }
        4 => {
            vm.out.flush();
            let mut v = msg.to_vec();
            v.push(b'\n');
            vm.write_stderr(&v);
            Ok(Value::Bool(true))
        }
        _ => {
            let target = vm.get_ini(b"error_log").unwrap_or_default();
            if !target.is_empty() {
                use std::io::Write;
                use std::os::unix::ffi::OsStrExt;
                let p = std::path::Path::new(std::ffi::OsStr::from_bytes(&target));
                let line = format!(
                    "[{}] {}\n",
                    "01-Jan-1970 00:00:00 UTC",
                    msg.to_string_lossy()
                );
                let ok = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(p)
                    .and_then(|mut f| f.write_all(line.as_bytes()))
                    .is_ok();
                return Ok(Value::Bool(ok));
            }
            vm.out.flush();
            let mut v = msg.to_vec();
            v.push(b'\n');
            vm.write_stderr(&v);
            Ok(Value::Bool(true))
        }
    }
}

fn error_get_last(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    match &vm.err.last {
        None => Ok(Value::Null),
        Some((t, m, f, l)) => {
            let mut a = PhpArray::new();
            a.insert(Key::from("type"), Value::Int(*t));
            a.insert(Key::from("message"), Value::Str(m.clone()));
            a.insert(Key::from("file"), Value::Str(f.clone()));
            a.insert(Key::from("line"), Value::Int(*l));
            Ok(Value::Array(a))
        }
    }
}

fn error_clear_last(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    vm.err.last = None;
    Ok(Value::Null)
}

fn debug_backtrace(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let opts = a.int_or(0, 1);
    let limit = a.int_or(1, 0);
    let mut frames = vm.backtrace_sealed(opts & 2 == 0);
    // The debug_backtrace() call itself is not part of the trace.
    if !frames.is_empty() {
        frames.remove(0);
    }
    if limit > 0 {
        frames.truncate(limit as usize);
    }
    Ok(Value::Array(vm.trace_array(&frames, opts & 1 != 0)))
}

fn debug_print_backtrace(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let opts = a.int_or(0, 0);
    let limit = a.int_or(1, 0);
    let mut frames = vm.backtrace_sealed(opts & 2 == 0);
    if !frames.is_empty() {
        frames.remove(0);
    }
    if limit > 0 {
        frames.truncate(limit as usize);
    }
    let arr = vm.trace_array(&frames, false);
    let s = vm.trace_string(&arr);
    // Without the trailing "{main}" line.
    let s = match s.rfind('#') {
        Some(p) if s[p..].contains("{main}") => s[..p].to_string(),
        _ => s,
    };
    vm.echo(s.as_bytes());
    Ok(Value::Null)
}

fn assert(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if vm.ini.assertions != 1 {
        return Ok(Value::Bool(true));
    }
    let v = a.val(0);
    if vm.to_bool(&v) {
        return Ok(Value::Bool(true));
    }
    if vm
        .get_ini(b"assert.active")
        .is_some_and(|v| goro_runtime::convert::str_to_int(&v) == 0)
    {
        return Ok(Value::Bool(true));
    }
    // The compiler passes the assertion's source text when no description
    // is given; an explicit null stays null.
    let desc = a.val(1);
    // assert.callback / assert.exception / assert.warning / assert.bail.
    super::misc::assert_failed(vm, desc)
}

// ---------------------------------------------------------------- ini

fn ini_get(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let k = a.str(0);
    Ok(match vm.get_ini(&k) {
        Some(v) => Value::Str(v),
        None => Value::Bool(false),
    })
}

fn ini_set(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let k = a.str(0);
    let v = match a.val(1) {
        Value::Null => PhpStr::empty(),
        Value::Bool(b) => PhpStr::from(if b { "1" } else { "" }),
        other => vm.to_php_str(&other)?,
    };
    let Some(old) = vm.get_ini(&k) else {
        return Ok(Value::Bool(false));
    };
    // open_basedir can only be tightened at run time (PHP manual: "can only
    // be set to a more restrictive value").
    if k.as_bytes() == b"open_basedir"
        && !old.is_empty()
        && (v.is_empty()
            || v.split(|&c| c == b':')
                .any(|d| !d.is_empty() && !vm.basedir_allows(d)))
    {
        return Ok(Value::Bool(false));
    }
    if k.as_bytes() == b"date.timezone" && !super::date::ini_timezone_check(vm, a, &v, &old)? {
        return Ok(Value::Bool(false));
    }
    if super::misc::assert_ini_deprecated(&k, &v) {
        vm.fdeprecated(
            a,
            &format!("{} INI setting is deprecated", k.to_string_lossy()),
        )?;
    }
    vm.ini
        .originals
        .entry(k.clone())
        .or_insert(Some(old.clone()));
    vm.set_ini(&k, &v);
    if k.as_bytes() == b"assert.callback" {
        super::misc::assert_callback_reset(vm);
    }
    Ok(Value::Str(old))
}

fn ini_restore(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let k = a.str(0);
    if let Some(Some(orig)) = vm.ini.originals.remove(&k) {
        vm.set_ini(&k, &orig);
    }
    Ok(Value::Null)
}

/// PHP manual: ini_get_all(); an extension's settings are those its
/// module registers, an unknown extension is a warning.
fn ini_get_all(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let details = a.bool_or(1, true);
    let mut keys: Vec<PhpStr> = match a.opt_str(0) {
        None => vm.ini.values.keys().cloned().collect(),
        Some(ext) => {
            let mods: Vec<_> = vm
                .modules
                .iter()
                .filter(|m| m.name.as_bytes().eq_ignore_ascii_case(&ext))
                .collect();
            if mods.is_empty() {
                vm.fwarn(
                    a,
                    &format!("Extension \"{}\" cannot be found", ext.to_string_lossy()),
                )?;
                return Ok(Value::Bool(false));
            }
            mods.iter()
                .flat_map(|m| m.ini.iter().map(|(k, _)| PhpStr::from(*k)))
                .collect()
        }
    };
    keys.sort();
    keys.dedup();
    let mut out = PhpArray::new();
    for k in keys {
        let Some(v) = vm.ini.values.get(&k).cloned() else {
            continue;
        };
        if !details {
            out.insert(Key::Str(k), Value::Str(v));
            continue;
        }
        let g = match vm.ini.originals.get(&k) {
            Some(Some(o)) => o.clone(),
            _ => v.clone(),
        };
        let mut e = PhpArray::new();
        e.insert(Key::from("global_value"), Value::Str(g));
        e.insert(Key::from("local_value"), Value::Str(v));
        e.insert(Key::from("access"), Value::Int(7));
        out.insert(Key::Str(k), Value::Array(e));
    }
    Ok(Value::Array(out))
}

fn set_include_path(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let old = vm.ini.include_path.clone();
    let p = a.str(0);
    // Test general_functions/include_path.phpt: an empty path is refused,
    // and ini_restore() restores the startup value.
    if p.is_empty() {
        return Ok(Value::Bool(false));
    }
    vm.ini
        .originals
        .entry(PhpStr::from("include_path"))
        .or_insert(Some(old.clone()));
    vm.set_ini(b"include_path", &p);
    Ok(Value::Str(old))
}

fn get_include_path(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Str(vm.ini.include_path.clone()))
}

fn get_cfg_var(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let k = a.str(0);
    if k.as_bytes() == b"cfg_file_path" {
        return Ok(Value::Str(PhpStr::empty()));
    }
    if let Some(v) = vm.get_ini(&k) {
        return Ok(Value::Str(v));
    }
    // Array entries (`ary[a] = 1` in the configuration) come back as an
    // array (test get_cfg_var_array).
    let mut prefix = k.to_vec();
    prefix.push(b'[');
    let mut items: Vec<(PhpStr, PhpStr)> = vm
        .ini
        .values
        .iter()
        .filter(|(n, _)| n.starts_with(&prefix) && n.ends_with(b"]"))
        .map(|(n, v)| (PhpStr::new(&n[prefix.len()..n.len() - 1]), v.clone()))
        .collect();
    if items.is_empty() {
        return Ok(Value::Bool(false));
    }
    items.sort();
    let mut out = PhpArray::new();
    for (sub, v) in items {
        out.insert(Key::from_php_str(&sub), Value::Str(v));
    }
    Ok(Value::Array(out))
}

fn php_ini_loaded_file(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Bool(false))
}

// ---------------------------------------------------------------- runtime

fn gc_collect_cycles(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let n = vm.collect_cycles()?;
    Ok(Value::Int(n as i64))
}
fn gc_enable(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    vm.gc_enabled = true;
    Ok(Value::Null)
}
fn gc_disable(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    vm.gc_enabled = false;
    Ok(Value::Null)
}
fn gc_enabled(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Bool(vm.gc_enabled))
}
fn gc_mem_caches(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Int(0))
}
fn gc_status(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let st = goro_runtime::gc::stats();
    let mut a = PhpArray::new();
    a.insert(Key::from("runs"), Value::Int(st.runs as i64));
    a.insert(Key::from("collected"), Value::Int(st.collected as i64));
    a.insert(
        Key::from("threshold"),
        Value::Int(goro_runtime::gc::DEFAULT_THRESHOLD as i64 + 1),
    );
    a.insert(
        Key::from("roots"),
        Value::Int(goro_runtime::gc::root_count() as i64),
    );
    a.insert(Key::from("running"), Value::Bool(false));
    a.insert(Key::from("protected"), Value::Bool(false));
    a.insert(Key::from("full"), Value::Bool(false));
    a.insert(Key::from("buffer_size"), Value::Int(16384));
    a.insert(
        Key::from("application_time"),
        Value::Float(vm.start_time.elapsed().as_secs_f64()),
    );
    a.insert(Key::from("collector_time"), Value::Float(0.0));
    a.insert(Key::from("destructor_time"), Value::Float(0.0));
    a.insert(Key::from("free_time"), Value::Float(0.0));
    Ok(Value::Array(a))
}

fn memory_get_usage(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    // An estimate: live objects and slots in use.
    let base = 2 * 1024 * 1024;
    let objs = goro_runtime::object::live_object_count() * 64;
    let slots = vm.ex.stack.used_slots() * 16;
    Ok(Value::Int((base + objs + slots) as i64))
}

fn noop_void(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Null)
}

fn zero_int(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Int(0))
}

fn empty_array(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Array(PhpArray::new()))
}

fn php_sapi_name(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Str(PhpStr::from("cli")))
}

fn phpversion(vm: &mut Vm, a: &mut Args) -> R<Value> {
    match a.opt_str(0) {
        None => Ok(Value::Str(PhpStr::from("8.5.4"))),
        Some(e) => {
            let l = e.to_ascii_lowercase();
            if vm
                .modules
                .iter()
                .any(|m| m.name.as_bytes().eq_ignore_ascii_case(&l))
                || is_bundled(&l)
            {
                Ok(Value::Str(PhpStr::from("8.5.4")))
            } else {
                Ok(Value::Bool(false))
            }
        }
    }
}

fn is_bundled(l: &[u8]) -> bool {
    matches!(
        l,
        b"core"
            | b"standard"
            | b"date"
            | b"pcre"
            | b"spl"
            | b"reflection"
            | b"json"
            | b"random"
            | b"hash"
    )
}

fn zend_version(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Str(PhpStr::from("4.5.4")))
}

fn uname() -> (String, String, String, String, String) {
    let mut u: libc::utsname = unsafe { std::mem::zeroed() };
    unsafe { libc::uname(&mut u) };
    let f = |b: &[libc::c_char]| {
        let bytes: Vec<u8> = b
            .iter()
            .take_while(|&&c| c != 0)
            .map(|&c| c as u8)
            .collect();
        String::from_utf8_lossy(&bytes).into_owned()
    };
    (
        f(&u.sysname),
        f(&u.nodename),
        f(&u.release),
        f(&u.version),
        f(&u.machine),
    )
}

fn php_uname(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let mode = a.str(0);
    // Test general_functions/php_uname_error.phpt.
    if a.has(0) && mode.len() != 1 {
        return vm.value_error("php_uname(): Argument #1 ($mode) must be a single character");
    }
    let (s, n, r, v, m) = uname();
    let out = match mode.first() {
        Some(b's') => s,
        Some(b'n') => n,
        Some(b'r') => r,
        Some(b'v') => v,
        Some(b'm') => m,
        Some(b'a') | None => format!("{} {} {} {} {}", s, n, r, v, m),
        _ => {
            return vm.value_error("php_uname(): Argument #1 ($mode) must be one of \"a\", \"m\", \"n\", \"r\", \"s\", or \"v\"");
        }
    };
    Ok(Value::Str(PhpStr::from(out)))
}

fn extension_loaded(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let e = a.str(0).to_ascii_lowercase();
    Ok(Value::Bool(
        vm.modules
            .iter()
            .any(|m| m.name.as_bytes().eq_ignore_ascii_case(&e)),
    ))
}

fn get_loaded_extensions(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if a.bool_or(0, false) {
        return Ok(Value::Array(PhpArray::new()));
    }
    let mut seen: Vec<&str> = Vec::new();
    for m in &vm.modules {
        if m.listed && !seen.contains(&m.name) {
            seen.push(m.name);
        }
    }
    Ok(Value::Array(PhpArray::from_values(
        seen.into_iter().map(Value::from),
    )))
}

fn get_extension_funcs(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let e = a.str(0).to_ascii_lowercase();
    let mut out = PhpArray::new();
    let mut found = false;
    for m in &vm.modules {
        if m.name.as_bytes().eq_ignore_ascii_case(&e) {
            found = true;
            for (sig, _) in m.functions {
                let _ = out.push(Value::from(crate::registry::sig_name(sig)));
            }
        }
    }
    Ok(if found {
        Value::Array(out)
    } else {
        Value::Bool(false)
    })
}

fn dl(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    vm.warn("dl(): Dynamically loaded extensions aren't enabled")?;
    Ok(Value::Bool(false))
}

fn getmypid(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Int(std::process::id() as i64))
}
fn getmyuid(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Int(unsafe { libc::getuid() } as i64))
}
fn getmygid(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Int(unsafe { libc::getgid() } as i64))
}
fn getmyinode(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Int(0))
}
fn getlastmod(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    use std::os::unix::ffi::OsStrExt;
    let p = vm.script_path.clone();
    let m = std::fs::metadata(std::ffi::OsStr::from_bytes(&p))
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);
    Ok(m.map(Value::Int).unwrap_or(Value::Bool(false)))
}

fn getenv(vm: &mut Vm, a: &mut Args) -> R<Value> {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    match a.opt_str(0) {
        None => {
            let mut out = PhpArray::new();
            for (k, v) in std::env::vars_os() {
                out.insert(
                    Key::from_bytes(k.as_bytes()),
                    Value::Str(PhpStr::from(v.into_vec())),
                );
            }
            Ok(Value::Array(out))
        }
        Some(n) => {
            let _ = vm;
            match std::env::var_os(std::ffi::OsStr::from_bytes(&n)) {
                Some(v) => Ok(Value::Str(PhpStr::from(v.into_vec()))),
                None => Ok(Value::Bool(false)),
            }
        }
    }
}

fn putenv(vm: &mut Vm, a: &mut Args) -> R<Value> {
    use std::os::unix::ffi::OsStrExt;
    let s = a.str(0);
    if s.is_empty() {
        return vm.value_error("putenv(): Argument #1 ($assignment) must have a valid syntax");
    }
    match s.iter().position(|&c| c == b'=') {
        Some(0) => vm.value_error("putenv(): Argument #1 ($assignment) must have a valid syntax"),
        Some(p) => {
            // SAFETY: the engine is single-threaded.
            unsafe {
                std::env::set_var(
                    std::ffi::OsStr::from_bytes(&s[..p]),
                    std::ffi::OsStr::from_bytes(&s[p + 1..]),
                );
            }
            Ok(Value::Bool(true))
        }
        None => {
            unsafe { std::env::remove_var(std::ffi::OsStr::from_bytes(&s)) };
            Ok(Value::Bool(true))
        }
    }
}

fn set_time_limit(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Bool(true))
}

/// Returns the previous setting and updates ignore_user_abort (test
/// general_functions/bug72300.phpt).
fn ignore_user_abort(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let old = vm
        .get_ini(b"ignore_user_abort")
        .map(|v| goro_runtime::convert::str_to_int(&v))
        .unwrap_or(0);
    if let Some(Value::Bool(b)) = a.get(0) {
        vm.set_ini(b"ignore_user_abort", if *b { b"1" } else { b"0" });
    }
    Ok(Value::Int(old))
}

fn sys_get_temp_dir(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    if let Some(v) = vm.get_ini(b"sys_temp_dir")
        && !v.is_empty()
    {
        let mut v = v.to_vec();
        while v.len() > 1 && v.ends_with(b"/") {
            v.pop();
        }
        return Ok(Value::Str(PhpStr::from(v)));
    }
    use std::os::unix::ffi::OsStringExt;
    let t = std::env::var_os("TMPDIR")
        .map(|v| v.into_vec())
        .unwrap_or_else(|| b"/tmp".to_vec());
    let mut t = t;
    while t.len() > 1 && t.ends_with(b"/") {
        t.pop();
    }
    Ok(Value::Str(PhpStr::from(t)))
}

fn microtime(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    if a.bool_or(0, false) {
        return Ok(Value::Float(d.as_secs_f64()));
    }
    let usec = d.subsec_micros() as f64 / 1e6;
    Ok(Value::Str(PhpStr::from(format!(
        "{:.8} {}",
        usec,
        d.as_secs()
    ))))
}

fn hrtime(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let mut ts: libc::timespec = unsafe { std::mem::zeroed() };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    if a.bool_or(0, false) {
        return Ok(Value::Int(
            ts.tv_sec as i64 * 1_000_000_000 + ts.tv_nsec as i64,
        ));
    }
    Ok(Value::Array(PhpArray::from_values([
        Value::Int(ts.tv_sec as i64),
        Value::Int(ts.tv_nsec as i64),
    ])))
}

fn usleep(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = a.int(0);
    if n < 0 {
        return vm.value_error(
            "usleep(): Argument #1 ($microseconds) must be greater than or equal to 0",
        );
    }
    vm.out.flush();
    std::thread::sleep(std::time::Duration::from_micros(n as u64));
    Ok(Value::Null)
}

fn sleep(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = a.int(0);
    if n < 0 {
        return vm
            .value_error("sleep(): Argument #1 ($seconds) must be greater than or equal to 0");
    }
    vm.out.flush();
    std::thread::sleep(std::time::Duration::from_secs(n as u64));
    Ok(Value::Int(0))
}

fn time_nanosleep(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.int(0);
    let ns = a.int(1);
    if s < 0 {
        return vm.value_error(
            "time_nanosleep(): Argument #1 ($seconds) must be greater than or equal to 0",
        );
    }
    if ns < 0 {
        return vm.value_error(
            "time_nanosleep(): Argument #2 ($nanoseconds) must be greater than or equal to 0",
        );
    }
    std::thread::sleep(std::time::Duration::new(s as u64, ns as u32));
    Ok(Value::Bool(true))
}

fn uniqid(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let prefix = a.str(0);
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    std::thread::sleep(std::time::Duration::from_micros(1));
    let mut s = prefix.to_vec();
    s.extend_from_slice(format!("{:08x}{:05x}", d.as_secs(), d.subsec_micros()).as_bytes());
    if a.bool_or(1, false) {
        // PHP manual: uniqid more_entropy adds a combined LCG value, printed
        // as "%.8F" of a number below 10 (test uniqid_basic.phpt).
        let mut b = [0u8; 8];
        super::math::os_random(&mut b);
        let f = (u64::from_le_bytes(b) >> 11) as f64 / (1u64 << 53) as f64 * 10.0;
        s.extend_from_slice(format!("{:.8}", f).as_bytes());
    }
    Ok(Value::Str(PhpStr::from(s)))
}

fn headers_sent(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Bool(vm.out.written > 0))
}

fn http_response_code(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Bool(false))
}

fn setlocale(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let l = a.val(1);
    let s = match &l {
        Value::Str(s) => s.clone(),
        Value::Array(arr) => match arr.iter().next() {
            Some((_, v)) => v.as_str().cloned().unwrap_or_default(),
            None => PhpStr::empty(),
        },
        _ => PhpStr::empty(),
    };
    if s.is_empty() || s.as_bytes() == b"0" || s.as_bytes() == b"C" || s.as_bytes() == b"POSIX" {
        return Ok(Value::Str(PhpStr::from("C")));
    }
    if s.as_bytes().starts_with(b"C.") || s.as_bytes() == b"en_US.UTF-8" {
        return Ok(Value::Str(s));
    }
    Ok(Value::Bool(false))
}

fn get_included_files(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let mut out = PhpArray::new();
    let _ = out.push(Value::Str(vm.script_path.clone()));
    for f in &vm.included {
        if *f != vm.script_path {
            let _ = out.push(Value::Str(f.clone()));
        }
    }
    Ok(Value::Array(out))
}

fn gethostname(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let (_, n, _, _, _) = uname();
    Ok(Value::Str(PhpStr::from(n)))
}

fn get_current_user(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let u = std::env::var("USER").unwrap_or_else(|_| "root".into());
    Ok(Value::Str(PhpStr::from(u)))
}

fn get_resources(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Array(PhpArray::new()))
}

fn clone_fn(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let v = a.val(0);
    if a.has(1) {
        let w = a.val(1);
        return vm.clone_with(v, w);
    }
    vm.clone_value(v)
}

/// Constants known only at run time: PHP_BINARY (the running executable)
/// and PHP_BUILD_DATE ("Mmm dd yyyy hh:mm:ss", the executable's date).
fn core_init(vm: &mut Vm) {
    use std::os::unix::ffi::OsStrExt;
    let exe = std::env::current_exe().ok();
    let bin = exe
        .as_ref()
        .map(|p| p.as_os_str().as_bytes().to_vec())
        .unwrap_or_default();
    vm.constants
        .insert(PhpStr::intern(b"PHP_BINARY"), Value::Str(PhpStr::from(bin)));
    let secs = exe
        .and_then(|p| std::fs::metadata(p).ok())
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    // Civil date from days since 1970-01-01 (proleptic Gregorian).
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    const MON: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let s = format!(
        "{} {:>2} {} {:02}:{:02}:{:02}",
        MON[(m - 1) as usize],
        d,
        y,
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    );
    vm.constants.insert(
        PhpStr::intern(b"PHP_BUILD_DATE"),
        Value::Str(PhpStr::from(s)),
    );
}
