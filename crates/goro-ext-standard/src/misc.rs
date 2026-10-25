use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::{ClassEntry, PhpObject, PropertyDef, Visibility};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use std::cell::RefCell;
use std::rc::Rc;

/// Deep-clone a Value so that arrays are independent (PHP value semantics).
/// Objects and resources are still shared (PHP object semantics).
/// References are preserved as-is to handle circular references safely.
fn deep_clone_value(val: &Value) -> Value {
    deep_clone_value_depth(val, 0)
}

fn deep_clone_value_depth(val: &Value, depth: usize) -> Value {
    if depth > 256 {
        return val.clone();
    }
    match val {
        Value::Array(arr) => {
            // Detect if we can borrow - if borrow_mut would fail (already borrowed mutably),
            // just return as is to avoid infinite recursion
            match arr.try_borrow() {
                Ok(src) => {
                    let mut new_arr = PhpArray::new();
                    for (k, v) in src.iter() {
                        new_arr.set(k.clone(), deep_clone_value_depth(v, depth + 1));
                    }
                    Value::Array(Rc::new(RefCell::new(new_arr)))
                }
                Err(_) => val.clone(),
            }
        }
        // Preserve references as-is - cloning a reference deep-clones the
        // underlying Value would defeat the purpose of references
        Value::Reference(_) => val.clone(),
        _ => val.clone(),
    }
}

/// Validate that an argument is an array and throw TypeError if not.
/// Returns Ok(()) if valid array (or reference to array), Err with VmError if not.
fn require_array_arg(
    vm: &mut Vm,
    val: &Value,
    func_name: &str,
    param_name: &str,
    param_num: u32,
) -> Result<(), VmError> {
    require_array_arg_inner(vm, val, func_name, Some(param_name), param_num)
}

/// Like require_array_arg but without parameter name in error message (for variadic args)
fn require_array_arg_variadic(
    vm: &mut Vm,
    val: &Value,
    func_name: &str,
    param_num: u32,
) -> Result<(), VmError> {
    require_array_arg_inner(vm, val, func_name, None, param_num)
}

/// Like Vm::value_type_name but returns "resource" for registered resource ids
/// (since resources are stored as Value::Long internally).
/// Skip ids 1-3 since those are pre-registered as STDIN/STDOUT/STDERR and
/// would falsely classify integer literals 1, 2, 3 as resources.
fn arg_type_name(val: &Value) -> String {
    match val {
        Value::Long(n) if *n > 3 && goro_core::value::is_registered_resource(*n) => {
            "resource".to_string()
        }
        Value::Reference(r) => arg_type_name(&r.borrow()),
        _ => Vm::value_type_name(val),
    }
}

fn require_array_arg_inner(
    vm: &mut Vm,
    val: &Value,
    func_name: &str,
    param_name: Option<&str>,
    param_num: u32,
) -> Result<(), VmError> {
    let make_msg = |type_name: &str| -> String {
        if let Some(name) = param_name {
            format!(
                "{}(): Argument #{} (${}) must be of type array, {} given",
                func_name, param_num, name, type_name
            )
        } else {
            format!(
                "{}(): Argument #{} must be of type array, {} given",
                func_name, param_num, type_name
            )
        }
    };
    match val {
        Value::Array(_) => Ok(()),
        Value::Reference(r) => {
            let inner = r.borrow();
            if matches!(&*inner, Value::Array(_)) {
                Ok(())
            } else {
                let type_name = arg_type_name(&inner);
                let msg = make_msg(&type_name);
                let exc = vm.create_exception(b"TypeError", &msg, 0);
                vm.current_exception = Some(exc);
                Err(VmError {
                    message: msg,
                    line: vm.current_line,
                })
            }
        }
        _ => {
            let type_name = arg_type_name(val);
            let msg = make_msg(&type_name);
            let exc = vm.create_exception(b"TypeError", &msg, 0);
            vm.current_exception = Some(exc);
            Err(VmError {
                message: msg,
                line: vm.current_line,
            })
        }
    }
}

/// Normalize a namespaced constant name: lowercase the namespace prefix, keep constant name as-is.
/// For "NS1\ns2\const1", returns "ns1\ns2\const1" (namespace lowered, const name preserved).
/// For "CONST" (no namespace), returns "CONST" as-is.
fn normalize_ns_const_name(name: &[u8]) -> Vec<u8> {
    if let Some(pos) = name.iter().rposition(|&b| b == b'\\') {
        let mut result = Vec::with_capacity(name.len());
        // Lowercase the namespace prefix
        for &b in &name[..pos] {
            result.push(b.to_ascii_lowercase());
        }
        // Keep the separator and constant name as-is
        result.extend_from_slice(&name[pos..]);
        result
    } else {
        name.to_vec()
    }
}

pub fn register(vm: &mut Vm) {
    register_std_streams();
    xp_socket::install_hooks(vm);
    ftp_wrapper::register();
    vm.register_function(b"fnmatch", fnmatch_fn);
    for (n, v) in [
        ("FNM_NOESCAPE", libc::FNM_NOESCAPE),
        ("FNM_PATHNAME", libc::FNM_PATHNAME),
        ("FNM_PERIOD", libc::FNM_PERIOD),
        ("FNM_CASEFOLD", libc::FNM_CASEFOLD),
    ] {
        vm.constants
            .insert(n.as_bytes().to_vec(), Value::Long(v as i64));
    }
    vm.resource_shutdown_hooks.push(shutdown_filtered_streams);
    vm.unserialize_value_hook = Some(crate::serialize::unserialize_value_hook);
    vm.serialize_scope_hook = Some(crate::serialize::serialize_scope_hook);
    vm.unserialize_scope_hook = Some(crate::serialize::unserialize_scope_hook);
    // Built-in php_user_filter base class (user_filters.stub.php). Compiled
    // from its PHP declaration so methods carry real signatures for
    // inheritance checks and reflection.
    {
        let src: &[u8] = b"<?php class php_user_filter {
    public string $filtername = \"\";
    public mixed $params = \"\";
    public $stream = null;
    public function filter($in, $out, &$consumed, bool $closing): int { return 0; }
    public function onCreate(): bool { return true; }
    public function onClose(): void {}
}
final class StreamBucket {
    public $bucket;
    public string $data;
    public int $datalen;
    public int $dataLength;
}";
        let mut lexer = goro_parser::Lexer::new(src);
        let tokens = lexer.tokenize();
        let mut parser = goro_parser::Parser::new(tokens);
        let mut done = false;
        if let Ok(program) = parser.parse() {
            let compiler = goro_core::compiler::Compiler::new();
            if let Ok((_, classes, _)) = compiler.compile(&program) {
                for mut cls in classes {
                    // (internal classes: no declaring file or lines)
                    cls.start_line = 0;
                    cls.end_line = 0;
                    if cls.name.eq_ignore_ascii_case(b"php_user_filter") {
                        cls.filename = None;
                        vm.classes.insert(b"php_user_filter".to_vec(), cls);
                        done = true;
                    } else if cls.name.eq_ignore_ascii_case(b"StreamBucket") {
                        cls.filename = None;
                        vm.classes.insert(b"streambucket".to_vec(), cls);
                    }
                }
            }
        }
        if !done {
            let mut pufc = ClassEntry::new(b"php_user_filter".to_vec());
            for p in [&b"filtername"[..], b"params", b"stream"] {
                pufc.properties.push(PropertyDef {
                    name: p.to_vec(),
                    default: Value::Null,
                    is_static: false,
                    is_readonly: false,
                    visibility: Visibility::Public,
                    set_visibility: None,
                    declaring_class: b"php_user_filter".to_vec(),
                    property_type: None,
                    has_get_hook: false,
                    has_set_hook: false,
                    is_virtual: false,
                    is_abstract: false,
                    is_final: false,
                    attributes: Vec::new(),
                    doc_comment: None,
                });
            }
            vm.classes.insert(b"php_user_filter".to_vec(), pufc);
        }
    }
    // RequestParseBodyException extends \Exception
    {
        let mut cls = ClassEntry::new(b"RequestParseBodyException".to_vec());
        cls.parent = Some(b"Exception".to_vec());
        vm.classes
            .insert(b"requestparsebodyexception".to_vec(), cls);
    }
    // Error handling
    vm.register_function(b"error_reporting", error_reporting);
    vm.register_function(b"set_error_handler", set_error_handler);
    vm.register_function(b"restore_error_handler", restore_error_handler);
    vm.register_function(b"error_get_last", error_get_last_fn);
    vm.register_function(b"error_clear_last", error_clear_last_fn);
    vm.register_function(b"set_exception_handler", set_exception_handler);
    vm.register_function(b"restore_exception_handler", restore_exception_handler);
    vm.register_function(b"get_error_handler", get_error_handler);
    vm.register_function(b"get_exception_handler", get_exception_handler);
    vm.register_function(b"trigger_error", trigger_error);
    vm.register_function(b"user_error", trigger_error);

    // Constants
    vm.register_function(b"define", define);
    // ZEND_DECLARE_CONST: what a `const X = ...;` statement runs (not a
    // call to define(): observers and disable_functions do not see it).
    vm.register_function(b"__declare_const", define);
    vm.register_function(b"constant", constant);

    // Output buffering
    vm.register_function(b"flush", flush_fn);

    // Function handling
    vm.register_function(b"func_num_args", func_num_args);
    vm.register_function(b"func_get_args", func_get_args);
    vm.register_function(b"func_get_arg", func_get_arg);
    vm.register_function(b"function_exists", function_exists);
    vm.register_function(b"is_callable", is_callable);
    vm.register_function(b"call_user_func", call_user_func_checked);
    vm.register_function(b"call_user_func_array", call_user_func_array_checked);

    // Array functions
    vm.register_function(b"array_push", array_push);
    vm.register_function(b"array_pop", array_pop);
    vm.register_function(b"array_shift", array_shift);
    vm.register_function(b"array_unshift", array_unshift);
    vm.register_function(b"array_keys", array_keys);
    vm.register_function(b"array_values", array_values);
    vm.register_function(b"array_merge", array_merge);
    vm.register_function(b"array_reverse", array_reverse);
    vm.register_function(b"array_flip", array_flip);
    vm.register_function(b"array_unique", array_unique);
    vm.register_function(b"array_slice", array_slice);
    vm.register_function(b"array_splice", array_splice);
    vm.register_function(b"array_search", array_search);
    vm.register_function(b"in_array", in_array);
    vm.register_function(b"array_filter", array_filter);
    vm.register_function(b"array_walk", array_walk);
    vm.register_function(b"array_reduce", array_reduce_fn);
    vm.register_function(b"array_combine", array_combine);
    vm.register_function(b"array_chunk", array_chunk);
    vm.register_function(b"array_pad", array_pad);
    vm.register_function(b"array_fill", array_fill);
    vm.register_function(b"array_fill_keys", array_fill_keys);
    vm.register_function(b"array_merge_recursive", array_merge_recursive);
    vm.register_function(b"sort", sort_fn);
    vm.register_function(b"rsort", rsort_fn);
    vm.register_function(b"asort", asort_fn);
    vm.register_function(b"arsort", arsort_fn);
    vm.register_function(b"ksort", ksort_fn);
    vm.register_function(b"krsort", krsort_fn);
    vm.register_function(b"shuffle", shuffle_fn);
    vm.register_function(b"natsort", natsort_fn);
    vm.register_function(b"natcasesort", natcasesort_fn);
    vm.register_function(b"range", range_fn);
    vm.register_function(b"current", current);
    vm.register_function(b"pos", current);
    vm.register_function(b"getrusage", getrusage_fn);
    vm.register_function(b"strptime", strptime_fn);
    vm.register_function(b"proc_nice", proc_nice_fn);
    vm.register_function(b"net_get_interfaces", net_get_interfaces_fn);
    vm.register_function(b"sys_getloadavg", sys_getloadavg_fn);
    vm.register_function(b"next", next_fn);
    vm.register_function(b"prev", prev_fn);
    vm.register_function(b"reset", reset_fn);
    vm.register_function(b"end", end_fn);
    vm.register_function(b"key", key_fn);

    // Misc
    vm.register_function(b"ini_set", ini_set);
    vm.register_function(b"ini_alter", ini_set);
    vm.register_function(b"ini_get", ini_get);
    vm.register_function(b"ini_get_all", ini_get_all);
    vm.register_function(b"ini_restore", ini_restore);
    vm.register_function(b"set_time_limit", set_time_limit);
    vm.register_function(b"assert", php_assert);
    vm.register_function(b"class_exists", class_exists);
    vm.register_function(b"get_declared_classes", get_declared_classes);
    vm.register_function(b"get_declared_traits", get_declared_traits_fn);
    vm.register_function(b"get_declared_interfaces", get_declared_interfaces_fn);
    vm.register_function(b"method_exists", method_exists);
    vm.register_function(b"is_object", is_object);
    vm.register_function(b"debug_zval_refcount", debug_zval_refcount);
    vm.register_function(b"compact", compact);
    vm.register_function(b"extract", extract_fn);
    vm.register_function(b"array_column", array_column);
    vm.register_function(b"array_count_values", array_count_values);
    vm.register_function(b"array_rand", array_rand);
    vm.register_function(b"register_shutdown_function", register_shutdown_fn);
    vm.register_function(b"interface_exists", interface_exists_fn);
    vm.register_function(b"trait_exists", trait_exists_fn);
    vm.register_function(b"enum_exists", enum_exists_fn);
    vm.register_function(b"exit", exit_fn);
    vm.register_function(b"die", exit_fn);
    vm.register_function(b"gc_collect_cycles", gc_collect_fn);
    vm.register_function(b"gc_enabled", gc_enabled_fn);
    vm.register_function(b"gc_disable", gc_disable_fn);
    vm.register_function(b"gc_enable", gc_enable_fn);
    vm.register_function(b"gc_status", gc_status_fn);
    vm.register_function(b"gc_mem_caches", gc_mem_caches_fn);
    vm.constants
        .entry(b"zend.enable_gc".to_vec())
        .or_insert(Value::String(PhpString::from_bytes(b"1")));
    vm.constants
        .entry(b"zend.exception_string_param_max_len".to_vec())
        .or_insert(Value::String(PhpString::from_bytes(b"15")));
    vm.constants
        .entry(b"zend.exception_ignore_args".to_vec())
        .or_insert(Value::String(PhpString::from_bytes(b"0")));
    vm.ini_validators.insert(
        b"zend.exception_string_param_max_len".to_vec(),
        on_set_exception_string_param_max_len,
    );
    // Stack limit settings (0 = detected from the system).
    for k in [
        &b"zend.max_allowed_stack_size"[..],
        b"zend.reserved_stack_size",
    ] {
        vm.constants
            .entry(k.to_vec())
            .or_insert(Value::String(PhpString::from_bytes(b"0")));
    }
    vm.register_function(goro_core::gc::DESTRUCTOR_FIBER_FN, gc_destructor_fiber_fn);
    goro_core::gc::gc_request_startup();
    vm.register_function(b"debug_zval_dump", debug_zval_dump_fn);
    vm.register_function(b"get_class_methods", get_class_methods_fn);
    vm.register_function(b"get_object_vars", get_object_vars_fn);
    vm.register_function(b"get_class", get_class_fn);
    vm.register_function(b"serialize", crate::serialize::serialize_fn);
    vm.register_function(b"unserialize", crate::serialize::unserialize_fn);
    vm.register_function(b"memory_get_usage", memory_get_usage_fn);
    vm.register_function(b"memory_get_peak_usage", memory_get_peak_fn);
    vm.register_function(b"memory_reset_peak_usage", memory_reset_peak_usage_fn);
    vm.register_function(b"sleep", sleep_fn);
    vm.register_function(b"usleep", usleep_fn);
    vm.register_function(b"uniqid", uniqid_fn);
    vm.register_function(b"getenv", getenv_fn);
    vm.register_function(b"putenv", putenv_fn);
    // SPL autoload functions moved to goro-ext-spl
    vm.register_function(b"class_alias", class_alias_fn);
    vm.register_function(b"is_a", is_a_fn);
    vm.register_function(b"is_subclass_of", is_subclass_of_fn);
    vm.register_function(b"get_parent_class", get_parent_class_fn);
    vm.register_function(b"get_called_class", get_called_class_fn);
    vm.register_function_with_params(
        b"clone",
        goro_core::clone::clone_fn,
        &[b"object", b"withProperties"],
    );
    vm.register_function(b"get_defined_vars", get_defined_vars_fn);
    vm.register_function(b"get_defined_functions", get_defined_functions_fn);
    vm.register_function(b"get_defined_constants", get_defined_constants_fn);
    vm.register_function(b"get_class_vars", get_class_vars_fn);
    vm.register_function(b"array_first", array_first_fn);
    vm.register_function(b"array_last", array_last_fn);
    vm.register_function(b"array_key_first", array_key_first_fn);
    vm.register_function(b"array_key_last", array_key_last_fn);
    vm.register_function(b"array_is_list", array_is_list_fn);
    vm.register_function(b"usort", usort_fn);
    vm.register_function(b"uasort", uasort_fn);
    vm.register_function(b"uksort", uksort_fn);
    vm.register_function(b"array_change_key_case", array_change_key_case_fn);
    vm.register_function(b"array_all", array_all_fn);
    vm.register_function(b"array_any", array_any_fn);
    vm.register_function(b"array_multisort", array_multisort_fn);
    vm.register_function(b"exec", exec_fn);
    vm.register_function(b"system", system_fn);
    vm.register_function(b"shell_exec", shell_exec_fn);
    vm.register_function(b"passthru", passthru_fn);
    vm.register_function(b"fopen", ctx_fopen_fn_entry);
    goro_core::value::set_default_context_hook(default_context);
    vm.register_function(b"fclose", fclose_fn);
    goro_core::value::register_resource_type_dtor(b"stream", stream_resource_dtor);
    goro_core::value::register_resource_type_dtor(b"stream-context", stream_context_dtor);
    vm.register_function(b"fread", fread_fn);
    vm.register_function(b"fwrite", fwrite_fn);
    vm.register_function(b"fgets", fgets_fn);
    vm.register_function(b"feof", feof_fn);
    vm.register_function(b"rewind", rewind_fn);
    vm.register_function(b"fseek", fseek_fn);
    vm.register_function(b"ftell", ftell_fn);
    vm.register_function(b"fflush", fflush_fn);
    vm.register_function(b"fsync", fsync_fn);
    vm.register_function(b"fdatasync", fdatasync_fn);
    vm.register_function(b"readfile", ctx_readfile_fn_entry);
    vm.register_function(b"file", ctx_file_fn_entry);
    vm.register_function(b"lstat", lstat_fn);
    vm.register_function(b"is_executable", is_executable_fn);
    vm.register_function(b"tempnam", tempnam_fn3);
    vm.register_function(b"sys_get_temp_dir", sys_get_temp_dir_fn3);
    vm.register_function(b"mkdir", ctx_mkdir_fn_entry);
    vm.register_function(b"rmdir", ctx_rmdir_fn_entry);
    vm.register_function(b"glob", glob_fn);
    vm.register_function(b"scandir", ctx_scandir_fn_entry);
    vm.register_function(b"is_writeable", is_writable_fn);
    vm.register_function(b"filemtime", filemtime_fn);
    vm.register_function(b"fileatime", fileatime_fn);
    vm.register_function(b"filectime", filectime_fn);
    vm.register_function(b"fileinode", fileinode_fn);
    vm.register_function(b"fileowner", fileowner_fn);
    vm.register_function(b"filegroup", filegroup_fn);
    vm.register_function(b"chown", chown_fn);
    vm.register_function(b"chgrp", chgrp_fn);
    vm.register_function(b"lchown", lchown_fn);
    vm.register_function(b"lchgrp", lchgrp_fn);
    vm.register_function(b"fpassthru", fpassthru_fn);
    vm.register_function(b"fgetc", fgetc_fn);
    vm.register_function(b"flock", flock_fn);
    vm.register_function(b"fstat", fstat_fn);
    vm.register_function(b"stream_get_contents", stream_get_contents_fn);
    vm.register_function(b"fputs", fwrite_fn); // fputs is an alias for fwrite
    vm.register_function(b"linkinfo", linkinfo_fn);
    vm.register_function(b"parse_ini_file", parse_ini_file_fn);
    vm.register_function(b"header", header_fn);
    vm.register_function(b"header_remove", header_remove_fn);
    vm.register_function(b"headers_sent", headers_sent_fn);
    vm.register_function(b"http_response_code", http_response_code_fn);
    vm.register_function(b"getallheaders", getallheaders_fn);
    vm.register_function(b"apache_request_headers", getallheaders_fn);
    vm.register_function(b"apache_response_headers", apache_response_headers_fn);
    vm.register_function(b"setcookie", setcookie_fn);
    vm.register_function(b"setrawcookie", setrawcookie_fn);
    // SPL object functions moved to goro-ext-spl
    vm.register_function(b"forward_static_call", forward_static_call_fn);
    vm.register_function(b"forward_static_call_array", forward_static_call_array_fn);
    vm.register_function(b"phpversion", phpversion_fn);
    vm.register_function(b"php_uname", php_uname_fn);
    vm.register_function(b"php_sapi_name", php_sapi_name_fn);
    vm.register_function(b"defined", defined_fn);
    vm.register_function(b"zend_version", zend_version_fn);
    vm.register_function(b"extension_loaded", extension_loaded_fn);
    vm.register_function(b"get_loaded_extensions", get_loaded_extensions_fn);
    vm.register_function(b"get_extension_funcs", get_extension_funcs_fn);
    // iterator_to_array and iterator_count moved to goro-ext-spl
    vm.register_function(b"array_map", array_map);
    vm.register_function(b"key_exists", array_key_exists_fn2);
    vm.register_function(b"array_replace", array_replace_fn);
    vm.register_function(b"array_replace_recursive", array_replace_recursive_fn);
    vm.register_function(b"array_find", array_find_fn);
    vm.register_function(b"get_meta_tags", get_meta_tags_fn);
    for (name, v) in [
        (&b"STREAM_NOTIFY_RESOLVE"[..], 1),
        (b"STREAM_NOTIFY_CONNECT", 2),
        (b"STREAM_NOTIFY_AUTH_REQUIRED", 3),
        (b"STREAM_NOTIFY_MIME_TYPE_IS", 4),
        (b"STREAM_NOTIFY_FILE_SIZE_IS", 5),
        (b"STREAM_NOTIFY_REDIRECTED", 6),
        (b"STREAM_NOTIFY_PROGRESS", 7),
        (b"STREAM_NOTIFY_COMPLETED", 8),
        (b"STREAM_NOTIFY_FAILURE", 9),
        (b"STREAM_NOTIFY_AUTH_RESULT", 10),
        (b"STREAM_NOTIFY_SEVERITY_INFO", 0),
        (b"STREAM_NOTIFY_SEVERITY_WARN", 1),
        (b"STREAM_NOTIFY_SEVERITY_ERR", 2),
    ] {
        vm.constants.entry(name.to_vec()).or_insert(Value::Long(v));
    }
    vm.register_function(b"http_get_last_response_headers", |_vm, _args| {
        Ok(http_wrapper::last_response_headers())
    });
    vm.register_function(b"http_clear_last_response_headers", |_vm, _args| {
        http_wrapper::clear_last_response_headers();
        Ok(Value::Null)
    });
    vm.register_function(b"array_find_key", array_find_key_fn);
    vm.register_function(b"array_product", array_product_fn);
    vm.register_function(b"array_sum", array_sum_fn);
    vm.register_function(b"parse_ini_string", parse_ini_string_fn);
    vm.register_function(b"assert_options", assert_options_fn);
    vm.register_function(b"ftruncate", ftruncate_fn);
    vm.register_function(b"tmpfile", tmpfile_fn);
    vm.register_function(b"phpinfo", crate::info::phpinfo);
    vm.register_function(b"phpcredits", crate::info::phpcredits);
    vm.register_function(b"get_cfg_var", get_cfg_var_fn);
    vm.register_function(b"php_ini_loaded_file", php_ini_loaded_file_fn);
    vm.register_function(b"php_ini_scanned_files", php_ini_scanned_files_fn);
    vm.register_function(b"getmypid", getmypid_fn);
    vm.register_function(b"ftok", ftok_fn);
    vm.register_function(b"getmyuid", getmyuid_fn);
    vm.register_function(b"getlastmod", getlastmod_fn);
    vm.register_function(b"getmyinode", getmyinode_fn);
    vm.register_function(b"getmygid", getmygid_fn);
    vm.register_function(b"get_current_user", get_current_user_fn);
    // sizeof is an alias for count (registered in type_funcs.rs)

    // String extras
    vm.register_function(b"str_split", str_split);
    vm.register_function(b"number_format", number_format);
    // hex2bin and bin2hex are registered in strings.rs
    vm.register_function(b"base64_encode", base64_encode);
    vm.register_function(b"base64_decode", base64_decode);
    vm.register_function(b"urlencode", urlencode);
    vm.register_function(b"urldecode", urldecode);
    vm.register_function(b"rawurlencode", rawurlencode);
    vm.register_function(b"rawurldecode", rawurldecode);
    vm.register_function(b"parse_url", parse_url_fn);
    vm.register_function(b"http_build_query", http_build_query_fn);
    vm.register_function(b"parse_str", parse_str_fn);
    vm.register_function(b"escapeshellarg", escapeshellarg_fn);
    vm.register_function(b"escapeshellcmd", escapeshellcmd_fn);
    // htmlspecialchars/htmlentities/htmlspecialchars_decode/html_entity_decode/
    // get_html_translation_table are registered by the html module.
    vm.register_function(b"fprintf", fprintf_fn);
    vm.register_function(b"vfprintf", vfprintf_fn);
    // File stubs (return false/null for now)
    vm.register_function(b"file_exists", file_exists_fn);
    vm.register_function(b"is_file", is_file_fn);
    vm.register_function(b"is_dir", is_dir_fn);
    vm.register_function(b"is_readable", is_readable_fn);
    vm.register_function(b"is_writable", is_writable_fn);
    vm.register_function(b"getcwd", getcwd_fn);
    vm.register_function(b"chdir", chdir_fn);
    vm.register_function(b"chroot", chroot_fn);
    vm.register_function(b"filesize", filesize_fn);
    vm.register_function(b"touch", touch_fn);
    vm.register_function(b"realpath_cache_size", realpath_cache_size_fn);
    vm.register_function(b"realpath_cache_get", realpath_cache_get_fn);
    vm.register_function(b"is_link", is_link_fn);
    vm.register_function(b"stat", stat_fn);
    vm.register_function(b"is_numeric", is_numeric_fn);
    vm.register_function(b"clearstatcache", clearstatcache_fn);
    vm.register_function(b"array_walk_recursive", array_walk_recursive_fn);
    vm.register_function(b"fileperms", fileperms_fn);
    vm.register_function(b"filetype", filetype_fn);
    vm.register_function(b"opendir", ctx_opendir_fn_entry);
    vm.register_function(b"closedir", closedir_fn);
    vm.register_function(b"readdir", readdir_fn);
    vm.register_function(b"chmod", chmod_fn);
    vm.register_function(b"symlink", symlink_fn);
    vm.register_function(b"readlink", readlink_fn);
    vm.register_function(b"debug_backtrace", debug_backtrace_fn);
    vm.register_function(b"debug_print_backtrace", debug_print_backtrace_fn);
    vm.register_function(b"array_key_exists", array_key_exists_fn2);
    vm.register_function(b"set_include_path", set_include_path_fn);
    vm.register_function(b"get_include_path", get_include_path_fn);
    vm.register_function(b"restore_include_path", restore_include_path_fn);
    vm.register_function(b"get_resource_type", get_resource_type_fn);
    vm.register_function(b"link", link_fn);
    vm.register_function(b"unlink", ctx_unlink_fn2_entry);
    vm.register_function(b"rename", ctx_rename_fn2_entry);
    vm.register_function(b"copy", ctx_copy_fn2_entry);
    vm.register_function(b"file_get_contents", ctx_file_get_contents_fn_entry);
    vm.register_function(b"file_put_contents", ctx_file_put_contents_fn_entry);
    vm.register_function(b"realpath", realpath_fn);
    vm.register_function(b"dirname", dirname_fn);
    vm.register_function(b"basename", basename_fn);
    vm.register_function(b"pathinfo", pathinfo_fn);
    // class_implements, class_parents, class_uses moved to goro-ext-spl
    vm.register_function(b"str_increment", str_increment_fn);
    vm.register_function(b"get_included_files", get_included_files_fn);
    vm.register_function(b"get_required_files", get_included_files_fn);
    // iterator_apply moved to goro-ext-spl
    vm.register_function(b"property_exists", property_exists_fn);
    vm.register_function(b"get_mangled_object_vars", get_mangled_object_vars_fn);
    vm.register_function(b"get_resource_id", get_resource_id_fn);
    vm.register_function(b"hrtime", hrtime_fn);
    vm.register_function(b"gethostname", gethostname_fn);
    vm.register_function(b"umask", umask_fn);
    vm.register_function(b"getopt", getopt_fn);
    vm.register_function(b"proc_open", proc_open::proc_open);
    goro_core::value::register_resource_type_dtor(b"process", proc_open::proc_rsrc_dtor);
    vm.register_function(b"proc_close", proc_open::proc_close);
    vm.register_function(b"proc_get_status", proc_open::proc_get_status);
    vm.register_function(b"proc_terminate", proc_open::proc_terminate);
    vm.register_function(b"stream_set_blocking", xp_socket::stream_set_blocking);
    vm.register_function(b"socket_set_blocking", xp_socket::stream_set_blocking);
    vm.register_function(b"stream_set_timeout", xp_socket::stream_set_timeout);
    vm.register_function(b"socket_set_timeout", xp_socket::stream_set_timeout);
    vm.register_function(b"stream_set_write_buffer", stream_set_write_buffer_fn);
    vm.register_function(b"stream_set_read_buffer", stream_set_read_buffer_fn);
    vm.register_function(b"stream_copy_to_stream", stream_copy_to_stream_fn);
    vm.register_function(b"stream_filter_append", stream_filter_append_fn);
    vm.register_function(b"stream_filter_prepend", stream_filter_prepend_fn);
    vm.register_function(b"stream_filter_register", stream_filter_register_fn);
    vm.register_function(b"stream_filter_remove", stream_filter_remove_fn);
    // Bucket brigade helpers used by user filters.
    vm.register_function(
        b"stream_bucket_make_writeable",
        crate::stream_filters::stream_bucket_make_writeable,
    );
    vm.register_function(
        b"stream_bucket_append",
        crate::stream_filters::stream_bucket_append,
    );
    vm.register_function(
        b"stream_bucket_prepend",
        crate::stream_filters::stream_bucket_prepend,
    );
    vm.register_function(
        b"stream_bucket_new",
        crate::stream_filters::stream_bucket_new,
    );
    // Filter direction constants.
    vm.constants.insert(
        b"STREAM_FILTER_READ".to_vec(),
        Value::Long(crate::stream_filters::STREAM_FILTER_READ),
    );
    vm.constants.insert(
        b"STREAM_FILTER_WRITE".to_vec(),
        Value::Long(crate::stream_filters::STREAM_FILTER_WRITE),
    );
    vm.constants.insert(
        b"STREAM_FILTER_ALL".to_vec(),
        Value::Long(crate::stream_filters::STREAM_FILTER_ALL),
    );
    // User filter return code constants.
    vm.constants.insert(
        b"PSFS_PASS_ON".to_vec(),
        Value::Long(crate::stream_filters::PSFS_PASS_ON as i64),
    );
    vm.constants.insert(
        b"PSFS_FEED_ME".to_vec(),
        Value::Long(crate::stream_filters::PSFS_FEED_ME as i64),
    );
    vm.constants.insert(
        b"PSFS_ERR_FATAL".to_vec(),
        Value::Long(crate::stream_filters::PSFS_ERR_FATAL as i64),
    );
    vm.constants
        .insert(b"PSFS_FLAG_NORMAL".to_vec(), Value::Long(0));
    vm.constants
        .insert(b"PSFS_FLAG_FLUSH_INC".to_vec(), Value::Long(1));
    vm.constants
        .insert(b"PSFS_FLAG_FLUSH_CLOSE".to_vec(), Value::Long(2));
    vm.register_function(b"stream_context_create", stream_context_create_fn);
    vm.register_function(b"stream_context_set_option", stream_context_set_option_fn);
    vm.register_function(b"stream_context_set_options", stream_context_set_options_fn);
    vm.register_function(b"stream_context_get_options", stream_context_get_options_fn);
    vm.register_function(b"stream_context_set_params", stream_context_set_params_fn);
    vm.register_function(b"stream_context_get_params", stream_context_get_params_fn);
    vm.register_function(b"stream_context_get_default", stream_context_get_default_fn);
    vm.register_function(b"stream_context_set_default", stream_context_set_default_fn);
    vm.register_function(b"stream_is_local", stream_is_local_fn);
    vm.register_function(b"stream_get_meta_data", stream_get_meta_data_fn);
    vm.register_function(b"stream_wrapper_register", stream_wrapper_register_fn);
    vm.register_function(b"stream_register_wrapper", stream_wrapper_register_fn);
    vm.register_function(b"stream_wrapper_unregister", stream_wrapper_unregister_fn);
    vm.register_function(b"stream_wrapper_restore", stream_wrapper_restore_fn);
    vm.register_function(b"stream_get_wrappers", stream_get_wrappers_fn);
    vm.register_function(b"stream_get_filters", stream_get_filters_fn);
    vm.register_function(b"stream_get_transports", stream_get_transports_fn);
    vm.register_function(b"stream_socket_client", ctx_stream_socket_client_entry);
    vm.register_function(b"stream_socket_server", ctx_stream_socket_server_entry);
    vm.register_function(b"stream_socket_get_name", xp_socket::stream_socket_get_name);
    vm.register_function(b"stream_select", xp_socket::stream_select);
    vm.register_function(b"stream_socket_pair", xp_socket::stream_socket_pair);
    vm.register_function(b"stream_get_line", stream_get_line_fn);
    vm.register_function(b"stream_set_chunk_size", stream_set_chunk_size_fn);
    vm.register_function(b"stream_socket_recvfrom", xp_socket::stream_socket_recvfrom);
    vm.register_function(b"stream_socket_sendto", xp_socket::stream_socket_sendto);
    vm.register_function(b"stream_socket_shutdown", xp_socket::stream_socket_shutdown);
    vm.register_function(
        b"stream_socket_enable_crypto",
        stream_socket_enable_crypto_fn,
    );
    vm.register_function(b"headers_list", headers_list_fn);
    vm.register_function(b"dir", ctx_dir_fn_entry);
    dir_class::register(vm);
    // main.c mail.* INI defaults
    for (k, v) in [
        (&b"mail.cr_lf_mode"[..], &b"crlf"[..]),
        (b"mail.add_x_header", b"0"),
        (b"mail.mixed_lf_and_crlf", b"0"),
    ] {
        vm.constants
            .entry(k.to_vec())
            .or_insert_with(|| Value::String(PhpString::from_bytes(v)));
    }
    vm.register_function(b"popen", popen_fn);
    vm.register_function(b"pclose", pclose_fn);
    vm.register_function(b"rewinddir", rewinddir_fn);
    vm.register_function(b"error_log", error_log_fn);
    vm.register_function(b"disk_free_space", disk_free_space_fn);
    vm.register_function(b"diskfreespace", diskfreespace_fn);
    vm.register_function(b"disk_total_space", disk_total_space_fn);
    vm.register_function(b"get_resources", get_resources_fn);
    vm.register_function(b"closelog", closelog_fn);
    vm.register_function(b"openlog", openlog_fn);
    vm.register_function(b"syslog", syslog_fn);
    vm.register_function(b"fsockopen", fsockopen_fn);
    vm.register_function(b"pfsockopen", pfsockopen_fn);
    vm.register_function(b"register_tick_function", register_tick_function_fn);
    vm.register_function(b"unregister_tick_function", unregister_tick_function_fn);
    vm.register_function(b"get_browser", crate::browscap::get_browser);
    vm.register_function(b"stream_socket_accept", xp_socket::stream_socket_accept);
    vm.register_function(b"stream_isatty", stream_isatty_fn);
    vm.register_function(b"get_headers", |vm, args| {
        Ok(http_wrapper::get_headers(vm, args))
    });
    vm.register_function(b"cli_set_process_title", cli_set_process_title_fn);
    vm.register_function(b"cli_get_process_title", cli_get_process_title_fn);
    vm.register_function(b"ini_parse_quantity", ini_parse_quantity_fn);
    vm.register_function(b"dl", dl_fn);
    vm.register_function(b"header_register_callback", header_register_callback_fn);
    vm.register_function(b"token_get_all", token_get_all_fn);
    vm.register_function(b"token_name", token_name_fn);
    vm.register_function(b"request_parse_body", request_parse_body_fn);
    vm.register_function(b"ignore_user_abort", ignore_user_abort_fn);
    vm.register_function(b"connection_aborted", connection_aborted_fn);
    vm.register_function(b"connection_status", connection_status_fn);
    // get_html_translation_table is registered by strings::register with proper flag handling
    vm.register_function(b"time_nanosleep", time_nanosleep_fn);
    vm.register_function(b"time_sleep_until", time_sleep_until_fn);
    vm.register_function(b"socket_get_status", stream_get_meta_data_fn); // alias
    vm.register_function(b"set_file_buffer", set_file_buffer_fn);
    vm.register_function(
        b"stream_resolve_include_path",
        stream_resolve_include_path_fn,
    );
    vm.register_function(b"stream_supports_lock", stream_supports_lock_fn);

    // Regex functions are now in the regex module (regex.rs)
}

// === Error handling ===

fn error_reporting(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let old = match vm.error_reporting {
        goro_core::errdisplay::ER_SILENT => 0,
        er => er,
    };
    if let Some(level) = args.first().map(|v| v.deref()) {
        if !matches!(level, Value::Null | Value::Undef) {
            let strict = vm.caller_strict_types;
            let line = vm.current_line;
            match vm.zpp_coerce_scalar(
                goro_core::arginfo::T_INT | goro_core::arginfo::T_NULL,
                &level,
                strict,
                line,
            ) {
                Ok(Some(v)) => {
                    // zend_alter_ini_entry_chars(): the INI entry changes too
                    // (restored at request end / by ini_restore()).
                    let n = v.to_long();
                    vm.error_reporting = n;
                    ini_remember(vm, b"error_reporting");
                    vm.constants.insert(
                        b"error_reporting".to_vec(),
                        Value::String(PhpString::from_string(n.to_string())),
                    );
                }
                Ok(None) => {
                    return Err(throw_type_error_msg(
                        vm,
                        format!(
                            "error_reporting(): Argument #1 ($error_level) must be of type ?int, {} given",
                            Vm::value_type_name(&level)
                        ),
                    ));
                }
                Err(()) => {
                    return Err(VmError {
                        message: vm.uncaught_exception_message(),
                        line,
                    });
                }
            }
        }
    }
    Ok(Value::Long(old))
}

fn set_error_handler(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let handler = args.first().map(|v| v.deref()).unwrap_or(Value::Null);
    if !matches!(handler, Value::Null) {
        let (ok, _, err) = check_callable(vm, &handler, false);
        if vm.current_exception.is_some() {
            return Err(VmError {
                message: "Uncaught exception".into(),
                line: vm.current_line,
            });
        }
        if !ok {
            return Err(throw_type_error_msg(
                vm,
                format!(
                    "set_error_handler(): Argument #1 ($callback) must be a valid callback or null, {}",
                    err.unwrap_or_default()
                ),
            ));
        }
    }
    let mask = args
        .get(1)
        .map(|v| v.to_long())
        .unwrap_or(goro_core::errdisplay::E_ALL);
    let prev = vm.error_handler.take();
    vm.error_handler_stack
        .push((prev.clone(), vm.error_handler_mask));
    if !matches!(handler, Value::Null) {
        vm.error_handler = Some(handler);
        vm.error_handler_mask = mask;
    }
    Ok(prev.unwrap_or(Value::Null))
}

fn restore_error_handler(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    match vm.error_handler_stack.pop() {
        Some((h, mask)) => {
            vm.error_handler = h;
            vm.error_handler_mask = mask;
        }
        None => vm.error_handler = None,
    }
    Ok(Value::True)
}

fn set_exception_handler(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let handler_opt = args.first();
    // Validate callback (PHP throws a TypeError for non-callable callbacks since 8.0)
    if let Some(handler) = handler_opt {
        if !matches!(handler, Value::Null) {
            if let Some(err) = validate_exception_handler_callback(vm, handler) {
                let exc = vm.throw_type_error(err.clone());
                vm.current_exception = Some(exc);
                return Err(VmError {
                    message: err,
                    line: vm.current_line,
                });
            }
        }
    }
    let prev = vm.exception_handler.take();
    vm.exception_handler_stack.push(prev.clone());
    if let Some(handler) = handler_opt {
        if !matches!(handler, Value::Null) {
            vm.exception_handler = Some(handler.clone());
        }
    }
    Ok(prev.unwrap_or(Value::Null))
}

/// Validate a callback argument for set_exception_handler. Returns Some(error message)
/// if the callback is invalid (PHP throws TypeError); None if it looks OK.
fn validate_exception_handler_callback(vm: &mut Vm, callback: &Value) -> Option<String> {
    callable_error(vm, callback).map(|e| {
        format!(
            "set_exception_handler(): Argument #1 ($callback) must be a valid callback or null, {}",
            e
        )
    })
}

fn restore_exception_handler(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    vm.exception_handler = vm.exception_handler_stack.pop().unwrap_or(None);
    Ok(Value::True)
}

fn get_error_handler(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(vm.error_handler.clone().unwrap_or(Value::Null))
}

fn get_exception_handler(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(vm.exception_handler.clone().unwrap_or(Value::Null))
}

fn trigger_error(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    use goro_core::errdisplay::{E_USER_DEPRECATED, E_USER_ERROR, E_USER_NOTICE, E_USER_WARNING};
    // (the message's bytes are displayed and passed on as they are)
    let message = goro_core::errdisplay::raw_error_string(
        args.first()
            .unwrap_or(&Value::Null)
            .to_php_string()
            .as_bytes(),
    );
    let error_type = args.get(1).map(|v| v.to_long()).unwrap_or(E_USER_NOTICE);
    let line = vm.current_line;
    match error_type {
        E_USER_ERROR => {
            vm.emit_deprecated_at(
                "Passing E_USER_ERROR to trigger_error() is deprecated since 8.4, throw an exception or call exit with a string message instead",
                line,
            );
            if vm.current_exception.is_some() {
                return Ok(Value::Null);
            }
        }
        E_USER_WARNING | E_USER_NOTICE | E_USER_DEPRECATED => {}
        _ => {
            return Err(throw_value_error_msg(
                vm,
                "trigger_error(): Argument #2 ($error_level) must be one of E_USER_ERROR, E_USER_WARNING, E_USER_NOTICE, or E_USER_DEPRECATED",
            ));
        }
    }
    if vm.call_user_error_handler(error_type, &message, line) {
        return Ok(Value::True);
    }
    let file = vm.current_file.clone();
    vm.error_cb(error_type, &message, &file, line);
    if error_type == E_USER_ERROR {
        // php_error_cb() bails out after a fatal error.
        vm.fatal_emitted = true;
        return Err(VmError { message, line });
    }
    Ok(Value::True)
}

// === Constants ===

fn define(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let name_val = args.first().unwrap_or(&Value::Null);
    // Check type of first argument
    match name_val {
        Value::Array(_) | Value::Object(_) => {
            let type_name = match name_val {
                Value::Array(_) => "array",
                Value::Object(obj) => {
                    let _ = obj;
                    "object" // simplified
                }
                _ => "unknown",
            };
            let msg = format!(
                "define(): Argument #1 ($constant_name) must be of type string, {} given",
                type_name
            );
            let exc = vm.create_exception(b"TypeError", &msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
        _ => {}
    }
    let name = name_val.to_php_string();
    let value = args.get(1).cloned().unwrap_or(Value::Null);
    let name_bytes = name.as_bytes().to_vec();
    // Check if :: is in the name (not allowed)
    let name_str_check = name.to_string_lossy();
    if name_str_check.contains("::") {
        let msg = "define(): Argument #1 ($constant_name) cannot be a class constant";
        let exc = vm.create_exception(b"ValueError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg.to_string(),
            line: vm.current_line,
        });
    }
    if args.get(2).is_some_and(|v| v.to_bool()) {
        vm.emit_warning("define(): Argument #3 ($case_insensitive) is ignored since declaration of case-insensitive constants is no longer supported");
    }
    // Check if it's a built-in keyword constant (TRUE, FALSE, NULL)
    let name_upper: Vec<u8> = name_bytes.iter().map(|b| b.to_ascii_uppercase()).collect();
    // zend_register_constant(): __COMPILER_HALT_OFFSET__ is reserved.
    if name_upper == b"TRUE"
        || name_upper == b"FALSE"
        || name_upper == b"NULL"
        || name_bytes == b"__COMPILER_HALT_OFFSET__"
    {
        let name_str = name.to_msg_string();
        vm.emit_warning(&format!(
            "Constant {} already defined, this will be an error in PHP 9",
            name_str
        ));
        return Ok(Value::False);
    }
    // Normalize namespace part (lowercase) while keeping constant name case-sensitive
    let normalized = normalize_ns_const_name(&name_bytes);
    // Check if constant is already defined
    if vm.constants.contains_key(&normalized) {
        let name_str = name.to_msg_string();
        vm.emit_warning(&format!(
            "Constant {} already defined, this will be an error in PHP 9",
            name_str
        ));
        return Ok(Value::False);
    }
    // copy_constant_array(): references are dropped; a recursive array
    // is rejected (validate_constant_array_argument()).
    let value = match value.deref() {
        Value::Array(a) => {
            let mut stack = Vec::new();
            match const_array_copy(&a, &mut stack) {
                Some(v) => v,
                None => {
                    let msg = "define(): Argument #2 ($value) cannot be a recursive array";
                    let exc = vm.create_exception(b"ValueError", msg, 0);
                    vm.current_exception = Some(exc);
                    return Err(VmError {
                        message: msg.to_string(),
                        line: vm.current_line,
                    });
                }
            }
        }
        _ => value,
    };
    let file = vm.current_file.to_string();
    vm.constant_files.insert(normalized.clone(), file);
    vm.user_constant_names
        .push((normalized.clone(), name_bytes.to_vec()));
    vm.constants.insert(normalized, value);
    Ok(Value::True)
}

/// A constant's copy of array `a`: references replaced by their values;
/// None when the array contains itself.
fn const_array_copy(
    a: &Rc<RefCell<PhpArray>>,
    stack: &mut Vec<*const RefCell<PhpArray>>,
) -> Option<Value> {
    let p = Rc::as_ptr(a);
    if stack.contains(&p) {
        return None;
    }
    stack.push(p);
    let mut out = PhpArray::new();
    let entries: Vec<(ArrayKey, Value)> = a
        .borrow()
        .iter()
        .map(|(k, v)| (k.clone(), v.deref()))
        .collect();
    for (k, v) in entries {
        let v = match v {
            Value::Array(inner) => const_array_copy(&inner, stack)?,
            v => v,
        };
        out.set(k, v);
    }
    stack.pop();
    Some(Value::Array(Rc::new(RefCell::new(out))))
}

fn constant(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let name = args.first().unwrap_or(&Value::Null).to_php_string();
    let name_str = name.to_msg_string();
    // Check for class constants (Class::CONST)
    if let Some(pos) = name_str.find("::") {
        let class_name = &name_str[..pos];
        let const_name = &name_str[pos + 2..];
        let class_name_stripped = class_name.strip_prefix('\\').unwrap_or(class_name);
        let mut class_lower: Vec<u8> = class_name_stripped
            .as_bytes()
            .iter()
            .map(|b| b.to_ascii_lowercase())
            .collect();
        // self/parent/static of the caller (zend_get_class_constant_ex);
        // without a scope the lookup below reports the error.
        let scope = vm.current_class_scope().map(|s| s.to_ascii_lowercase());
        let resolved = match class_lower.as_slice() {
            b"self" => scope,
            b"parent" => scope.and_then(|s| vm.classes.get(&s).and_then(|c| c.parent.clone())),
            b"static" => vm.called_class_stack.last().cloned(),
            _ => None,
        };
        if let Some(r) = resolved {
            class_lower = r.to_ascii_lowercase();
        }
        if !vm.classes.contains_key(&class_lower) && !vm.is_known_builtin_class(&class_lower) {
            vm.try_autoload_class(class_name_stripped.as_bytes());
            if vm.current_exception.is_some() {
                return Err(VmError {
                    message: "Uncaught exception".into(),
                    line: vm.current_line,
                });
            }
            if !vm.classes.contains_key(&class_lower) {
                let msg = format!("Class \"{}\" not found", class_name_stripped);
                let line = vm.current_line;
                let exc = vm.create_exception(b"Error", &msg, line);
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg, line });
            }
        }
        if let Some(class) = vm.classes.get(&class_lower).cloned() {
            if class.is_trait && class.constants.contains_key(const_name.as_bytes()) {
                let msg = format!(
                    "Cannot access trait constant {}::{} directly",
                    goro_core::string::msg_str(&class.name),
                    const_name
                );
                let line = vm.current_line;
                let exc = vm.create_exception(b"Error", &msg, line);
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg, line });
            }
            // Visibility check: constant() honours protected/private the same as
            // the compiler does for direct Class::CONST references. Caller scope
            // is the innermost class on the scope stack (or its hierarchy).
            if let Some(meta) = class.constants_meta.get(const_name.as_bytes()) {
                use goro_core::object::Visibility;
                if meta.visibility != Visibility::Public {
                    let caller_lower: Option<Vec<u8>> = vm
                        .current_class_scope()
                        .map(|c| c.iter().map(|b| b.to_ascii_lowercase()).collect());
                    let target_lower = class_lower.clone();
                    let accessible = match (&meta.visibility, &caller_lower) {
                        (Visibility::Private, Some(c)) => c == &target_lower,
                        (Visibility::Protected, Some(c)) => {
                            c == &target_lower
                                || vm.class_extends(c, &target_lower)
                                || vm.class_extends(&target_lower, c)
                        }
                        _ => false,
                    };
                    if !accessible {
                        let vis_str = if meta.visibility == Visibility::Private {
                            "private"
                        } else {
                            "protected"
                        };
                        let class_display = goro_core::string::msg_str(&class.name).to_string();
                        let msg = format!(
                            "Cannot access {} constant {}::{}",
                            vis_str, class_display, const_name
                        );
                        let line = vm.current_line;
                        let exc = vm.create_exception(b"Error", &msg, line);
                        vm.current_exception = Some(exc);
                        return Err(VmError { message: msg, line });
                    }
                }
            }
            if let Some(val) = class.constants.get(const_name.as_bytes()) {
                // Check for #[\Deprecated] attribute on class constant
                if let Some(meta) = class.constants_meta.get(const_name.as_bytes()) {
                    if !meta.attributes.is_empty() {
                        // GH-17711: guard self-referential deprecation messages.
                        // Canonicalize through class.name so aliases share a key.
                        let canonical_class_lower: Vec<u8> =
                            class.name.iter().map(|b| b.to_ascii_lowercase()).collect();
                        let mut guard_key = canonical_class_lower;
                        guard_key.extend_from_slice(b"::");
                        guard_key.extend_from_slice(const_name.as_bytes());
                        if !vm.deprecation_in_progress.contains(&guard_key) {
                            vm.deprecation_in_progress.insert(guard_key.clone());
                            let dep_result = vm.get_deprecated_message(&meta.attributes);
                            vm.deprecation_in_progress.remove(&guard_key);
                            if let Some(custom_msg) = dep_result {
                                let class_display =
                                    goro_core::string::msg_str(&class.name).to_string();
                                let prefix = format!("Constant {}::{}", class_display, const_name);
                                let msg = vm.format_deprecated_message(&prefix, &custom_msg);
                                vm.emit_user_deprecated_at(&msg, vm.current_line);
                            }
                        }
                    }
                }
                // Constant expressions are evaluated, typed constants verified.
                match vm.class_constant_value(&class_lower, const_name.as_bytes()) {
                    Some(Ok(v)) => return Ok(v),
                    Some(Err(())) => {
                        return Err(VmError {
                            message: String::new(),
                            line: vm.current_line,
                        });
                    }
                    None => {}
                }
                if let Value::String(ms) = val {
                    if let Some(case) = ms.as_bytes().strip_prefix(b"__enum_case__::") {
                        if let Some(v) = vm.get_enum_case(&class_lower, case) {
                            return Ok(v);
                        }
                    }
                }
                return Ok(val.clone());
            }
        } else if let Some(v) = vm.get_builtin_class_constant(&class_lower, const_name.as_bytes()) {
            // A natively implemented internal class.
            if let Some(m) = vm.ext_class_constant_deprecation(&class_lower, const_name.as_bytes())
            {
                vm.emit_deprecated_at(&m, vm.current_line);
                if vm.current_exception.is_some() {
                    return Ok(Value::Null);
                }
            }
            return Ok(v);
        }
        let msg = format!("Undefined constant {}", name_str);
        let line = vm.current_line;
        let exc = vm.create_exception(b"Error", &msg, line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line });
    }
    // Strip leading backslash for fully-qualified constant names
    let lookup_name = if name.as_bytes().starts_with(b"\\") {
        &name.as_bytes()[1..]
    } else {
        name.as_bytes()
    };
    if let Some(val) = vm
        .constants
        .get(lookup_name)
        .filter(|_| !is_ini_entry_name(lookup_name))
        .cloned()
    {
        if let Some(m) = vm.internal_deprecated_constants.get(lookup_name).cloned() {
            vm.emit_deprecated_at(&m, vm.current_line);
        }
        if let Some(m) = goro_core::vm::builtin_constant_deprecation(lookup_name) {
            vm.emit_deprecated_at(&m, vm.current_line);
        }
        return Ok(val);
    }
    let msg = format!("Undefined constant \"{}\"", name_str);
    let line = vm.current_line;
    let exc = vm.create_exception(b"Error", &msg, line);
    vm.current_exception = Some(exc);
    Err(VmError { message: msg, line })
}

// === Output buffering === (see ob.rs)

fn flush_fn(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // sapi_flush(): the CLI hands pending output to stdout.
    vm.flush_cli_output();
    if let Some(f) = vm.sapi_flush {
        f(vm);
    }
    Ok(Value::Null)
}

// === Function handling ===

fn func_num_args(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // Return the number of arguments passed to the calling user function
    if let Some((_name, _file, _line, args, _is_method)) = vm.current_function_frame() {
        Ok(Value::Long(args.len() as i64))
    } else {
        // PHP 8.4: calling from global scope throws Error
        let msg = "func_num_args() must be called from a function context";
        let exc = vm.create_exception(b"Error", msg, vm.current_line);
        vm.current_exception = Some(exc);
        Err(VmError {
            message: msg.to_string(),
            line: vm.current_line,
        })
    }
}
fn func_get_args(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    vm.sync_frame_args();
    // Return an array of all arguments passed to the calling user function
    if let Some((_name, _file, _line, args, _is_method)) = vm.current_function_frame() {
        let mut arr = PhpArray::new();
        for arg in args.iter() {
            arr.push(arg.clone());
        }
        vm.release_synced_frame_args();
        Ok(Value::Array(Rc::new(RefCell::new(arr))))
    } else {
        // PHP 8.4: calling from global scope throws Error
        let msg = "func_get_args() cannot be called from the global scope";
        let exc = vm.create_exception(b"Error", msg, vm.current_line);
        vm.current_exception = Some(exc);
        Err(VmError {
            message: msg.to_string(),
            line: vm.current_line,
        })
    }
}
fn func_get_arg(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let index = args.first().unwrap_or(&Value::Null).to_long();
    // PHP 8.0+: validate argument before checking call stack
    if index < 0 {
        let msg = "func_get_arg(): Argument #1 ($position) must be greater than or equal to 0"
            .to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    vm.sync_frame_args();
    if let Some((_name, _file, _line, caller_args, _is_method)) = vm.current_function_frame() {
        if index as usize >= caller_args.len() {
            let msg = "func_get_arg(): Argument #1 ($position) must be less than the number of the arguments passed to the currently executed function".to_string();
            let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            Err(VmError {
                message: msg,
                line: vm.current_line,
            })
        } else {
            let v = caller_args[index as usize].clone();
            vm.release_synced_frame_args();
            Ok(v)
        }
    } else {
        // PHP 8.4: calling from global scope throws Error
        let msg = "func_get_arg() cannot be called from the global scope";
        let exc = vm.create_exception(b"Error", msg, vm.current_line);
        vm.current_exception = Some(exc);
        Err(VmError {
            message: msg.to_string(),
            line: vm.current_line,
        })
    }
}
fn function_exists(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let name = args.first().unwrap_or(&Value::Null).to_php_string();
    let raw = name.as_bytes();
    // Strip leading backslash for namespace resolution
    let stripped = if raw.first() == Some(&b'\\') {
        &raw[1..]
    } else {
        raw
    };
    let name_lower: Vec<u8> = stripped.iter().map(|b| b.to_ascii_lowercase()).collect();
    // "Class::method" is not a function
    if name_lower.contains(&b':') {
        return Ok(Value::False);
    }
    if name_lower == goro_core::gc::DESTRUCTOR_FIBER_FN {
        return Ok(Value::False);
    }
    if vm.functions.contains_key(&name_lower) || vm.user_functions.contains_key(&name_lower) {
        Ok(Value::True)
    } else {
        Ok(Value::False)
    }
}
fn is_closure_bytes(b: &[u8]) -> bool {
    b.starts_with(b"__closure_")
        || b.starts_with(b"__arrow_")
        || b.starts_with(b"__bound_closure_")
        || b.starts_with(b"__closure_fcc_")
}

/// Resolve a class name for a callable (zend_is_callable_check_class):
/// handles self/parent/static (with their deprecation unless `quiet`) and
/// autoloading. Returns the lowercase name.
fn callable_check_class(vm: &mut Vm, name: &[u8], quiet: bool) -> Result<Vec<u8>, String> {
    let lower = lower_bytes(name);
    let scope = vm.current_class_scope().map(|s| lower_bytes(&s));
    let deprecate = |vm: &mut Vm, kw: &str| {
        if !quiet {
            let line = vm.current_line;
            vm.emit_deprecated_at(
                &format!("Use of \"{}\" in callables is deprecated", kw),
                line,
            );
        }
    };
    match lower.as_slice() {
        b"self" => {
            let sc = scope.ok_or_else(|| {
                "cannot access \"self\" when no class scope is active".to_string()
            })?;
            deprecate(vm, "self");
            return Ok(sc);
        }
        b"parent" => {
            let sc = scope.ok_or_else(|| {
                "cannot access \"parent\" when no class scope is active".to_string()
            })?;
            let p = vm
                .classes
                .get(&sc)
                .and_then(|c| c.parent.as_ref().map(|p| lower_bytes(p)))
                .ok_or_else(|| {
                    "cannot access \"parent\" when current class scope has no parent".to_string()
                })?;
            deprecate(vm, "parent");
            return Ok(p);
        }
        b"static" => {
            let c = vm
                .called_class_stack
                .last()
                .map(|c| lower_bytes(c))
                .or(scope)
                .ok_or_else(|| {
                    "cannot access \"static\" when no class scope is active".to_string()
                })?;
            deprecate(vm, "static");
            return Ok(c);
        }
        _ => {}
    }
    let stripped = lower
        .strip_prefix(b"\\")
        .map(|s| s.to_vec())
        .unwrap_or(lower);
    if vm.classes.contains_key(&stripped) || vm.is_known_builtin_class(&stripped) {
        return Ok(stripped);
    }
    vm.try_autoload_class(name.strip_prefix(b"\\").unwrap_or(name));
    if vm.classes.contains_key(&stripped) {
        return Ok(stripped);
    }
    Err(format!(
        "class \"{}\" not found",
        goro_core::string::msg_str(name)
    ))
}

/// zend_is_callable_ex(): returns (callable, callable_name, error).
pub(crate) fn check_callable(
    vm: &mut Vm,
    val: &Value,
    syntax_only: bool,
) -> (bool, Vec<u8>, Option<String>) {
    let flags = if syntax_only {
        goro_core::callable::IS_CALLABLE_CHECK_SYNTAX_ONLY
    } else {
        0
    };
    let r = vm.zend_is_callable_ex(val, flags);
    (r.ok, r.name, r.error)
}

/// Reject callbacks that cannot possibly be resolved (zend_fcall_info_init errors).
fn validate_callback_arg(vm: &mut Vm, func: &str, cb: &Value) -> Result<(), VmError> {
    validate_callback_arg_check(vm, func, cb).map(|_| ())
}

/// zend_fcall_info_init() for call_user_func() and friends: the check, or
/// the TypeError.
fn validate_callback_arg_check(
    vm: &mut Vm,
    func: &str,
    cb: &Value,
) -> Result<goro_core::callable::CallableCheck, VmError> {
    let r = vm.zend_is_callable_ex(cb, 0);
    if vm.current_exception.is_some() {
        return Err(VmError {
            message: "Uncaught exception".into(),
            line: vm.current_line,
        });
    }
    if !r.ok {
        let msg = format!(
            "{}(): Argument #1 ($callback) must be a valid callback, {}",
            func,
            r.error.unwrap_or_default()
        );
        return Err(throw_type_error_msg(vm, msg));
    }
    Ok(r)
}

/// A callable naming an existing method the caller may not call, which
/// zend_is_callable_ex() resolved to the class's __call() / __callStatic()
/// trampoline: call that with the method name and the arguments.
fn call_inaccessible_via_trampoline(
    vm: &mut Vm,
    r: &goro_core::callable::CallableCheck,
    args: PhpArray,
) -> Option<Result<Value, VmError>> {
    let m = r.trampoline.as_ref()?;
    let class = vm.classes.get(r.calling_scope.as_ref()?)?;
    // (a missing method of the class of the calling $this goes to its
    // __call(): zend_std_get_static_method())
    if class.get_method(m).is_none() && r.object.is_none() {
        return None;
    }
    let (target, magic): (Value, &[u8]) = match &r.object {
        Some(o) => (o.clone(), b"__call"),
        None => (
            Value::String(PhpString::from_vec(class.name.clone())),
            b"__callStatic",
        ),
    };
    let mut cb = PhpArray::new();
    cb.push(target);
    cb.push(Value::String(PhpString::from_bytes(magic)));
    let cb = Value::Array(Rc::new(RefCell::new(cb)));
    let name = Value::String(PhpString::from_vec(m.clone()));
    Some(call_user_func(
        vm,
        &[cb, name, Value::Array(Rc::new(RefCell::new(args)))],
    ))
}

/// forward_static_call(): like call_user_func(), but a static method of a
/// class the called class (static::) extends keeps that called class.
fn forward_static_call_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let Some(cb) = args.first() else {
        return Ok(Value::Null);
    };
    validate_callback_arg(vm, "forward_static_call", cb)?;
    if vm.current_class_scope().is_none() {
        let msg = "Cannot call forward_static_call() when no class scope is active";
        let exc = vm.create_exception(b"Error", msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: format!("Uncaught Error: {}", msg),
            line: vm.current_line,
        });
    }
    let target = match cb.deref() {
        Value::Array(a) => {
            let a = a.borrow();
            match (
                a.get(&ArrayKey::Int(0)).map(|v| v.deref()),
                a.get(&ArrayKey::Int(1)).map(|v| v.deref()),
            ) {
                (Some(Value::String(c)), Some(Value::String(m))) => {
                    Some((c.as_bytes().to_vec(), m.as_bytes().to_vec()))
                }
                _ => None,
            }
        }
        Value::String(s) => {
            let b = s.as_bytes();
            b.windows(2)
                .position(|w| w == b"::")
                .map(|p| (b[..p].to_vec(), b[p + 2..].to_vec()))
        }
        _ => None,
    };
    let called = vm.called_class_stack.last().map(|c| lower_bytes(c));
    if let (Some((cls, m)), Some(called)) = (target, called) {
        if !m.windows(2).any(|w| w == b"::") {
            if let Ok(cl) = callable_check_class(vm, &cls, true) {
                let forwards = called == cl || vm.class_extends(&called, &cl);
                let method = vm
                    .classes
                    .get(&cl)
                    .and_then(|c| c.get_method(&lower_bytes(&m)).cloned());
                if let (true, Some(method)) = (forwards, method) {
                    if method.is_static {
                        let called_name = vm.called_class_stack.last().cloned().unwrap_or_default();
                        let named = std::mem::take(&mut vm.pending_named_args);
                        vm.called_class_stack.push(called_name);
                        vm.push_class_scope(method.declaring_class.clone());
                        let r = vm.execute_fn_with_named_args(
                            &method.op_array,
                            args[1..].to_vec(),
                            named,
                            None,
                        );
                        vm.pop_class_scope();
                        vm.called_class_stack.pop();
                        return r;
                    }
                }
            }
        }
    }
    call_user_func(vm, args)
}

fn forward_static_call_array_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let Some(cb) = args.first() else {
        return Ok(Value::Null);
    };
    let mut call_args = vec![cb.clone()];
    if let Some(Value::Array(a)) = args.get(1).map(|v| v.deref()) {
        let mut named = Vec::new();
        for (k, v) in a.borrow().iter() {
            match k {
                ArrayKey::String(s) => named.push((s.as_bytes().to_vec(), v.clone())),
                ArrayKey::Int(_) if !named.is_empty() => {
                    let msg = "Cannot use positional argument after named argument";
                    let exc = vm.create_exception(b"Error", msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    return Err(VmError {
                        message: format!("Uncaught Error: {}", msg),
                        line: vm.current_line,
                    });
                }
                ArrayKey::Int(_) => call_args.push(v.clone()),
            }
        }
        if !named.is_empty() {
            vm.pending_named_args = named;
        }
    }
    forward_static_call_fn(vm, &call_args)
}

/// A callback zend_is_callable_ex() accepts without any side effect or
/// trampoline: a plain function name, a Closure, or `[$object, 'method']`
/// naming a public method of the object's user class. (The checked
/// call_user_func() variants then skip the full check.)
fn plain_callable(vm: &Vm, cb: &Value) -> bool {
    match cb {
        Value::String(s) => {
            let b = s.as_bytes();
            if b.is_empty() || b.len() > 64 || b[0] == b'\\' || b.contains(&b':') {
                return false;
            }
            let mut buf = [0u8; 64];
            for (d, c) in buf.iter_mut().zip(b) {
                *d = c.to_ascii_lowercase();
            }
            let lower = &buf[..b.len()];
            !matches!(
                lower,
                b"extract"
                    | b"compact"
                    | b"get_defined_vars"
                    | b"func_get_args"
                    | b"func_get_arg"
                    | b"func_num_args"
            ) && (vm.user_functions.contains_key(lower) || vm.functions.contains_key(lower))
        }
        Value::Object(o) => o.borrow().closure.is_some(),
        Value::Array(a) => {
            let a = a.borrow();
            if a.len() != 2 {
                return false;
            }
            let (Some(target), Some(Value::String(m))) = (a.get_int(0), a.get_int(1)) else {
                return false;
            };
            let m = m.as_bytes();
            if m.is_empty() || m.contains(&b':') || m.starts_with(b"__") {
                return false;
            }
            // `['Class', 'staticMethod']` of a declared user class
            if let Value::String(c) = target {
                let c = c.as_bytes().to_ascii_lowercase();
                if matches!(c.as_slice(), b"self" | b"parent" | b"static")
                    || c.first() == Some(&b'\\')
                {
                    return false;
                }
                return vm.classes.get(&c).is_some_and(|ce| {
                    ce.filename.is_some()
                        && !ce.is_trait
                        && ce.get_method(&m.to_ascii_lowercase()).is_some_and(|md| {
                            md.visibility == goro_core::object::Visibility::Public
                                && md.is_static
                                && !md.is_abstract
                        })
                });
            }
            let Value::Object(o) = target else {
                return false;
            };
            let ob = o.borrow();
            if ob.native.is_some() || ob.closure.is_some() {
                return false;
            }
            vm.classes
                .get(&ob.class_name.to_ascii_lowercase())
                .is_some_and(|c| {
                    c.filename.is_some()
                        && c.get_method(&m.to_ascii_lowercase()).is_some_and(|md| {
                            md.visibility == goro_core::object::Visibility::Public
                                && !md.is_abstract
                        })
                })
        }
        _ => false,
    }
}

fn call_user_func_checked(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.first().is_some_and(|cb| plain_callable(vm, cb)) {
        return call_user_func(vm, args);
    }
    if let Some(cb) = args.first() {
        let r = validate_callback_arg_check(vm, "call_user_func", cb)?;
        if r.trampoline.is_some() && vm.pending_named_args.is_empty() {
            let mut a = PhpArray::new();
            for v in &args[1..] {
                a.push(v.deref());
            }
            if let Some(res) = call_inaccessible_via_trampoline(vm, &r, a) {
                return res;
            }
        }
        if let Some(cb) = callable_with_this(cb, &r) {
            let mut a = args.to_vec();
            a[0] = cb;
            return call_user_func(vm, &a);
        }
    }
    call_user_func(vm, args)
}

/// `'A::m'` / `['A', 'm']` naming a non-static method, called from a
/// method whose `$this` is an A: zend_is_callable_check_class() calls it
/// on that `$this`.
fn callable_with_this(cb: &Value, r: &goro_core::callable::CallableCheck) -> Option<Value> {
    let obj = r.object.as_ref()?;
    if r.trampoline.is_some() {
        return None;
    }
    let method = match cb.deref() {
        Value::String(s) => {
            let b = s.as_bytes();
            let p = b.windows(2).position(|w| w == b"::")?;
            let class = b[..p].to_ascii_lowercase();
            if matches!(class.as_slice(), b"parent" | b"self" | b"static") {
                return None;
            }
            b.to_vec()
        }
        Value::Array(a) => {
            let a = a.borrow();
            match (
                a.get(&ArrayKey::Int(0)).map(|v| v.deref()),
                a.get(&ArrayKey::Int(1)).map(|v| v.deref()),
            ) {
                (Some(Value::String(c)), Some(Value::String(m)))
                    if !m.as_bytes().windows(2).any(|w| w == b"::") =>
                {
                    [c.as_bytes(), b"::", m.as_bytes()].concat()
                }
                _ => return None,
            }
        }
        _ => return None,
    };
    // (the method of that class, as `[$this, 'A::m']`)
    let mut a = PhpArray::new();
    a.push(obj.clone());
    a.push(Value::String(PhpString::from_vec(method)));
    Some(Value::Array(Rc::new(RefCell::new(a))))
}

fn call_user_func_array_checked(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.first().is_some_and(|cb| plain_callable(vm, cb))
        && matches!(args.get(1), Some(Value::Array(_)))
    {
        return call_user_func_array(vm, args);
    }
    if let Some(cb) = args.first() {
        let r = validate_callback_arg_check(vm, "call_user_func_array", cb)?;
        // Z_PARAM_ARRAY($args)
        if let Some(a) = args.get(1).map(|v| v.deref()) {
            if !matches!(a, Value::Array(_)) {
                return Err(throw_type_error_msg(
                    vm,
                    format!(
                        "call_user_func_array(): Argument #2 ($args) must be of type array, {} given",
                        Vm::value_type_name(&a)
                    ),
                ));
            }
        }
        if r.trampoline.is_some() {
            if let Some(Value::Array(a)) = args.get(1).map(|v| v.deref()) {
                let a = a.borrow().clone();
                if let Some(res) = call_inaccessible_via_trampoline(vm, &r, a) {
                    return res;
                }
            }
        }
        if let Some(cb) = callable_with_this(cb, &r) {
            let mut a = args.to_vec();
            a[0] = cb;
            return call_user_func_array(vm, &a);
        }
    }
    call_user_func_array(vm, args)
}

fn is_callable(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().cloned().unwrap_or(Value::Null);
    let syntax_only = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
    let (ok, name, _) = check_callable(vm, &val, syntax_only);
    if vm.current_exception.is_some() {
        return Err(VmError {
            message: "Uncaught exception".into(),
            line: vm.current_line,
        });
    }
    set_callable_name_and_return(
        vm,
        args,
        Some(name),
        if ok { Value::True } else { Value::False },
    )
}

fn set_callable_name_and_return(
    _vm: &mut Vm,
    args: &[Value],
    callable_name: Option<Vec<u8>>,
    result: Value,
) -> Result<Value, VmError> {
    // Set the callable name in the third argument if provided (pass by reference)
    if let Some(name_bytes) = callable_name {
        if let Some(name_ref) = args.get(2) {
            if let Value::Reference(r) = name_ref {
                *r.borrow_mut() = Value::String(PhpString::from_vec(name_bytes));
            }
        }
    }
    Ok(result)
}
/// call_user_func() of `[$objOrClass, "Scope::method"]`: None when the
/// method string has no scope part (or it does not resolve).
fn call_scoped_method_callable(
    vm: &mut Vm,
    first: &Value,
    method_lower: &[u8],
    call_args: &[Value],
    named_args: &[(Vec<u8>, Value)],
) -> Option<Result<Value, VmError>> {
    let pos = method_lower.windows(2).position(|w| w == b"::")?;
    let (org_lower, org_name, this) = match first {
        Value::Object(o) => {
            let cn = o.borrow().class_name.clone();
            (lower_bytes(&cn), cn, Some(first.clone()))
        }
        Value::String(s) if !is_closure_bytes(s.as_bytes()) => {
            let l = callable_check_class(vm, s.as_bytes(), true).ok()?;
            let name = vm.classes.get(&l)?.name.clone();
            (l, name, None)
        }
        _ => return None,
    };
    let prefix = &method_lower[..pos];
    let target = match prefix {
        b"parent" => lower_bytes(vm.classes.get(&org_lower)?.parent.as_ref()?),
        b"self" | b"static" => org_lower.clone(),
        p => lower_bytes(p.strip_prefix(b"\\").unwrap_or(p)),
    };
    if !vm.classes.contains_key(&target) {
        // A method of a natively implemented internal class
        let m = &method_lower[pos + 2..];
        let fail = |vm: &mut Vm| VmError {
            message: "Uncaught exception".into(),
            line: vm.current_line,
        };
        if m == b"__construct" {
            let h = vm.object_construct_handlers.get(&target).copied()?;
            let obj = this?;
            let mut a = vec![obj];
            a.extend_from_slice(call_args);
            let line = vm.current_line;
            let _ = h(vm, &a, line);
            return Some(if vm.current_exception.is_some() {
                Err(fail(vm))
            } else {
                Ok(Value::Null)
            });
        }
        let (native, h) = vm.find_native_class_handler(&target)?;
        if !(h.has_method)(&native, m, this.is_none()) {
            return None;
        }
        let had = vm.current_exception.is_some();
        let r = h.invoke(vm, &native, m, &org_name, this.as_ref(), call_args);
        return Some(if !had && vm.current_exception.is_some() {
            Err(fail(vm))
        } else {
            Ok(r)
        });
    }
    let class = vm.classes.get(&target)?.clone();
    let method = class.get_method(&method_lower[pos + 2..])?;
    let mut op = method.op_array.clone();
    if op.attributes.is_empty() && !method.attributes.is_empty() {
        std::rc::Rc::make_mut(&mut op).attributes = method.attributes.clone();
    }
    // zend_is_callable_check_class(): parent/self take the frame's called
    // scope when it is a subclass, else the class itself; a class name is
    // called on itself (or on the object).
    let frame_called = vm.called_class_stack.last().cloned();
    let called = match prefix {
        _ if this.is_some() => org_name,
        b"parent" | b"self" => match frame_called {
            Some(fc) if vm.class_extends(&lower_bytes(&fc), &target) => fc,
            _ => class.name.clone(),
        },
        b"static" => frame_called.unwrap_or(org_name),
        _ => class.name.clone(),
    };
    let this = if method.is_static { None } else { this };
    let declaring = method.declaring_class.clone();
    vm.called_class_stack.push(called);
    vm.push_class_scope(declaring);
    let r = vm.execute_fn_with_named_args(&op, call_args.to_vec(), named_args.to_vec(), this);
    vm.pop_class_scope();
    vm.called_class_stack.pop();
    Some(r)
}

pub(crate) fn call_user_func(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let closure = match args.first() {
        Some(Value::Object(o)) if o.borrow().closure.is_some() => Some(Rc::downgrade(o)),
        _ => None,
    };
    let r = call_user_func_inner(vm, args);
    // zend_call_function() of a closure releases it (ZEND_CALL_CLOSURE).
    if let Some(o) = closure.and_then(|w| w.upgrade()) {
        goro_core::gc::possible_root_object(&o);
    }
    r
}

fn call_user_func_inner(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        return Ok(Value::Null);
    }

    // [$closure, '__invoke']: Closure::__invoke() (internal) makes the
    // call, so the closure is not called from user code.
    if let Value::Array(arr) = args[0].deref() {
        let a = arr.borrow();
        let mut it = a.values();
        if let (Some(Value::Object(o)), Some(m)) = (it.next().map(|v| v.deref()), it.next()) {
            if o.borrow().class_name.eq_ignore_ascii_case(b"Closure")
                && m.deref()
                    .to_php_string()
                    .as_bytes()
                    .eq_ignore_ascii_case(b"__invoke")
            {
                vm.ucf_call_line = None;
                vm.closure_invoke_call = true;
            }
        }
    }
    let callback_v = args[0].closure_callable();
    let callback = &callback_v;
    if let Value::String(s) = callback {
        if let Some(msg) = Vm::forbidden_dynamic_call(s.as_bytes()) {
            let exc = vm.create_exception(b"Error", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: format!("Uncaught Error: {}", msg),
                line: vm.current_line,
            });
        }
    }
    // A static method of an internal class: zend_call_function() on it.
    if let Some(t) = vm.internal_static_target(callback) {
        let named = std::mem::take(&mut vm.pending_named_args);
        return vm.call_internal_static(t, &args[1..], named);
    }
    let call_args: Vec<Value> = args[1..].to_vec();
    // Take any forwarded named args from the VM
    let named_args: Vec<(Vec<u8>, Value)> = std::mem::take(&mut vm.pending_named_args);

    // Handle array callback: [class_or_object, method_name]
    if let Value::Array(arr) = callback {
        let arr_borrow = arr.borrow();
        // [&$obj, 'method'] — element references are transparent. A
        // closure's internal [name, captured...] keeps its by-reference
        // captures as the shared reference cells.
        let mut vals: Vec<Value> = arr_borrow.values().cloned().collect();
        if let Some(v) = vals.first_mut() {
            *v = v.deref();
        }
        let is_closure = matches!(vals.first(), Some(Value::String(n))
            if goro_core::closure::is_closure_fn_name(n.as_bytes())
                || n.as_bytes().starts_with(b"__closure_fcc_"));
        if !is_closure {
            if let Some(v) = vals.get_mut(1) {
                *v = v.deref();
            }
        }
        drop(arr_borrow);
        if vals.len() >= 2 {
            let method_name = vals[1].to_php_string();
            let method_lower: Vec<u8> = method_name
                .as_bytes()
                .iter()
                .map(|b| b.to_ascii_lowercase())
                .collect();

            // [$objOrClass, "Scope::method"] (deprecated form): Scope is
            // resolved relative to the class of the first member.
            if let Some(r) =
                call_scoped_method_callable(vm, &vals[0], &method_lower, &call_args, &named_args)
            {
                return r;
            }

            // Check if first element is a class name (string) or object
            match &vals[0] {
                Value::String(class_name) => {
                    let class_bytes = class_name.as_bytes();
                    // Check if this is a closure with captures (not a class::method call)
                    if !class_bytes.starts_with(b"__closure_")
                        && !class_bytes.starts_with(b"__arrow_")
                    {
                        // Static method call: ['ClassName', 'method']
                        let class_lower: Vec<u8> =
                            class_bytes.iter().map(|b| b.to_ascii_lowercase()).collect();
                        // Check for "Class::method" in the class name itself
                        if let Some(pos) = class_lower.iter().position(|&b| b == b':') {
                            if pos + 1 < class_lower.len() && class_lower[pos + 1] == b':' {
                                let real_class = &class_lower[..pos];
                                let real_method = &class_lower[pos + 2..];
                                if let Some(class) = vm.classes.get(real_class).cloned() {
                                    if let Some(method) = class.methods.get(real_method) {
                                        let mut op = method.op_array.clone();
                                        if op.attributes.is_empty() && !method.attributes.is_empty()
                                        {
                                            std::rc::Rc::make_mut(&mut op).attributes =
                                                method.attributes.clone();
                                        }
                                        let display_fallback = format!(
                                            "{}::{}",
                                            String::from_utf8_lossy(&class.name),
                                            String::from_utf8_lossy(&method.name)
                                        );
                                        let line = vm.current_line;
                                        vm.check_user_function_deprecation(
                                            &op,
                                            &display_fallback,
                                            line,
                                        );
                                        let declaring = method.declaring_class.clone();
                                        let class_name_orig = class.name.clone();
                                        vm.called_class_stack.push(class_name_orig);
                                        vm.push_class_scope(declaring);
                                        let result = vm.execute_fn_with_named_args(
                                            &op, call_args, named_args, None,
                                        );
                                        vm.pop_class_scope();
                                        vm.called_class_stack.pop();
                                        return result;
                                    }
                                }
                                return Ok(Value::Null);
                            }
                        }

                        // Check for forwarding calls (parent::, self:: in array syntax)
                        let is_forwarding = class_lower == b"parent"
                            || class_lower == b"self"
                            || class_lower == b"static";
                        let resolved_class_lower = if class_lower == b"parent" {
                            vm.get_current_class_name()
                                .as_ref()
                                .and_then(|scope| vm.classes.get(scope))
                                .and_then(|c| c.parent.clone())
                                .map(|p| {
                                    p.iter()
                                        .map(|b| b.to_ascii_lowercase())
                                        .collect::<Vec<u8>>()
                                })
                                .unwrap_or(class_lower.clone())
                        } else if class_lower == b"static" {
                            vm.called_class_stack
                                .last()
                                .map(|n| lower_bytes(n))
                                .or_else(|| vm.get_current_class_name())
                                .unwrap_or(class_lower.clone())
                        } else if class_lower == b"self" {
                            vm.get_current_class_name()
                                .as_ref()
                                .cloned()
                                .or_else(|| {
                                    vm.called_class_stack
                                        .last()
                                        .map(|n| n.iter().map(|b| b.to_ascii_lowercase()).collect())
                                })
                                .unwrap_or(class_lower.clone())
                        } else {
                            class_lower.clone()
                        };

                        let found = vm.classes.get(&resolved_class_lower).map(|class| {
                            (
                                class.name.clone(),
                                class.methods.get(&method_lower).cloned(),
                                class.get_method(b"__callstatic").cloned(),
                            )
                        });
                        if let Some((class_name, class_method, call_static)) = found {
                            if let Some(method) = class_method.as_ref() {
                                // Check visibility
                                let accessible = match method.visibility {
                                    goro_core::object::Visibility::Public => true,
                                    goro_core::object::Visibility::Protected
                                    | goro_core::object::Visibility::Private => {
                                        if let Some(caller) = vm.current_class_scope() {
                                            let caller_lower: Vec<u8> = caller
                                                .iter()
                                                .map(|b| b.to_ascii_lowercase())
                                                .collect();
                                            let declaring_lower: Vec<u8> = method
                                                .declaring_class
                                                .iter()
                                                .map(|b| b.to_ascii_lowercase())
                                                .collect();
                                            if method.visibility
                                                == goro_core::object::Visibility::Private
                                            {
                                                caller_lower == declaring_lower
                                            } else {
                                                caller_lower == declaring_lower
                                                    || vm.class_extends(
                                                        &caller_lower,
                                                        &declaring_lower,
                                                    )
                                                    || vm.class_extends(
                                                        &declaring_lower,
                                                        &caller_lower,
                                                    )
                                            }
                                        } else {
                                            false
                                        }
                                    }
                                };
                                if accessible {
                                    let mut op = method.op_array.clone();
                                    if op.attributes.is_empty() && !method.attributes.is_empty() {
                                        std::rc::Rc::make_mut(&mut op).attributes =
                                            method.attributes.clone();
                                    }
                                    let display_fallback = format!(
                                        "{}::{}",
                                        String::from_utf8_lossy(&class_name),
                                        String::from_utf8_lossy(&method.name)
                                    );
                                    let line = vm.current_line;
                                    vm.check_user_function_deprecation(
                                        &op,
                                        &display_fallback,
                                        line,
                                    );
                                    let declaring = method.declaring_class.clone();
                                    if is_forwarding {
                                        // Forward: keep current called_class
                                    } else {
                                        vm.called_class_stack.push(class_name.clone());
                                    }
                                    vm.push_class_scope(declaring);
                                    let result = vm.execute_fn_with_named_args(
                                        &op, call_args, named_args, None,
                                    );
                                    vm.pop_class_scope();
                                    if !is_forwarding {
                                        vm.called_class_stack.pop();
                                    }
                                    return result;
                                }
                                // Method not accessible - fall through to __callStatic
                            }
                            // Method not found or not accessible - check for __callStatic magic method
                            if let Some(cs) = call_static {
                                let called = (!is_forwarding).then(|| class_name.clone());
                                return vm.call_static_trampoline(
                                    &cs,
                                    called,
                                    method_name.as_bytes(),
                                    &call_args,
                                    named_args,
                                );
                            }
                            return Ok(Value::Null);
                        }
                        // Extension-provided static methods of built-in classes.
                        if let Some((native, h)) =
                            vm.find_native_class_handler(&resolved_class_lower)
                        {
                            if (h.has_method)(&native, &method_lower, true) {
                                let called = class_bytes.to_vec();
                                return Ok(h.invoke(
                                    vm,
                                    &native,
                                    &method_lower,
                                    &called,
                                    None,
                                    &call_args,
                                ));
                            }
                        }
                        // Not a closure, not a class - fall through to closure path below
                    }
                    // Fall through to closure handling below
                }
                Value::Object(obj) => {
                    // Object method call: [$obj, 'method']
                    let class_lower: Vec<u8>;
                    {
                        let obj_borrow = obj.borrow();
                        class_lower = obj_borrow
                            .class_name
                            .iter()
                            .map(|b| b.to_ascii_lowercase())
                            .collect();
                    }
                    // (what the call needs of the class: not a copy of all of it)
                    let found = vm.classes.get(&class_lower).map(|class| {
                        (
                            class.name.clone(),
                            class.methods.get(&method_lower).cloned(),
                            class.get_method(b"__call").cloned(),
                        )
                    });
                    if let Some((class_name, class_method, call_magic)) = found {
                        // A private method of the calling scope shadows the one
                        // a subclass redeclared (ZEND_ACC_CHANGED).
                        let shadow = vm
                            .current_class_scope()
                            .map(|c| c.to_ascii_lowercase())
                            .filter(|sc| *sc != class_lower && vm.class_extends(&class_lower, sc))
                            .and_then(|sc| {
                                vm.classes
                                    .get(&sc)
                                    .and_then(|c| c.methods.get(&method_lower))
                                    .filter(|m| {
                                        m.visibility == goro_core::object::Visibility::Private
                                            && m.declaring_class.eq_ignore_ascii_case(&sc)
                                    })
                                    .cloned()
                            });
                        if let Some(method) = shadow.as_ref().or(class_method.as_ref()) {
                            // Check visibility before executing
                            let accessible = match method.visibility {
                                goro_core::object::Visibility::Public => true,
                                goro_core::object::Visibility::Protected
                                | goro_core::object::Visibility::Private => {
                                    // Check if caller scope has access
                                    if let Some(caller) = vm.current_class_scope() {
                                        let caller_lower: Vec<u8> =
                                            caller.iter().map(|b| b.to_ascii_lowercase()).collect();
                                        let declaring_lower: Vec<u8> = method
                                            .declaring_class
                                            .iter()
                                            .map(|b| b.to_ascii_lowercase())
                                            .collect();
                                        if method.visibility
                                            == goro_core::object::Visibility::Private
                                        {
                                            caller_lower == declaring_lower
                                        } else {
                                            // Protected: caller must be same or related class
                                            caller_lower == declaring_lower
                                                || vm.class_extends(&caller_lower, &declaring_lower)
                                                || vm.class_extends(&declaring_lower, &caller_lower)
                                        }
                                    } else {
                                        false // Global scope can't access protected/private
                                    }
                                }
                            };
                            if accessible {
                                let mut op = method.op_array.clone();
                                if op.attributes.is_empty() && !method.attributes.is_empty() {
                                    std::rc::Rc::make_mut(&mut op).attributes =
                                        method.attributes.clone();
                                }
                                let display_fallback = format!(
                                    "{}::{}",
                                    String::from_utf8_lossy(&class_name),
                                    String::from_utf8_lossy(&method.name)
                                );
                                let line = vm.current_line;
                                vm.check_user_function_deprecation(&op, &display_fallback, line);
                                // Static methods called through an object get no $this.
                                let this = if method.is_static {
                                    None
                                } else {
                                    Some(Value::Object(obj.clone()))
                                };
                                // The method runs in its declaring class's scope,
                                // with the object's class as the called class.
                                let scope = if method.declaring_class.is_empty() {
                                    class_name.to_ascii_lowercase()
                                } else {
                                    method.declaring_class.to_ascii_lowercase()
                                };
                                vm.called_class_stack.push(obj.borrow().class_name.clone());
                                vm.push_class_scope(scope);
                                let r =
                                    vm.execute_fn_with_named_args(&op, call_args, named_args, this);
                                vm.pop_class_scope();
                                vm.called_class_stack.pop();
                                return r;
                            }
                            // Method exists but not accessible - fall through to __call
                        }
                        // Method not found or not accessible - check for __call magic method
                        if let Some(call_method) = call_magic.as_ref() {
                            let op = call_method.op_array.clone();
                            // __call($name, $arguments) - pack call_args into an array for the second param
                            let mut args_array = goro_core::array::PhpArray::new();
                            for arg in &call_args {
                                args_array.push(arg.clone());
                            }
                            let magic_args = vec![
                                Value::String(PhpString::from_vec(method_name.as_bytes().to_vec())),
                                Value::Array(std::rc::Rc::new(std::cell::RefCell::new(args_array))),
                            ];
                            let scope = if call_method.declaring_class.is_empty() {
                                class_name.to_ascii_lowercase()
                            } else {
                                call_method.declaring_class.to_ascii_lowercase()
                            };
                            vm.called_class_stack.push(obj.borrow().class_name.clone());
                            vm.push_class_scope(scope);
                            let r = vm.execute_fn_with_named_args(
                                &op,
                                magic_args,
                                vec![],
                                Some(Value::Object(obj.clone())),
                            );
                            vm.pop_class_scope();
                            vm.called_class_stack.pop();
                            return r;
                        }
                    }
                    // Built-in class methods (SPL, ext-provided native classes).
                    if let Some(r) = vm.call_object_method(
                        &Value::Object(obj.clone()),
                        method_name.as_bytes(),
                        &call_args,
                    ) {
                        return Ok(r);
                    }
                    return Ok(Value::Null);
                }
                _ => {}
            }
        }
        // Fall through to old behavior for other array callbacks (closures)
        if !vals.is_empty() {
            let name = vals[0].to_php_string().as_bytes().to_vec();
            let captured: Vec<Value> = vals[1..].to_vec();
            let func_lower: Vec<u8> = name.iter().map(|b| b.to_ascii_lowercase()).collect();

            if let Some(builtin) = vm.functions.get(&func_lower).copied() {
                return vm.call_builtin_from_internal(builtin, &func_lower, &call_args);
            }
            if let Some(user_fn) = vm.user_functions.get(&func_lower).cloned() {
                let mut combined_args = captured;
                combined_args.extend(call_args);
                return vm.execute_fn_with_named_args(&user_fn, combined_args, named_args, None);
            }
        }
        return Ok(Value::Null);
    }

    // Handle object callback (invoking __invoke)
    if let Value::Object(obj) = callback {
        let class_lower: Vec<u8>;
        {
            let obj_borrow = obj.borrow();
            class_lower = obj_borrow
                .class_name
                .iter()
                .map(|b| b.to_ascii_lowercase())
                .collect();
        }
        if let Some(class) = vm.classes.get(&class_lower).cloned() {
            if let Some(method) = class.methods.get(b"__invoke".as_slice()) {
                let mut op = method.op_array.clone();
                if op.attributes.is_empty() && !method.attributes.is_empty() {
                    std::rc::Rc::make_mut(&mut op).attributes = method.attributes.clone();
                }
                let display_fallback =
                    format!("{}::__invoke", String::from_utf8_lossy(&class.name));
                let line = vm.current_line;
                vm.check_user_function_deprecation(&op, &display_fallback, line);
                // __invoke runs in its declaring class's scope.
                let scope = if method.declaring_class.is_empty() {
                    class.name.clone()
                } else {
                    method.declaring_class.clone()
                };
                vm.called_class_stack.push(obj.borrow().class_name.clone());
                vm.push_class_scope(scope);
                let r = vm.execute_fn_with_named_args(
                    &op,
                    call_args,
                    named_args,
                    Some(Value::Object(obj.clone())),
                );
                vm.pop_class_scope();
                vm.called_class_stack.pop();
                return r;
            }
        }
        return Ok(Value::Null);
    }

    // Handle closure callback (stored as string name or array with captures)
    if let Value::String(s) = callback {
        let func_name = s.as_bytes().to_vec();
        let func_lower: Vec<u8> = func_name.iter().map(|b| b.to_ascii_lowercase()).collect();
        // zend_is_callable_check_func(): a function named with a leading
        // `\` (not "\\Class::method", resolved below).
        let fn_lower: &[u8] = if func_lower.windows(2).any(|w| w == b"::") {
            &func_lower
        } else {
            func_lower.strip_prefix(b"\\").unwrap_or(&func_lower)
        };

        // Try builtin first
        if let Some(builtin) = vm.functions.get(fn_lower).copied() {
            return vm.call_builtin_from_internal_named(builtin, fn_lower, &call_args, named_args);
        }

        // Try user function (but not "Class::method" strings - those need proper LSB handling below)
        let looks_like_class_method = func_lower.windows(2).any(|w| w == b"::");
        if !looks_like_class_method {
            if let Some(user_fn) = vm.user_functions.get(fn_lower).cloned() {
                if !user_fn.attributes.is_empty() {
                    let fallback = String::from_utf8_lossy(&func_name).into_owned();
                    let line = vm.current_line;
                    vm.check_user_function_deprecation(&user_fn, &fallback, line);
                }
                return vm.execute_fn_with_named_args(&user_fn, call_args, named_args, None);
            }
        }

        // Try "Class::method" syntax
        if let Some(pos) = func_lower.windows(2).rposition(|w| w == b"::") {
            if pos + 1 < func_lower.len() && func_lower[pos + 1] == b':' {
                // A fully qualified "\Foo::bar" names class Foo.
                let class_part_lower = func_lower[..pos]
                    .strip_prefix(b"\\")
                    .unwrap_or(&func_lower[..pos])
                    .to_vec();
                let method_lower = &func_lower[pos + 2..];

                // Check for forwarding calls (parent::, self::)
                let is_forwarding = class_part_lower == b"parent"
                    || class_part_lower == b"self"
                    || class_part_lower == b"static";

                // Resolve parent/self to actual class names
                let resolved_class_lower = if class_part_lower == b"parent" {
                    vm.get_current_class_name()
                        .as_ref()
                        .and_then(|scope| vm.classes.get(scope))
                        .and_then(|c| c.parent.clone())
                        .map(|p| {
                            p.iter()
                                .map(|b| b.to_ascii_lowercase())
                                .collect::<Vec<u8>>()
                        })
                        .unwrap_or(class_part_lower.clone())
                } else if class_part_lower == b"static" {
                    vm.called_class_stack
                        .last()
                        .map(|n| lower_bytes(n))
                        .or_else(|| vm.get_current_class_name())
                        .unwrap_or(class_part_lower.clone())
                } else if class_part_lower == b"self" {
                    vm.get_current_class_name()
                        .as_ref()
                        .cloned()
                        .or_else(|| {
                            vm.called_class_stack
                                .last()
                                .map(|n| n.iter().map(|b| b.to_ascii_lowercase()).collect())
                        })
                        .unwrap_or(class_part_lower.clone())
                } else {
                    class_part_lower.clone()
                };

                // If class is missing and not forwarding/builtin, give a
                // registered autoloader a chance to define it (PHP triggers
                // autoload for the class half of a "Class::method" callable).
                if !is_forwarding
                    && !vm.classes.contains_key(&resolved_class_lower)
                    && !vm.is_known_builtin_class(&resolved_class_lower)
                {
                    let class_orig = &func_name[..pos];
                    vm.try_autoload_class(class_orig);
                }

                if let Some(class) = vm.classes.get(&resolved_class_lower).cloned() {
                    if let Some(method) = class.methods.get(method_lower) {
                        let mut op = method.op_array.clone();
                        if op.attributes.is_empty() && !method.attributes.is_empty() {
                            std::rc::Rc::make_mut(&mut op).attributes = method.attributes.clone();
                        }
                        let display_fallback = format!(
                            "{}::{}",
                            String::from_utf8_lossy(&class.name),
                            String::from_utf8_lossy(&method.name)
                        );
                        let line = vm.current_line;
                        vm.check_user_function_deprecation(&op, &display_fallback, line);
                        let declaring = method.declaring_class.clone();
                        // Push called_class and class_scope for proper LSB
                        if is_forwarding {
                            // Forward: keep current called_class
                            // (don't push anything extra - let the current stack value persist)
                        } else {
                            // Non-forwarding: push explicit class name
                            let class_name_orig = class.name.clone();
                            vm.called_class_stack.push(class_name_orig);
                        }
                        vm.push_class_scope(declaring);
                        let result =
                            vm.execute_fn_with_named_args(&op, call_args, named_args, None);
                        vm.pop_class_scope();
                        if !is_forwarding {
                            vm.called_class_stack.pop();
                        }
                        return result;
                    }
                    // Check __callStatic
                    if let Some(call_static) = class.get_method(b"__callstatic") {
                        let orig_method = &func_name[pos + 2..];
                        let called = (!is_forwarding).then(|| class.name.clone());
                        return vm.call_static_trampoline(
                            call_static,
                            called,
                            orig_method,
                            &call_args,
                            named_args,
                        );
                    }
                } else if !is_forwarding && !vm.is_known_builtin_class(&resolved_class_lower) {
                    // Class still not found after autoload: PHP throws a TypeError.
                    let class_orig = &func_name[..pos];
                    let class_display = goro_core::value::display_class_name(class_orig);
                    let msg = format!(
                        "call_user_func(): Argument #1 ($callback) must be a valid callback, class \"{}\" not found",
                        class_display
                    );
                    let exc = vm.create_exception(b"TypeError", &msg, 0);
                    vm.current_exception = Some(exc);
                    return Err(VmError {
                        message: msg,
                        line: vm.current_line,
                    });
                }
            }
        }
    }

    Ok(Value::Null)
}
fn call_user_func_array(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.len() < 2 {
        return Ok(Value::Null);
    }
    let callback = args[0].clone();
    let (positional, named) = match &args[1] {
        Value::Array(arr) => {
            let arr = arr.borrow();
            let mut pos = Vec::new();
            let mut named = Vec::new();
            let mut had_named = false;
            for (k, v) in arr.iter() {
                match k {
                    goro_core::array::ArrayKey::String(s) => {
                        had_named = true;
                        named.push((s.as_bytes().to_vec(), v.clone()));
                    }
                    goro_core::array::ArrayKey::Int(_) => {
                        if had_named {
                            // Cannot use positional argument after named argument
                            let exc = vm.create_exception(
                                b"Error",
                                "Cannot use positional argument after named argument",
                                0,
                            );
                            vm.current_exception = Some(exc);
                            return Err(VmError {
                                message: "Cannot use positional argument after named argument"
                                    .into(),
                                line: 0,
                            });
                        }
                        pos.push(v.clone());
                    }
                }
            }
            (pos, named)
        }
        _ => (vec![], vec![]),
    };
    // Store named args for forwarding
    if !named.is_empty() {
        vm.pending_named_args = named;
    }
    let mut call_args = vec![callback];
    call_args.extend(positional);
    call_user_func(vm, &call_args)
}

// === Array functions ===

fn array_push(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let mut arr = arr.borrow_mut();
        for val in &args[1..] {
            if let Err(msg) = arr.try_push(val.deref_for_storage()) {
                drop(arr);
                let exc = vm.create_exception(b"Error", msg, 0);
                vm.current_exception = Some(exc);
                return Err(VmError {
                    message: format!("Uncaught Error: {}", msg),
                    line: 0,
                });
            }
        }
        Ok(Value::Long(arr.len() as i64))
    } else {
        require_array_arg(
            vm,
            &args.first().map(|v| v.deref()).unwrap_or(Value::Null),
            "array_push",
            "array",
            1,
        )?;
        Ok(Value::Long(0))
    }
}

fn array_pop(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let arr_ref = args.first().unwrap_or(&Value::Null);
    // Unwrap reference if needed
    let arr_val = if let Value::Reference(r) = arr_ref {
        r.borrow().clone()
    } else {
        arr_ref.clone()
    };
    if let Value::Array(arr) = &arr_val {
        let mut arr = arr.borrow_mut();
        // Remove last element
        let len = arr.len();
        if len == 0 {
            return Ok(Value::Null);
        }
        let (_, last_val) = arr.pop_entry().unwrap();
        // Reset internal pointer
        arr.pointer = 0;
        drop(arr);
        // RETVAL_COPY_DEREF()
        Ok(last_val.deref())
    } else {
        Ok(Value::Null)
    }
}

fn array_shift(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let mut arr_mut = arr.borrow_mut();
        if arr_mut.is_empty() {
            return Ok(Value::Null);
        }
        if let Some(first) = arr_mut.shift_packed() {
            drop(arr_mut);
            // RETVAL_COPY_DEREF(); the array's slot is released.
            let v = first.deref();
            goro_core::gc::on_child_released(&first);
            return Ok(v);
        }
        let entries: Vec<_> = arr_mut
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        // RETVAL_COPY_DEREF()
        let first_val = entries[0].1.deref();
        // Re-index: rebuild array with sequential integer keys
        let mut new_arr = PhpArray::new();
        for (key, val) in entries.iter().skip(1) {
            match key {
                goro_core::array::ArrayKey::String(s) => {
                    new_arr.set(goro_core::array::ArrayKey::String(s.clone()), val.clone());
                }
                goro_core::array::ArrayKey::Int(_) => {
                    new_arr.push(val.clone()); // Re-index integer keys
                }
            }
        }
        *arr_mut = new_arr;
        Ok(first_val)
    } else {
        Ok(Value::Null)
    }
}

fn array_unshift(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        // Deep-clone the new values BEFORE we borrow_mut, to avoid
        // recursive self-reference when the user passes $arr itself as a value.
        let new_values: Vec<Value> = args[1..].iter().map(deep_clone_value).collect();

        let mut arr_mut = arr.borrow_mut();
        let existing: Vec<_> = arr_mut
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        let mut new_arr = PhpArray::new();
        // Prepended values always get sequential integer keys
        for val in &new_values {
            new_arr.push(val.clone());
        }
        // Existing entries: string keys preserved, integer keys re-indexed
        for (key, val) in &existing {
            match key {
                ArrayKey::String(s) => {
                    new_arr.set(ArrayKey::String(s.clone()), val.clone());
                }
                ArrayKey::Int(_) => {
                    new_arr.push(val.clone());
                }
            }
        }
        // Iterators over the array stay on their elements.
        let n = new_values.len();
        arr_mut.iterators_splice(0, 0, n);
        arr_mut.iterators_move_to(&mut new_arr);
        *arr_mut = new_arr;

        Ok(Value::Long((new_values.len() + existing.len()) as i64))
    } else {
        Ok(Value::Long(0))
    }
}

fn array_keys(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let search_value = args.get(1);
        let strict = args.get(2).map(|v| v.is_truthy()).unwrap_or(false);
        let arr = arr.borrow();
        let mut result = PhpArray::new();
        for (key, val) in arr.iter() {
            let include = if let Some(search) = search_value {
                if strict {
                    val.identical(search)
                } else {
                    val.equals(search)
                }
            } else {
                true
            };
            if include {
                let key_val = match key {
                    goro_core::array::ArrayKey::Int(n) => Value::Long(*n),
                    goro_core::array::ArrayKey::String(s) => Value::String(s.clone()),
                };
                result.push(key_val);
            }
        }
        Ok(Value::Array(Rc::new(RefCell::new(result))))
    } else {
        Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
    }
}

fn array_values(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let arr = arr.borrow();
        let mut result = PhpArray::new();
        for (_, val) in arr.iter() {
            result.push(unwrap_rc1_ref(val));
        }
        Ok(Value::Array(Rc::new(RefCell::new(result))))
    } else {
        Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
    }
}

/// Copy an array element value for merging: rc=1 references become values,
/// nested arrays get their own storage.
fn merge_copy_entry(v: &Value) -> Value {
    match v {
        Value::Reference(r) if Rc::strong_count(r) == 1 => {
            let inner = r.borrow().clone();
            merge_copy_entry(&inner)
        }
        Value::Array(a) => Value::Array(Rc::new(RefCell::new(a.borrow().clone()))),
        other => other.clone(),
    }
}

/// php_array_merge_wrapper()
fn php_array_merge_wrapper(
    vm: &mut Vm,
    args: &[Value],
    fname: &str,
    recursive: bool,
) -> Result<Value, VmError> {
    let mut arrays: Vec<Rc<RefCell<PhpArray>>> = Vec::new();
    let mut count: u64 = 0;
    for (i, arg) in args.iter().enumerate() {
        match arg.deref() {
            Value::Array(a) => {
                count += a.borrow().len() as u64;
                arrays.push(a);
            }
            _ => {
                require_array_arg_variadic(vm, arg, fname, (i + 1) as u32)?;
                return Ok(Value::Null);
            }
        }
    }
    if arrays.is_empty() {
        return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
    }
    if count >= 0x40000000 {
        let msg = format!(
            "The total number of elements must be lower than {}",
            0x40000000u32
        );
        let exc = vm.create_exception(b"Error", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    let mut dest = PhpArray::new();
    for (k, v) in arrays[0].borrow().iter() {
        let v = merge_copy_entry(v);
        match k {
            ArrayKey::String(_) => dest.set(k.clone(), v),
            ArrayKey::Int(_) => dest.push(v),
        }
    }
    let dest = Rc::new(RefCell::new(dest));
    for src in &arrays[1..] {
        if recursive {
            let mut stack = Vec::new();
            if !php_array_merge_recursive(vm, &dest, src, &mut stack) {
                if vm.current_exception.is_some() {
                    return Err(VmError {
                        message: vm.uncaught_exception_message(),
                        line: vm.current_line,
                    });
                }
                return Ok(Value::Null);
            }
        } else {
            let entries: Vec<(ArrayKey, Value)> = src
                .borrow()
                .iter()
                .map(|(k, v)| (k.clone(), merge_copy_entry(v)))
                .collect();
            let mut d = dest.borrow_mut();
            for (k, v) in entries {
                match k {
                    ArrayKey::String(_) => d.set(k, v),
                    ArrayKey::Int(_) => d.push(v),
                }
            }
        }
    }
    let out = Rc::try_unwrap(dest)
        .map(|c| c.into_inner())
        .unwrap_or_else(|rc| rc.borrow().clone());
    Ok(Value::Array(Rc::new(RefCell::new(out))))
}

/// convert_to_array() of a merged value.
fn merge_to_array(vm: &mut Vm, v: &Value) -> PhpArray {
    match v {
        Value::Array(a) => a.borrow().clone(),
        other => vm.convert_to_array(other.clone()).borrow().clone(),
    }
}

/// php_array_merge_recursive()
fn php_array_merge_recursive(
    vm: &mut Vm,
    dest: &Rc<RefCell<PhpArray>>,
    src: &Rc<RefCell<PhpArray>>,
    stack: &mut Vec<usize>,
) -> bool {
    let entries: Vec<(ArrayKey, Value)> = src
        .borrow()
        .iter()
        .map(|(k, v)| (k.clone(), merge_copy_entry(v)))
        .collect();
    for (k, src_entry) in entries {
        match &k {
            ArrayKey::String(_) => {
                let dest_entry = dest.borrow().get(&k).cloned();
                match dest_entry {
                    Some(de) => {
                        let src_val = src_entry.deref();
                        let dval = de.deref();
                        if let Value::Array(t) = &dval {
                            if stack.contains(&(Rc::as_ptr(t) as usize)) {
                                let exc = vm.create_exception(
                                    b"Error",
                                    "Recursion detected",
                                    vm.current_line,
                                );
                                vm.current_exception = Some(exc);
                                return false;
                            }
                        }
                        let thash_ptr = if let Value::Array(t) = &dval {
                            Some(Rc::as_ptr(t) as usize)
                        } else {
                            None
                        };
                        let mut darr = if matches!(dval, Value::Null | Value::Undef) {
                            let mut a = PhpArray::new();
                            a.push(Value::Null);
                            a
                        } else {
                            merge_to_array(vm, &dval)
                        };
                        let src_val = if let Value::Object(_) = &src_val {
                            Value::Array(Rc::new(RefCell::new(merge_to_array(vm, &src_val))))
                        } else {
                            src_val
                        };
                        if let Value::Array(sa) = &src_val {
                            if let Some(p) = thash_ptr {
                                stack.push(p);
                            }
                            let drc = Rc::new(RefCell::new(darr));
                            let ok = php_array_merge_recursive(vm, &drc, sa, stack);
                            if thash_ptr.is_some() {
                                stack.pop();
                            }
                            if !ok {
                                return false;
                            }
                            darr = Rc::try_unwrap(drc)
                                .map(|c| c.into_inner())
                                .unwrap_or_else(|rc| rc.borrow().clone());
                        } else if let Err(msg) = darr.try_push(src_val.clone()) {
                            let exc = vm.create_exception(b"Error", msg, vm.current_line);
                            vm.current_exception = Some(exc);
                            return false;
                        }
                        // SEPARATE_ZVAL(dest_entry): a reference in the
                        // destination is replaced, not written through.
                        let newv = Value::Array(Rc::new(RefCell::new(darr)));
                        dest.borrow_mut().set(k.clone(), newv);
                    }
                    None => {
                        let v = merge_copy_entry(&src_entry);
                        dest.borrow_mut().set(k.clone(), v);
                    }
                }
            }
            ArrayKey::Int(_) => {
                let v = merge_copy_entry(&src_entry);
                let pushed = dest.borrow_mut().try_push(v);
                if let Err(msg) = pushed {
                    let exc = vm.create_exception(b"Error", msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    return false;
                }
            }
        }
    }
    true
}

fn array_merge(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    php_array_merge_wrapper(vm, args, "array_merge", false)
}

fn array_reverse(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let preserve_keys = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
        let arr = arr.borrow();
        let entries: Vec<_> = arr
            .iter()
            .map(|(k, v)| (k.clone(), unwrap_rc1_ref(v)))
            .collect();
        let mut result = PhpArray::new();
        for (key, val) in entries.into_iter().rev() {
            if preserve_keys {
                result.set(key, val);
            } else {
                match key {
                    goro_core::array::ArrayKey::Int(_) => result.push(val),
                    goro_core::array::ArrayKey::String(s) => {
                        result.set(goro_core::array::ArrayKey::String(s), val)
                    }
                }
            }
        }
        Ok(Value::Array(Rc::new(RefCell::new(result))))
    } else {
        Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
    }
}

fn array_flip(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let first = args.first().unwrap_or(&Value::Null);
    require_array_arg(vm, first, "array_flip", "array", 1)?;
    if let Some(Value::Array(arr)) = args.first() {
        let arr = arr.borrow();
        let mut result = PhpArray::new();
        for (key, val) in arr.iter() {
            let val = &val.deref();
            let new_key = match val {
                Value::Long(n) => goro_core::array::ArrayKey::Int(*n),
                Value::String(s) => {
                    // Convert numeric strings to integer keys
                    let s_str = s.to_string_lossy();
                    if let Ok(n) = s_str.parse::<i64>() {
                        if n.to_string() == s_str.as_ref() {
                            goro_core::array::ArrayKey::Int(n)
                        } else {
                            goro_core::array::ArrayKey::String(s.clone())
                        }
                    } else {
                        goro_core::array::ArrayKey::String(s.clone())
                    }
                }
                _ => {
                    vm.emit_warning(
                        "array_flip(): Can only flip string and integer values, entry skipped",
                    );
                    continue;
                }
            };
            let new_val = match key {
                goro_core::array::ArrayKey::Int(n) => Value::Long(*n),
                goro_core::array::ArrayKey::String(s) => Value::String(s.clone()),
            };
            result.set(new_key, new_val);
        }
        Ok(Value::Array(Rc::new(RefCell::new(result))))
    } else {
        Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
    }
}

fn array_unique(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let first = args.first().unwrap_or(&Value::Null);
    require_array_arg(vm, first, "array_unique", "array", 1)?;
    if let Some(Value::Array(arr)) = args.first() {
        let flags = args.get(1).map(|v| v.to_long()).unwrap_or(2); // SORT_STRING = 2 default
        if flags == SORT_STRING {
            // SORT_STRING: keep the first element of each string value
            // (zval_get_tmp_string(): "Array to string conversion",
            // __toString(), or an Error for unconvertible objects).
            // (zval_add_ref(): a reference only the array holds is copied
            // as its value)
            let entries: Vec<(ArrayKey, Value)> = arr
                .borrow()
                .iter()
                .map(|(k, v)| (k.clone(), unwrap_rc1_ref(v)))
                .collect();
            let mut result = PhpArray::new();
            let mut seen: std::collections::HashSet<Vec<u8>> = std::collections::HashSet::new();
            for (key, val) in entries {
                let s = match val.deref() {
                    Value::String(s) => s.as_bytes().to_vec(),
                    Value::Array(_) => {
                        vm.emit_warning("Array to string conversion");
                        b"Array".to_vec()
                    }
                    other => vm.value_to_string_checked(&other)?.as_bytes().to_vec(),
                };
                if vm.current_exception.is_some() {
                    return Ok(Value::Null);
                }
                if seen.insert(s) {
                    result.set(key, val);
                }
            }
            return Ok(Value::Array(Rc::new(RefCell::new(result))));
        }
        array_unique_sorted(vm, arr, flags)
    } else {
        Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
    }
}

/// array_unique() for the non-SORT_STRING flags: zend_sort() the buckets
/// with the unstable data comparator, then drop each run of equal
/// neighbours but the earliest one (ext/standard/array.c).
fn array_unique_sorted(
    vm: &mut Vm,
    arr: &Rc<RefCell<PhpArray>>,
    flags: i64,
) -> Result<Value, VmError> {
    let mut result = arr.borrow().clone();
    let mut tmp: Vec<(usize, ArrayKey, Value)> = arr
        .borrow()
        .iter()
        .enumerate()
        .map(|(i, (k, v))| (i, k.clone(), v.clone()))
        .collect();
    if tmp.len() <= 1 {
        return Ok(Value::Array(Rc::new(RefCell::new(result))));
    }
    crate::zsort::zend_sort(&mut tmp, &mut |a, b| php_data_cmp(vm, &a.2, &b.2, flags));
    if vm.current_exception.is_some() {
        return Ok(Value::Null);
    }
    let mut lastkept = 0usize;
    for c in 1..tmp.len() {
        if php_data_cmp(vm, &tmp[lastkept].2, &tmp[c].2, flags) != 0 {
            lastkept = c;
        } else {
            let del = if tmp[lastkept].0 > tmp[c].0 {
                std::mem::replace(&mut lastkept, c)
            } else {
                c
            };
            result.remove(&tmp[del].1);
        }
    }
    for (_, v) in result.iter_mut() {
        *v = unwrap_rc1_ref(v);
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn array_slice(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Validate $length is int or null
    if let Some(v) = args.get(2) {
        match v.deref() {
            Value::Null | Value::Long(_) | Value::True | Value::False => {}
            other => {
                let type_name = Vm::value_type_name(&other);
                let msg = format!(
                    "array_slice(): Argument #3 ($length) must be of type ?int, {} given",
                    type_name
                );
                let exc = vm.create_exception(b"TypeError", &msg, 0);
                vm.current_exception = Some(exc);
                return Err(VmError {
                    message: msg,
                    line: vm.current_line,
                });
            }
        }
    }
    if let Some(Value::Array(arr)) = args.first() {
        let arr = arr.borrow();
        let entries: Vec<_> = arr.iter().collect();
        let offset = args.get(1).map(|v| v.to_long()).unwrap_or(0);
        let length = match args.get(2) {
            Some(Value::Null) | None => None,
            Some(v) => Some(v.to_long()),
        };
        let preserve_keys = args.get(3).map(|v| v.is_truthy()).unwrap_or(false);

        let start = if offset < 0 {
            (entries.len() as i64 + offset).max(0) as usize
        } else {
            offset as usize
        };
        let end = match length {
            Some(l) if l < 0 => (entries.len() as i64 + l).max(start as i64) as usize,
            Some(l) => start.saturating_add(l as usize).min(entries.len()),
            None => entries.len(),
        };

        let mut result = PhpArray::new();
        let end = end.min(entries.len());
        let start = start.min(end);
        for &(ref k, v) in &entries[start..end] {
            match k {
                ArrayKey::String(s) => {
                    // String keys are always preserved
                    result.set(ArrayKey::String(s.clone()), unwrap_rc1_ref(v));
                }
                ArrayKey::Int(i) => {
                    if preserve_keys {
                        result.set(ArrayKey::Int(*i), unwrap_rc1_ref(v));
                    } else {
                        result.push(unwrap_rc1_ref(v));
                    }
                }
            }
        }
        Ok(Value::Array(Rc::new(RefCell::new(result))))
    } else {
        Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
    }
}

fn array_splice(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let used_ret = vm.call_result_unused.take().as_deref() != Some(b"array_splice".as_slice());
    let first = args.first().unwrap_or(&Value::Null);
    let arr_opt = match first {
        Value::Array(a) => Some(a.clone()),
        Value::Reference(r) => match &*r.borrow() {
            Value::Array(a) => Some(a.clone()),
            _ => None,
        },
        _ => None,
    };
    let Some(arr) = arr_opt else {
        return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
    };
    let mut offset = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let num_in = arr.borrow().len() as i64;
    // length is nullable: null means remove everything from offset
    let mut length = match args.get(2) {
        Some(Value::Null) | None => num_in,
        Some(v) => v.to_long(),
    };
    // convert_to_array() of the replacement.
    let replacement: Option<Vec<Value>> = match args.get(3).map(|v| v.deref()) {
        None => None,
        Some(Value::Array(a)) => Some(a.borrow().values().cloned().collect()),
        // (array) of a Closure is [closure]
        Some(c @ Value::Object(_)) if c.is_closure_obj() => Some(vec![c]),
        Some(Value::Object(obj)) => Some(
            vm.object_to_array(&obj)
                .borrow()
                .values()
                .cloned()
                .collect(),
        ),
        Some(Value::Null | Value::Undef) => Some(Vec::new()),
        Some(other) => Some(vec![other]),
    };

    // php_splice(): clamp the offset and the length.
    if offset > num_in {
        offset = num_in;
    } else if offset < 0 {
        offset = (num_in + offset).max(0);
    }
    if length < 0 {
        length += num_in - offset;
    } else if (offset as u32).wrapping_add(length as u32) > num_in as u32 {
        length = num_in - offset;
    }
    let start = offset as usize;
    let end = if length <= 0 {
        start
    } else {
        start.saturating_add(length as usize).min(num_in as usize)
    };

    let entries: Vec<(ArrayKey, Value)> = arr
        .borrow()
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    // (a destructor run below gives this call a backtrace frame holding its
    // arguments: count its holders once that frame exists)
    if !used_ret
        && entries
            .iter()
            .any(|(_, v)| matches!(v.deref(), Value::Object(_) | Value::Array(_)))
    {
        vm.sync_builtin_frames();
    }
    let strong_before = Rc::strong_count(&arr);

    // Integer keys are renumbered, string keys kept.
    let mut new_arr = PhpArray::new();
    let mut removed_arr = PhpArray::new();
    for (i, (k, v)) in entries.iter().enumerate() {
        if i == start {
            if let Some(repl) = &replacement {
                for v in repl {
                    new_arr.push(v.clone());
                }
            }
        }
        let target = if i >= start && i < end {
            &mut removed_arr
        } else {
            &mut new_arr
        };
        match k {
            ArrayKey::String(_) => target.set(k.clone(), v.clone()),
            ArrayKey::Int(_) => target.push(v.clone()),
        }
    }
    if start >= entries.len() {
        if let Some(repl) = &replacement {
            for v in repl {
                new_arr.push(v.clone());
            }
        }
    }
    drop(entries);
    {
        // php_splice(): iterators over the array follow their elements.
        let mut old = arr.borrow_mut();
        let inserted = replacement.as_ref().map_or(0, |r| r.len());
        old.iterators_splice(start, end - start, inserted);
        old.iterators_move_to(&mut new_arr);
        *old = new_arr;
    }

    if !used_ret {
        // The removed elements are released now; a destructor that replaces
        // or modifies the array makes the splice fail (php_splice() holds an
        // extra reference, so such a write separates the array from it).
        let may_destruct = removed_arr
            .values()
            .any(|v| matches!(v.deref(), Value::Object(_) | Value::Array(_)));
        let snapshot: Option<Vec<(ArrayKey, Value)>> = may_destruct.then(|| {
            arr.borrow()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        });
        drop(removed_arr);
        vm.run_pending_destructors_pub();
        if vm.current_exception.is_some() {
            return Ok(Value::Null);
        }
        let modified = snapshot.is_some_and(|snap| {
            let a = arr.borrow();
            a.len() != snap.len()
                || a.iter()
                    .zip(snap.iter())
                    .any(|((k, v), (sk, sv))| k != sk || !v.identical(sv))
        });
        if modified || Rc::strong_count(&arr) < strong_before {
            let msg = "Array was modified during array_splice operation";
            let exc = vm.create_exception(b"Error", msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg.to_string(),
                line: vm.current_line,
            });
        }
        return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
    }
    Ok(Value::Array(Rc::new(RefCell::new(removed_arr))))
}

fn array_search(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let needle = args.first().unwrap_or(&Value::Null);
    let strict = args.get(2).map(|v| v.is_truthy()).unwrap_or(false);
    let haystack_raw = args.get(1).cloned().unwrap_or(Value::Null);
    let haystack = match &haystack_raw {
        Value::Reference(r) => r.borrow().clone(),
        v => v.clone(),
    };
    if let Value::Array(arr) = &haystack {
        let entries: Vec<(ArrayKey, Value)> = arr
            .borrow()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for (key, val) in entries {
            let matches = if strict {
                val.identical(needle)
            } else {
                vm.loose_equals(needle, &val)
            };
            if vm.current_exception.is_some() {
                return Ok(Value::Null);
            }
            if matches {
                return Ok(match key {
                    ArrayKey::Int(n) => Value::Long(n),
                    ArrayKey::String(s) => Value::String(s),
                });
            }
        }
        return Ok(Value::False);
    }
    // Throw TypeError for non-array haystack
    let type_name = Vm::value_type_name(&haystack);
    let msg = format!(
        "array_search(): Argument #2 ($haystack) must be of type array, {} given",
        type_name
    );
    let exc = vm.throw_type_error(msg.clone());
    vm.current_exception = Some(exc);
    Err(VmError {
        message: msg,
        line: vm.current_line,
    })
}

fn in_array(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let needle = args.first().unwrap_or(&Value::Null);
    let haystack_raw = args.get(1).cloned().unwrap_or(Value::Null);
    let haystack = match &haystack_raw {
        Value::Reference(r) => r.borrow().clone(),
        v => v.clone(),
    };
    if let Value::Array(arr) = &haystack {
        let strict = args.get(2).map(|v| v.is_truthy()).unwrap_or(false);
        if strict {
            let found = arr.borrow().values().any(|val| val.identical(needle));
            return Ok(if found { Value::True } else { Value::False });
        }
        let values: Vec<Value> = arr.borrow().values().cloned().collect();
        for val in values {
            let eq = vm.loose_equals(needle, &val);
            if vm.current_exception.is_some() {
                return Ok(Value::Null);
            }
            if eq {
                return Ok(Value::True);
            }
        }
        return Ok(Value::False);
    }
    // Throw TypeError for non-array haystack
    let type_name = Vm::value_type_name(&haystack);
    let msg = format!(
        "in_array(): Argument #2 ($haystack) must be of type array, {} given",
        type_name
    );
    let exc = vm.throw_type_error(msg.clone());
    vm.current_exception = Some(exc);
    Err(VmError {
        message: msg,
        line: vm.current_line,
    })
}

/// PHP_FUNCTION(getrusage) (ext/standard/microtime.c)
fn getrusage_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let who = if args.first().map(|v| v.to_long()) == Some(1) {
        libc::RUSAGE_CHILDREN
    } else {
        libc::RUSAGE_SELF
    };
    // SAFETY: getrusage() fills the zeroed struct it is given.
    let mut u: libc::rusage = unsafe { std::mem::zeroed() };
    if unsafe { libc::getrusage(who, &mut u) } == -1 {
        return Ok(Value::False);
    }
    let mut arr = PhpArray::new();
    for (k, v) in [
        ("ru_oublock", u.ru_oublock as i64),
        ("ru_inblock", u.ru_inblock as i64),
        ("ru_msgsnd", u.ru_msgsnd as i64),
        ("ru_msgrcv", u.ru_msgrcv as i64),
        ("ru_maxrss", u.ru_maxrss as i64),
        ("ru_ixrss", u.ru_ixrss as i64),
        ("ru_idrss", u.ru_idrss as i64),
        ("ru_minflt", u.ru_minflt as i64),
        ("ru_majflt", u.ru_majflt as i64),
        ("ru_nsignals", u.ru_nsignals as i64),
        ("ru_nvcsw", u.ru_nvcsw as i64),
        ("ru_nivcsw", u.ru_nivcsw as i64),
        ("ru_nswap", u.ru_nswap as i64),
        ("ru_utime.tv_usec", u.ru_utime.tv_usec as i64),
        ("ru_utime.tv_sec", u.ru_utime.tv_sec as i64),
        ("ru_stime.tv_usec", u.ru_stime.tv_usec as i64),
        ("ru_stime.tv_sec", u.ru_stime.tv_sec as i64),
    ] {
        arr.set(
            ArrayKey::String(PhpString::from_bytes(k.as_bytes())),
            Value::Long(v),
        );
    }
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

fn assoc(pairs: Vec<(&str, Value)>) -> PhpArray {
    let mut arr = PhpArray::new();
    for (k, v) in pairs {
        arr.set(ArrayKey::String(PhpString::from_bytes(k.as_bytes())), v);
    }
    arr
}

/// PHP_FUNCTION(strptime) (ext/standard/datetime.c)
fn strptime_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    vm.emit_function_deprecation(
        "Function strptime() is deprecated since 8.2, use date_parse_from_format() (for locale-independent parsing), or IntlDateFormatter::parse() (for locale-dependent parsing) instead",
    );
    let args = &vm.zpp_self_check(b"strptime", args)?[..];
    let ts = args
        .first()
        .map(|v| v.to_php_string().as_bytes().to_vec())
        .unwrap_or_default();
    let format = args
        .get(1)
        .map(|v| v.to_php_string().as_bytes().to_vec())
        .unwrap_or_default();
    let cstr = |b: &[u8]| {
        let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
        std::ffi::CString::new(&b[..end]).unwrap_or_default()
    };
    let (ts, format) = (cstr(&ts), cstr(&format));
    // SAFETY: both strings are NUL-terminated; strptime() fills the zeroed
    // tm and returns a pointer into `ts` (or NULL).
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let rest = unsafe { libc::strptime(ts.as_ptr(), format.as_ptr(), &mut tm) };
    if rest.is_null() {
        return Ok(Value::False);
    }
    let unparsed = unsafe { std::ffi::CStr::from_ptr(rest) }
        .to_bytes()
        .to_vec();
    let arr = assoc(vec![
        ("tm_sec", Value::Long(tm.tm_sec as i64)),
        ("tm_min", Value::Long(tm.tm_min as i64)),
        ("tm_hour", Value::Long(tm.tm_hour as i64)),
        ("tm_mday", Value::Long(tm.tm_mday as i64)),
        ("tm_mon", Value::Long(tm.tm_mon as i64)),
        ("tm_year", Value::Long(tm.tm_year as i64)),
        ("tm_wday", Value::Long(tm.tm_wday as i64)),
        ("tm_yday", Value::Long(tm.tm_yday as i64)),
        ("unparsed", Value::String(PhpString::from_vec(unparsed))),
    ]);
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

/// PHP_FUNCTION(proc_nice) (ext/standard/exec.c)
fn proc_nice_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let pri = args.first().map(|v| v.to_long()).unwrap_or(0);
    let pri = pri.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    // SAFETY: nice() only changes this process' priority; errno tells a
    // failure apart from a -1 result.
    let failed = unsafe {
        *libc::__errno_location() = 0;
        libc::nice(pri);
        *libc::__errno_location() != 0
    };
    if failed {
        vm.emit_warning(
            "proc_nice(): Only a super user may attempt to increase the priority of a process",
        );
        return Ok(Value::False);
    }
    Ok(Value::True)
}

/// php_inet_ntop()
fn sockaddr_to_string(addr: *const libc::sockaddr) -> Option<String> {
    if addr.is_null() {
        return None;
    }
    // SAFETY: getifaddrs() hands out sockaddrs of the size their family
    // implies.
    unsafe {
        match (*addr).sa_family as i32 {
            libc::AF_INET => {
                let a = &*(addr as *const libc::sockaddr_in);
                Some(std::net::Ipv4Addr::from(u32::from_be(a.sin_addr.s_addr)).to_string())
            }
            libc::AF_INET6 => {
                let a = &*(addr as *const libc::sockaddr_in6);
                Some(std::net::Ipv6Addr::from(a.sin6_addr.s6_addr).to_string())
            }
            _ => None,
        }
    }
}

/// PHP_FUNCTION(net_get_interfaces) (ext/standard/net.c, getifaddrs())
fn net_get_interfaces_fn(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut addrs: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: the list getifaddrs() allocates is walked, then freed.
    if unsafe { libc::getifaddrs(&mut addrs) } != 0 {
        let e = std::io::Error::last_os_error();
        vm.emit_warning(&format!(
            "getifaddrs() failed {}: {}",
            e.raw_os_error().unwrap_or(0),
            e
        ));
        return Ok(Value::False);
    }
    type Iface = (Vec<u8>, Vec<PhpArray>, bool);
    let mut ifaces: Vec<Iface> = Vec::new();
    let mut p = addrs;
    while !p.is_null() {
        let ifa = unsafe { &*p };
        let name = unsafe { std::ffi::CStr::from_ptr(ifa.ifa_name) }
            .to_bytes()
            .to_vec();
        let flags = ifa.ifa_flags;
        let mut u: Vec<(&str, Value)> = vec![("flags", Value::Long(flags as i64))];
        if !ifa.ifa_addr.is_null() {
            let family = unsafe { (*ifa.ifa_addr).sa_family } as i64;
            u.push(("family", Value::Long(family)));
            if let Some(h) = sockaddr_to_string(ifa.ifa_addr) {
                u.push(("address", Value::String(PhpString::from_string(h))));
            }
        }
        if let Some(h) = sockaddr_to_string(ifa.ifa_netmask) {
            u.push(("netmask", Value::String(PhpString::from_string(h))));
        }
        // (ifa_broadaddr / ifa_dstaddr share a union)
        if flags & libc::IFF_BROADCAST as u32 != 0 {
            if let Some(h) = sockaddr_to_string(ifa.ifa_ifu) {
                u.push(("broadcast", Value::String(PhpString::from_string(h))));
            }
        }
        if flags & libc::IFF_POINTOPOINT as u32 != 0 {
            if let Some(h) = sockaddr_to_string(ifa.ifa_ifu) {
                u.push(("ptp", Value::String(PhpString::from_string(h))));
            }
        }
        let u = assoc(u);
        match ifaces.iter_mut().find(|(n, _, _)| *n == name) {
            Some((_, list, _)) => list.push(u),
            None => ifaces.push((name, vec![u], flags & libc::IFF_UP as u32 != 0)),
        }
        p = ifa.ifa_next;
    }
    unsafe { libc::freeifaddrs(addrs) };
    let mut out = PhpArray::new();
    for (name, list, up) in ifaces {
        let mut uni = PhpArray::new();
        for u in list {
            uni.push(Value::Array(Rc::new(RefCell::new(u))));
        }
        let iface = assoc(vec![
            ("unicast", Value::Array(Rc::new(RefCell::new(uni)))),
            ("up", if up { Value::True } else { Value::False }),
        ]);
        out.set(
            ArrayKey::String(PhpString::from_vec(name)),
            Value::Array(Rc::new(RefCell::new(iface))),
        );
    }
    Ok(Value::Array(Rc::new(RefCell::new(out))))
}

/// PHP_FUNCTION(sys_getloadavg)
fn sys_getloadavg_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut load = [0f64; 3];
    // SAFETY: getloadavg() writes at most 3 doubles.
    if unsafe { libc::getloadavg(load.as_mut_ptr(), 3) } == -1 {
        return Ok(Value::False);
    }
    let mut arr = PhpArray::new();
    for l in load {
        arr.push(Value::Double(l));
    }
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

/// zend_fcall_info_init() failure of a zpp `f` parameter: TypeError
/// "<func>(): <label> must be a valid callback[ or null], <error>".
pub(crate) fn require_callable(
    vm: &mut Vm,
    callback: &Value,
    fn_name: &str,
    arg_label: &str,
    or_null: bool,
) -> Result<(), VmError> {
    let err = callable_error(vm, callback);
    if vm.current_exception.is_some() {
        return Err(VmError {
            message: "Uncaught exception".into(),
            line: vm.current_line,
        });
    }
    match err {
        None => Ok(()),
        Some(e) => {
            let msg = format!(
                "{}(): {} must be a valid callback{}, {}",
                fn_name,
                arg_label,
                if or_null { " or null" } else { "" },
                e
            );
            Err(throw_type_error_msg(vm, msg))
        }
    }
}

fn validate_callback_generic(
    vm: &mut Vm,
    callback: &Value,
    fn_name: &str,
    arg_label: &str,
) -> Result<(), VmError> {
    if matches!(callback, Value::Null) {
        return Ok(()); // null is valid (identity/zip)
    }
    require_callable(vm, callback, fn_name, arg_label, true)
}

/// PHP_FUNCTION(array_map)
fn array_map(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.len() < 2 {
        let msg = format!(
            "array_map() expects at least 2 arguments, {} given",
            args.len()
        );
        let exc = vm.create_exception(b"ArgumentCountError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    let callback = args[0].deref();
    let callback = if matches!(callback, Value::Undef) {
        Value::Null
    } else {
        callback
    };
    if !matches!(callback, Value::Null) {
        if let Some(reason) = callable_error(vm, &callback) {
            let msg = format!(
                "array_map(): Argument #1 ($callback) must be a valid callback or null, {}",
                reason
            );
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
    }
    // (each zend_call_function() of a closure releases it: see the end)
    let closure = match &callback {
        Value::Object(o) if o.borrow().closure.is_some() => Some(Rc::downgrade(o)),
        _ => None,
    };
    let r = array_map_impl(vm, args, callback);
    if let Some(o) = closure.and_then(|w| w.upgrade()) {
        goro_core::gc::possible_root_object(&o);
    }
    r
}

fn array_map_impl(vm: &mut Vm, args: &[Value], callback: Value) -> Result<Value, VmError> {
    let callback = callback.closure_callable();
    let mut arrays: Vec<Rc<RefCell<PhpArray>>> = Vec::new();
    for i in 1..args.len() {
        match args[i].deref() {
            Value::Array(a) => arrays.push(a),
            other => {
                let tn = match &other {
                    Value::True => "true".to_string(),
                    Value::False => "false".to_string(),
                    Value::Object(o) => {
                        goro_core::value::display_class_name(&o.borrow().class_name)
                    }
                    _ => Vm::value_type_name(&other).to_string(),
                };
                let msg = if i == 1 {
                    format!(
                        "array_map(): Argument #2 ($array) must be of type array, {} given",
                        tn
                    )
                } else {
                    format!(
                        "array_map(): Argument #{} must be of type array, {} given",
                        i + 1,
                        tn
                    )
                };
                let exc = vm.throw_type_error(msg.clone());
                vm.current_exception = Some(exc);
                return Err(VmError {
                    message: msg,
                    line: vm.current_line,
                });
            }
        }
    }
    if arrays.len() == 1 {
        let arr = &arrays[0];
        if matches!(callback, Value::Null) {
            let copy = arr.borrow().clone();
            return Ok(Value::Array(Rc::new(RefCell::new(copy))));
        }
        let entries: Vec<(ArrayKey, Value)> = arr
            .borrow()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let mut result = PhpArray::new();
        for (k, v) in entries {
            // (a by-ref callback's reference is stored as is)
            let r = vm.call_keeping_ref(|vm| call_user_func(vm, &[callback.clone(), v]))?;
            if vm.current_exception.is_some() {
                return Ok(Value::Null);
            }
            result.set(k, r);
        }
        return Ok(Value::Array(Rc::new(RefCell::new(result))));
    }
    let lists: Vec<Vec<Value>> = arrays
        .iter()
        .map(|a| a.borrow().values().cloned().collect())
        .collect();
    let maxlen = lists.iter().map(|l| l.len()).max().unwrap_or(0);
    let mut result = PhpArray::new();
    for i in 0..maxlen {
        let row: Vec<Value> = lists
            .iter()
            .map(|l| l.get(i).cloned().unwrap_or(Value::Null))
            .collect();
        if matches!(callback, Value::Null) {
            let mut sub = PhpArray::new();
            for v in row {
                sub.push(v);
            }
            result.push(Value::Array(Rc::new(RefCell::new(sub))));
        } else {
            let mut cargs = vec![callback.clone()];
            cargs.extend(row);
            let r = vm.call_keeping_ref(|vm| call_user_func(vm, &cargs))?;
            if vm.current_exception.is_some() {
                return Ok(Value::Null);
            }
            result.push(r);
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn array_filter(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let first = args.first().unwrap_or(&Value::Null);
    require_array_arg(vm, first, "array_filter", "array", 1)?;
    if let Some(Value::Array(arr)) = args.first() {
        // (zval_add_ref(): a reference only the array holds is copied as
        // its value)
        let arr_data: Vec<(ArrayKey, Value)> = arr
            .borrow()
            .iter()
            .map(|(k, v)| match v {
                Value::Reference(r) if Rc::strong_count(r) == 1 => (k.clone(), v.deref()),
                _ => (k.clone(), v.clone()),
            })
            .collect();
        let callback = args.get(1);
        let mode = args.get(2).map(|v| v.to_long()).unwrap_or(0);
        // mode: 0 = default (pass value)
        //        1 = ARRAY_FILTER_USE_BOTH (pass value and key)
        //        2 = ARRAY_FILTER_USE_KEY (pass key only)
        let mut result = PhpArray::new();

        // Treat null/undef callback same as no callback
        let has_callback = match callback {
            Some(Value::Null) | Some(Value::Undef) | None => false,
            Some(_) => true,
        };

        if has_callback {
            let cb = callback.unwrap().clone();
            validate_callback_generic(vm, &cb, "array_filter", "Argument #2 ($callback)")?;

            // Pre-validate arg count for known string-named user functions / builtins.
            // array_filter calls the callback with `n_args`:
            //   mode 0: 1 arg (value)
            //   mode 1: 2 args (value, key)
            //   mode 2: 1 arg (key)
            let n_args: u32 = if mode == 1 { 2 } else { 1 };
            if let Value::String(s) = &cb {
                let func_name = s.as_bytes().to_vec();
                let func_lower: Vec<u8> =
                    func_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                if let Some(user_fn) = vm.user_functions.get(&func_lower).cloned() {
                    let required = user_fn.required_param_count;
                    if n_args < required {
                        let display_name =
                            String::from_utf8_lossy(user_fn.effective_name()).into_owned();
                        let total_params = user_fn.param_count;
                        let has_optional = total_params > required;
                        let qualifier = if has_optional { "at least" } else { "exactly" };
                        let msg = format!(
                            "Too few arguments to function {}(), {} passed and {} {} expected",
                            display_name, n_args, qualifier, required
                        );
                        let exc = vm.create_exception(b"ArgumentCountError", &msg, vm.current_line);
                        vm.current_exception = Some(exc);
                        return Err(VmError {
                            message: format!("Uncaught ArgumentCountError: {}", msg),
                            line: vm.current_line,
                        });
                    }
                }
            }

            // An internal function receives the element's value (its
            // by-value parameter derefs a reference element).
            let internal_cb = match &cb {
                Value::String(s) => {
                    let lower = s.as_bytes().to_ascii_lowercase();
                    let name = lower.strip_prefix(b"\\").unwrap_or(&lower);
                    !vm.user_functions.contains_key(name)
                }
                _ => false,
            };
            for (key, val) in &arr_data {
                let key_val = match key {
                    ArrayKey::Int(n) => Value::Long(*n),
                    ArrayKey::String(s) => Value::String(s.clone()),
                };
                let val = &if internal_cb {
                    val.deref()
                } else {
                    val.clone()
                };
                let keep = match mode {
                    1 => {
                        // ARRAY_FILTER_USE_BOTH - pass value and key
                        call_user_func(vm, &[cb.clone(), val.clone(), key_val])?.is_truthy()
                    }
                    2 => {
                        // ARRAY_FILTER_USE_KEY - pass key to callback
                        call_user_func(vm, &[cb.clone(), key_val])?.is_truthy()
                    }
                    _ => {
                        // Default - pass value to callback
                        call_user_func(vm, &[cb.clone(), val.clone()])?.is_truthy()
                    }
                };
                if keep {
                    result.set(key.clone(), val.clone());
                }
            }
        } else {
            // No callback - filter falsy values
            for (key, val) in &arr_data {
                if val.is_truthy() {
                    result.set(key.clone(), val.clone());
                }
            }
        }

        Ok(Value::Array(Rc::new(RefCell::new(result))))
    } else {
        Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
    }
}

/// zend_is_callable_error(): None if `cb` is callable from the current scope,
/// otherwise the reason PHP reports after "must be a valid callback, ".
pub(crate) fn callable_error(vm: &mut Vm, cb: &Value) -> Option<String> {
    vm.callable_error(cb)
}

fn walk_key_value(k: &ArrayKey) -> Value {
    match k {
        ArrayKey::Int(n) => Value::Long(*n),
        ArrayKey::String(s) => Value::String(s.clone()),
    }
}

/// php_array_walk() for arrays: each element is made a reference (so by-ref
/// callbacks write back) and the callback is invoked through the generic
/// callable path, which handles by-value/by-ref parameters and arity.
///
/// `holder` is the variable (reference) the walked array lives in: as in
/// php_array_walk(), it is reloaded after every callback, so that a callback
/// replacing the array continues with the new one.
fn php_array_walk_arr(
    vm: &mut Vm,
    arr: Rc<RefCell<PhpArray>>,
    holder: Option<&Rc<RefCell<Value>>>,
    cb: &Value,
    userdata: Option<&Value>,
    recursive: bool,
    depth: usize,
) -> Result<bool, VmError> {
    if depth > 256 {
        return Ok(true);
    }
    // While the callback runs, the walk must not hold the array strongly
    // when it is reachable from `holder`: an extra holder would make a
    // write from the callback separate (copy) it.
    let mut weak = Rc::downgrade(&arr);
    let mut strong = if holder.is_some() { None } else { Some(arr) };
    let mut pos = 0usize;
    loop {
        let Some(arr) = strong.clone().or_else(|| weak.upgrade()) else {
            break;
        };
        let entry = {
            // (the element is written: the array's storage is its own
            // first, see PhpArray::make_unique())
            if let Ok(mut a) = arr.try_borrow_mut() {
                a.make_unique();
            }
            let a = arr.borrow();
            a.entry_at(pos).map(|(k, v)| (k.clone(), v.clone()))
        };
        let (key, val) = match entry {
            Some(e) => e,
            None => break,
        };
        // ZVAL_MAKE_REF
        // (an element array shared with a copy is separated: it is written
        // through the reference)
        let r = match val {
            Value::Reference(r) => {
                if recursive {
                    if let Ok(mut inner) = r.try_borrow_mut() {
                        goro_core::array::separate_shared(&mut inner);
                    }
                }
                r
            }
            mut other => {
                // (`other` is one more holder)
                if matches!(&other, Value::Array(a) if Rc::strong_count(a) > 2) {
                    goro_core::array::separate_shared(&mut other);
                }
                let r = Rc::new(RefCell::new(other));
                arr.borrow_mut()
                    .set(key.clone(), Value::Reference(r.clone()));
                r
            }
        };
        let inner_arr = if recursive {
            if let Value::Array(sub) = &*r.borrow() {
                Some(sub.clone())
            } else {
                None
            }
        } else {
            None
        };
        if let Some(sub) = inner_arr {
            if Rc::ptr_eq(&sub, &arr) {
                vm.emit_warning("array_walk_recursive(): Recursion detected");
                return Ok(true);
            }
            drop(arr);
            if !php_array_walk_arr(vm, sub, Some(&r), cb, userdata, true, depth + 1)? {
                return Ok(false);
            }
        } else {
            drop(arr);
            let mut cargs = vec![
                cb.clone(),
                Value::Reference(r.clone()),
                walk_key_value(&key),
            ];
            if let Some(u) = userdata {
                cargs.push(u.clone());
            }
            call_user_func(vm, &cargs)?;
            if vm.current_exception.is_some() {
                return Ok(false);
            }
        }
        // Reload the array: the callback may have replaced it. A copy of
        // the walked array (separated by a write) still holds the element's
        // reference and continues from the same place.
        if let Some(h) = holder {
            let cur = h.borrow().deref();
            match cur {
                Value::Array(a) => {
                    if weak.upgrade().is_none_or(|w| !Rc::ptr_eq(&a, &w)) {
                        weak = Rc::downgrade(&a);
                        let copied = matches!(a.borrow().get(&key),
                            Some(Value::Reference(x)) if Rc::ptr_eq(x, &r));
                        if !copied {
                            pos = 0;
                            continue;
                        }
                    }
                }
                Value::Object(_) => break,
                _ => {
                    let msg = "Iterated value is no longer an array or object".to_string();
                    let exc = vm.throw_type_error(msg.clone());
                    vm.current_exception = Some(exc);
                    return Err(VmError {
                        message: msg,
                        line: vm.current_line,
                    });
                }
            }
        }
        // Continue after the current key (its position may have moved).
        let Some(arr) = strong.clone().or_else(|| weak.upgrade()) else {
            break;
        };
        let a = arr.borrow();
        if let Some(p) = a.position_of(&key) {
            pos = p + 1;
        }
    }
    let Some(arr) = strong.take().or_else(|| weak.upgrade()) else {
        return Ok(true);
    };
    // References only held by the array (rc=1) are indistinguishable from
    // plain values; store them as such.
    if let Ok(mut a) = arr.try_borrow_mut() {
        for (_, v) in a.iter_mut() {
            let unwrap = match v {
                Value::Reference(r) if Rc::strong_count(r) == 1 => Some(r.borrow().clone()),
                _ => None,
            };
            if let Some(inner) = unwrap {
                *v = inner;
            }
        }
    }
    Ok(true)
}

fn array_walk(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.len() > 3 {
        let msg = format!(
            "array_walk() expects at most 3 arguments, {} given",
            args.len()
        );
        let exc = vm.create_exception(b"ArgumentCountError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    let first = args.first().unwrap_or(&Value::Null);

    // Accept arrays and objects (objects are iterated by their properties)
    let is_array = matches!(first, Value::Array(_))
        || matches!(first, Value::Reference(r) if matches!(&*r.borrow(), Value::Array(_)));
    let is_object = matches!(first, Value::Object(_))
        || matches!(first, Value::Reference(r) if matches!(&*r.borrow(), Value::Object(_)));

    if !is_array && !is_object {
        require_array_arg(vm, first, "array_walk", "array", 1)?;
        return Ok(Value::True);
    }

    let callback_v = match args.get(1) {
        Some(cb) => cb.unwrap_closure(),
        None => return Ok(Value::True),
    };
    let callback = &callback_v;
    if let Some(reason) = callable_error(vm, callback) {
        let msg = format!(
            "array_walk(): Argument #2 ($callback) must be a valid callback, {}",
            reason
        );
        let exc = vm.throw_type_error(msg.clone());
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }

    // For objects, convert properties to entries with PHP-style mangled names
    if is_object {
        let outer = match first {
            Value::Object(o) => o.clone(),
            Value::Reference(r) => {
                if let Value::Object(o) = &*r.borrow() {
                    o.clone()
                } else {
                    return Ok(Value::True);
                }
            }
            _ => return Ok(Value::True),
        };
        // Trigger lazy initialization so all declared props exist, and
        // redirect proxies to the real backing object (so iteration sees
        // the real instance).
        vm.maybe_run_lazy_init(&outer);
        let obj = vm.proxy_redirect(&outer);
        // The property table (zend_std_get_properties): mangled keys, each
        // with the storage key of its slot.
        let entries: Vec<(ArrayKey, Vec<u8>, Value)> = vm
            .object_prop_table(&obj)
            .into_iter()
            .map(|e| {
                let storage = match &e.slot {
                    Some(sl) => sl
                        .moved_key
                        .clone()
                        .filter(|mk| obj.borrow().has_property(mk))
                        .unwrap_or_else(|| sl.storage_key.clone()),
                    None => e.key.clone(),
                };
                (
                    ArrayKey::String(PhpString::from_vec(e.key)),
                    storage,
                    e.value,
                )
            })
            .collect();
        let extra_data = args.get(2);
        return array_walk_object_entries(vm, callback, &entries, extra_data, &obj);
    }

    // Array case
    if let Value::Array(arr) = first.deref() {
        let holder = match first {
            Value::Reference(r) => Some(r),
            _ => None,
        };
        php_array_walk_arr(vm, arr, holder, callback, args.get(2), false, 0)?;
        return Ok(Value::True);
    }

    Ok(Value::True)
}

/// array_walk for objects: iterates props, calls callback with &$value,
/// and writes the modified value back to the actual property name.
fn array_walk_object_entries(
    vm: &mut Vm,
    callback: &Value,
    entries: &[(ArrayKey, Vec<u8>, Value)],
    extra_data: Option<&Value>,
    obj: &Rc<RefCell<goro_core::object::PhpObject>>,
) -> Result<Value, VmError> {
    // (BcMath\Number's properties are virtual: its property table is empty)
    if obj
        .borrow()
        .class_name
        .eq_ignore_ascii_case(b"BcMath\\Number")
    {
        return Ok(Value::True);
    }
    let (func_name, captured) = match callback {
        Value::String(s) => (s.as_bytes().to_vec(), vec![]),
        Value::Array(cb) => {
            let cb = cb.borrow();
            let vals: Vec<Value> = cb.values().cloned().collect();
            if vals.is_empty() {
                return Ok(Value::True);
            }
            (
                vals[0].to_php_string().as_bytes().to_vec(),
                vals[1..].to_vec(),
            )
        }
        _ => return Ok(Value::True),
    };
    let func_lower: Vec<u8> = func_name.iter().map(|b| b.to_ascii_lowercase()).collect();

    if let Some(user_fn) = vm.user_functions.get(&func_lower).cloned() {
        for (key, prop_name, val) in entries {
            // The callback's by-reference parameter binds the property slot
            // itself (typed properties type the reference).
            let slot_ref = vm.object_slot_ref(obj, prop_name);
            let val_ref = slot_ref
                .clone()
                .unwrap_or_else(|| Rc::new(RefCell::new(val.clone())));
            let mut fn_cvs = vec![Value::Undef; user_fn.cv_names.len()];
            let mut idx = 0;
            for cv in &captured {
                if idx < fn_cvs.len() {
                    fn_cvs[idx] = cv.clone();
                    idx += 1;
                }
            }
            if idx < fn_cvs.len() {
                fn_cvs[idx] = Value::Reference(val_ref.clone());
                idx += 1;
            }
            if idx < fn_cvs.len() {
                let key_val = match key {
                    ArrayKey::Int(n) => Value::Long(*n),
                    ArrayKey::String(s) => Value::String(s.clone()),
                };
                fn_cvs[idx] = key_val;
                idx += 1;
            }
            if let Some(extra) = extra_data {
                if idx < fn_cvs.len() {
                    fn_cvs[idx] = extra.clone();
                }
            }
            let r = vm.execute_fn(&user_fn, fn_cvs);
            if let Some(sr) = &slot_ref {
                drop(val_ref);
                vm.object_slot_unref(obj, prop_name, sr);
                r?;
                continue;
            }
            r?;
            // Write modified value back to the object property.
            // Type coercion: if the property has a declared type and the
            // value is incompatible, throw a TypeError (matches PHP's
            // reference-held-by-typed-property behaviour).
            let new_val = val_ref.borrow().clone();
            let class_lower: Vec<u8> = obj
                .borrow()
                .class_name
                .iter()
                .map(|b| b.to_ascii_lowercase())
                .collect();
            let type_check_failed: Option<String> = {
                if let Some(class_def) = vm.get_class_def(&class_lower) {
                    if let Some(p) = class_def
                        .properties
                        .iter()
                        .find(|p| !p.is_static && p.name == *prop_name)
                    {
                        if let Some(t) = &p.property_type {
                            if !vm.value_matches_type(&new_val, t) {
                                let type_name = vm.param_type_name(t);
                                let val_type = value_type_for_error(&new_val);
                                let class_display =
                                    goro_core::value::display_class_name(&obj.borrow().class_name);
                                let prop_display = goro_core::string::msg_str(prop_name);
                                Some(format!(
                                    "Cannot assign {} to reference held by property {}::${} of type {}",
                                    val_type, class_display, prop_display, type_name
                                ))
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            };
            if let Some(msg) = type_check_failed {
                let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
                vm.current_exception = Some(exc);
                // Don't write back, continue to next entry
                continue;
            }
            obj.borrow_mut().set_property(prop_name.clone(), new_val);
        }
    } else if let Some(builtin) = vm.functions.get(&func_lower).copied() {
        for (_, _, val) in entries {
            vm.call_builtin_from_internal(builtin, &func_lower, std::slice::from_ref(val))?;
        }
    }
    Ok(Value::True)
}

fn value_type_for_error(v: &Value) -> &'static str {
    let v = v.deref();
    match &v {
        Value::Null | Value::Undef => "null",
        Value::True | Value::False => "bool",
        Value::Long(_) => "int",
        Value::Double(_) => "float",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
        _ => "mixed",
    }
}

fn array_combine(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let keys = match args.first() {
        Some(Value::Array(a)) => a.borrow(),
        _ => return Ok(Value::False),
    };
    let vals = match args.get(1) {
        Some(Value::Array(a)) => a.borrow(),
        _ => return Ok(Value::False),
    };
    let keys_len = keys.len();
    let vals_len = vals.len();
    if keys_len != vals_len {
        let msg = "array_combine(): Argument #1 ($keys) and argument #2 ($values) must have the same number of elements".to_string();
        let exc = vm.throw_type_error(msg.clone());
        if let Value::Object(obj) = &exc {
            obj.borrow_mut().class_name = b"ValueError".to_vec();
        }
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    let mut result = PhpArray::new();
    let keys_vec: Vec<_> = keys.values().cloned().collect();
    let vals_vec: Vec<_> = vals.values().map(unwrap_rc1_ref).collect();
    drop(keys);
    drop(vals);
    for (k, v) in keys_vec.iter().zip(vals_vec.iter()) {
        // PHP array_combine converts values to strings first, then to array keys
        let key_str = match k.deref() {
            Value::Long(n) => {
                result.set(goro_core::array::ArrayKey::Int(n), unwrap_rc1_ref(v));
                continue;
            }
            Value::String(s) => s,
            Value::Array(_) => {
                vm.emit_warning("Array to string conversion");
                PhpString::from_bytes(b"Array")
            }
            other => vm.value_to_string_checked(&other)?,
        };
        if vm.current_exception.is_some() {
            return Ok(Value::Null);
        }
        let key = {
            let s_str = key_str.to_string_lossy();
            if let Ok(n) = s_str.parse::<i64>() {
                if n.to_string() == s_str.as_ref() {
                    goro_core::array::ArrayKey::Int(n)
                } else {
                    goro_core::array::ArrayKey::String(key_str)
                }
            } else {
                goro_core::array::ArrayKey::String(key_str)
            }
        };
        result.set(key, unwrap_rc1_ref(v));
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn array_chunk(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let size = args.get(1).map(|v| v.to_long()).unwrap_or(1);
        if size < 1 {
            let msg = "array_chunk(): Argument #2 ($length) must be greater than 0".to_string();
            let exc = vm.throw_type_error(msg.clone());
            // Change to ValueError
            if let Value::Object(obj) = &exc {
                obj.borrow_mut().class_name = b"ValueError".to_vec();
            }
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
        let size = size as usize;
        let preserve_keys = args.get(2).map(|v| v.is_truthy()).unwrap_or(false);
        let arr = arr.borrow();
        let entries: Vec<_> = arr
            .iter()
            .map(|(k, v)| (k.clone(), unwrap_rc1_ref(v)))
            .collect();
        let mut result = PhpArray::new();
        for chunk in entries.chunks(size) {
            let mut sub = PhpArray::new();
            for (k, v) in chunk {
                if preserve_keys {
                    sub.set(k.clone(), v.clone());
                } else {
                    sub.push(v.clone());
                }
            }
            result.push(Value::Array(Rc::new(RefCell::new(sub))));
        }
        Ok(Value::Array(Rc::new(RefCell::new(result))))
    } else {
        Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
    }
}

fn array_pad(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let input = args.first().unwrap_or(&Value::Null);
    let pad_size = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let pad_value = args.get(2).map_or(Value::Null, Value::deref_for_storage);

    // PHP caps the target length at HT_MAX_SIZE (2^30 by default). Throw a
    // ValueError rather than trying to allocate a huge array.
    const MAX_ARRAY_SIZE: i64 = 1 << 30;
    if pad_size <= -MAX_ARRAY_SIZE || pad_size >= MAX_ARRAY_SIZE {
        let msg =
            "array_pad(): Argument #2 ($length) must not exceed the maximum allowed array size"
                .to_string();
        let exc = vm.create_exception(b"ValueError", &msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }

    if let Value::Array(arr) = input {
        let arr = arr.borrow();
        let current_len = arr.len() as i64;
        let abs_size = pad_size.unsigned_abs() as usize;

        if abs_size <= current_len as usize {
            // No padding needed, return a copy
            let mut result = PhpArray::new();
            for (k, v) in arr.iter() {
                result.set(k.clone(), v.clone());
            }
            return Ok(Value::Array(Rc::new(RefCell::new(result))));
        }

        let pad_count = abs_size - current_len as usize;
        let mut result = PhpArray::new();

        if pad_size < 0 {
            // Pad at the beginning: pad values reindex from 0, string keys preserved
            for _ in 0..pad_count {
                result.push(pad_value.clone());
            }
            for (k, v) in arr.iter() {
                match k {
                    ArrayKey::Int(_) => {
                        result.push(v.clone());
                    }
                    ArrayKey::String(_) => {
                        result.set(k.clone(), v.clone());
                    }
                }
            }
        } else {
            // Pad at the end: input first with string keys preserved, then pad values
            for (k, v) in arr.iter() {
                match k {
                    ArrayKey::Int(_) => {
                        result.push(v.clone());
                    }
                    ArrayKey::String(_) => {
                        result.set(k.clone(), v.clone());
                    }
                }
            }
            for _ in 0..pad_count {
                result.push(pad_value.clone());
            }
        }

        Ok(Value::Array(Rc::new(RefCell::new(result))))
    } else {
        Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
    }
}

fn array_fill(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let start = args.first().map(|v| v.to_long()).unwrap_or(0);
    let num = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    if num < 0 {
        let msg =
            "array_fill(): Argument #2 ($count) must be greater than or equal to 0".to_string();
        let exc = vm.throw_type_error(msg.clone());
        if let Value::Object(obj) = &exc {
            obj.borrow_mut().class_name = b"ValueError".to_vec();
        }
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    if num > i32::MAX as i64 {
        let msg = "array_fill(): Argument #2 ($count) is too large".to_string();
        let exc = vm.throw_type_error(msg.clone());
        if let Value::Object(obj) = &exc {
            obj.borrow_mut().class_name = b"ValueError".to_vec();
        }
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    if num > 0 && start > i64::MAX - num + 1 {
        let msg = "Cannot add element to the array as the next element is already occupied";
        let exc = vm.create_exception(b"Error", msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg.to_string(),
            line: vm.current_line,
        });
    }
    // array_init_size(): zend_hash_check_size() refuses more than
    // HT_MAX_SIZE elements; below that the allocation may exceed the
    // memory limit.
    if num > 0x4000_0000 {
        let msg = format!(
            "Possible integer overflow in memory allocation ({} * 16 + 16)",
            num
        );
        let line = vm.current_line;
        return Err(vm.emit_fatal_at(&msg, line));
    }
    let bytes = (num as usize).next_power_of_two().max(8) * 16;
    let limit = goro_core::value::memory_get_limit();
    if limit > 0 && goro_core::value::memory_get_usage().saturating_add(bytes) > limit {
        goro_core::value::memory_alloc(bytes);
        return Err(vm.memory_limit_fatal());
    }
    let val = args.get(2).map_or(Value::Null, Value::deref_for_storage);
    let mut result = PhpArray::new();
    for i in 0..num {
        result.set(goro_core::array::ArrayKey::Int(start + i), val.clone());
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn array_fill_keys(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let keys = args.first().unwrap_or(&Value::Null);
    require_array_arg(vm, keys, "array_fill_keys", "keys", 1)?;
    let fill_val = args.get(1).map_or(Value::Null, Value::deref_for_storage);
    let mut result = PhpArray::new();
    if let Value::Array(arr) = keys {
        // Clone iterator so we can call vm methods inside the loop (for __toString).
        let entries: Vec<Value> = arr.borrow().iter().map(|(_, v)| v.clone()).collect();
        for raw in &entries {
            let val = raw.deref();
            // array_fill_keys uses PHP's key-coercion rules, which mirror how
            // hash table inserts work in zend_hash. Float -> string, bool/null
            // -> int(0/1)/"" follow normal rules, but resource/object/array
            // values are coerced via to-string semantics (with the usual
            // warnings).
            let k = match &val {
                Value::Long(n) => ArrayKey::Int(*n),
                // Resources become "Resource id #N" strings.
                Value::Resource(n) => ArrayKey::String(PhpString::from_vec(
                    format!("Resource id #{}", n).into_bytes(),
                )),
                // (zend_symtable_update() of the string: "1" is the key 1)
                Value::Double(_) => Vm::value_to_array_key(Value::String(val.to_php_string())),
                // Only canonical integer strings become int keys ("05" stays a string)
                Value::String(_) => Vm::value_to_array_key(val.clone()),
                Value::True => ArrayKey::Int(1),
                Value::False => ArrayKey::String(PhpString::empty()),
                Value::Null | Value::Undef => ArrayKey::String(PhpString::empty()),
                Value::Array(_) => {
                    // PHP emits a warning and uses literal "Array" as the key.
                    vm.emit_warning("Array to string conversion");
                    ArrayKey::String(PhpString::from_vec(b"Array".to_vec()))
                }
                Value::Object(obj) => {
                    // Use __toString if available; otherwise fall back to to_php_string.
                    let obj_val = Value::Object(obj.clone());
                    if let Some(s) = vm.call_object_method(&obj_val, b"__toString", &[]) {
                        Vm::value_to_array_key(Value::String(s.to_php_string()))
                    } else {
                        ArrayKey::String(val.to_php_string())
                    }
                }
                _ => value_to_array_key(&val),
            };
            result.set(k, fill_val.clone());
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn array_merge_recursive(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    php_array_merge_wrapper(vm, args, "array_merge_recursive", true)
}

// Sort flag constants
const SORT_REGULAR: i64 = 0;
const SORT_NUMERIC: i64 = 1;
const SORT_STRING: i64 = 2;
const SORT_LOCALE_STRING: i64 = 5;
const SORT_NATURAL: i64 = 6;
const SORT_FLAG_CASE: i64 = 8;

/// String conversion used by the sort comparators (zval_get_tmp_string):
/// arrays warn, objects use __toString.
pub(crate) fn sort_cmp_string(vm: &mut Vm, v: &Value) -> Vec<u8> {
    let v = v.deref();
    match &v {
        Value::Array(_) => {
            vm.emit_warning("Array to string conversion");
            b"Array".to_vec()
        }
        Value::Object(_) => {
            if vm.current_exception.is_some() {
                return Vec::new();
            }
            match vm.value_to_string_checked(&v) {
                Ok(s) => s.as_bytes().to_vec(),
                Err(_) => Vec::new(),
            }
        }
        _ => v.to_php_string().as_bytes().to_vec(),
    }
}

/// zval_get_double() as used by numeric_compare_function.
fn sort_cmp_double(vm: &mut Vm, v: &Value) -> f64 {
    let v = v.deref();
    match &v {
        Value::Object(o) => {
            let cn = goro_core::value::display_class_name(&o.borrow().class_name);
            vm.emit_warning(&format!(
                "Object of class {} could not be converted to float",
                cn
            ));
            1.0
        }
        Value::Array(a) => {
            if !a.borrow().is_empty() {
                1.0
            } else {
                0.0
            }
        }
        _ => v.to_double(),
    }
}

fn three_way<T: PartialOrd>(a: T, b: T) -> i32 {
    if a < b {
        -1
    } else if a > b {
        1
    } else {
        0
    }
}

fn bytes_cmp(a: &[u8], b: &[u8]) -> i32 {
    match a.cmp(b) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Greater => 1,
        _ => 0,
    }
}

/// zend_compare() including operator-overloading objects (GMP).
fn sort_regular_cmp(vm: &mut Vm, a: &Value, b: &Value) -> i64 {
    let a = a.deref();
    let b = b.deref();
    if let (Some(cmp_fn), Some(is_type)) = (vm.object_compare, vm.object_is_operator_type) {
        if is_type(&a) || is_type(&b) {
            match cmp_fn(vm, &a, &b) {
                Some(std::cmp::Ordering::Less) => return -1,
                Some(std::cmp::Ordering::Equal) => return 0,
                Some(std::cmp::Ordering::Greater) => return 1,
                // ZEND_UNCOMPARABLE (an exception is pending)
                None if vm.current_exception.is_some() => return 1,
                // left to the objects' compare handlers (BcMath\Number)
                None => {}
            }
        }
    }
    // zend_std_compare_objects(): an object compared with a string is cast
    // with __toString() (or is greater when it cannot be).
    let obj_str = match (&a, &b) {
        (Value::Object(o), Value::String(_)) => Some((o.clone(), true)),
        (Value::String(_), Value::Object(o)) => Some((o.clone(), false)),
        _ => None,
    };
    if let Some((o, object_lhs)) = obj_str {
        let cl = o.borrow().class_name.to_ascii_lowercase();
        let has_tostring = vm
            .classes
            .get(&cl)
            .is_some_and(|c| c.get_method(b"__tostring").is_some());
        if !has_tostring {
            return if object_lhs { 1 } else { -1 };
        }
        let casted = Value::String(vm.value_to_string(&Value::Object(o)));
        return if object_lhs {
            casted.compare_zend(&b)
        } else {
            a.compare_zend(&casted)
        };
    }
    // (zend_compare() casts an object compared with a number, noticing)
    if matches!(
        (&a, &b),
        (Value::Object(_), Value::Long(_) | Value::Double(_))
            | (Value::Long(_) | Value::Double(_), Value::Object(_))
    ) {
        let line = vm.current_line;
        vm.emit_object_comparison_notice(&a, &b, line);
    }
    a.compare_zend(&b)
}

/// php_get_data_compare_func(): unstable value comparison for the given flags.
fn php_data_cmp(vm: &mut Vm, a: &Value, b: &Value, flags: i64) -> i32 {
    let base_flag = flags & !SORT_FLAG_CASE;
    let ci = (flags & SORT_FLAG_CASE) != 0;
    match base_flag {
        SORT_NUMERIC => {
            let x = sort_cmp_double(vm, a);
            let y = sort_cmp_double(vm, b);
            three_way(x, y)
        }
        SORT_LOCALE_STRING => {
            let x = sort_cmp_string(vm, a);
            let y = sort_cmp_string(vm, b);
            crate::locale::locale_strcoll(&x, &y)
        }
        SORT_STRING => {
            let x = sort_cmp_string(vm, a);
            let y = sort_cmp_string(vm, b);
            if ci {
                let lx: Vec<u8> = x.iter().map(|c| c.to_ascii_lowercase()).collect();
                let ly: Vec<u8> = y.iter().map(|c| c.to_ascii_lowercase()).collect();
                bytes_cmp(&lx, &ly)
            } else {
                bytes_cmp(&x, &y)
            }
        }
        SORT_NATURAL => {
            let x = sort_cmp_string(vm, a);
            let y = sort_cmp_string(vm, b);
            crate::zsort::strnatcmp_ex(&x, &y, ci)
        }
        _ => {
            let r = sort_regular_cmp(vm, a, b);
            // php_array_data_compare_unstable_i(): uncomparable enum cases
            // are grouped by identity and shifted to the end.
            if r == 1 {
                if let Some(rp) = enum_identity(b) {
                    return match enum_identity(a) {
                        Some(lp) => three_way(lp, rp),
                        None => -1,
                    };
                }
            }
            three_way(r, 0)
        }
    }
}

fn enum_identity(v: &Value) -> Option<usize> {
    match v.deref() {
        Value::Object(o) if o.borrow().has_property(b"__enum_case") => {
            Some(Rc::as_ptr(&o) as *const u8 as usize)
        }
        _ => None,
    }
}

pub(crate) fn key_to_bytes(k: &ArrayKey) -> Vec<u8> {
    match k {
        ArrayKey::Int(n) => n.to_string().into_bytes(),
        ArrayKey::String(s) => s.as_bytes().to_vec(),
    }
}

pub(crate) fn key_to_value(k: &ArrayKey) -> Value {
    match k {
        ArrayKey::Int(n) => Value::Long(*n),
        ArrayKey::String(s) => Value::String(s.clone()),
    }
}

/// php_get_key_compare_func(): unstable key comparison for the given flags.
fn php_key_cmp(vm: &mut Vm, a: &ArrayKey, b: &ArrayKey, flags: i64) -> i32 {
    let base_flag = flags & !SORT_FLAG_CASE;
    let ci = (flags & SORT_FLAG_CASE) != 0;
    match base_flag {
        SORT_NUMERIC => {
            if let (ArrayKey::Int(x), ArrayKey::Int(y)) = (a, b) {
                return if x > y { 1 } else { -1 };
            }
            let d = |k: &ArrayKey| match k {
                ArrayKey::Int(n) => *n as f64,
                ArrayKey::String(s) => goro_core::value::parse_leading_numeric_string(s.as_bytes())
                    .map(|(v, _)| v)
                    .unwrap_or(0.0),
            };
            three_way(d(a), d(b))
        }
        SORT_LOCALE_STRING => {
            let x = key_to_bytes(a);
            let y = key_to_bytes(b);
            crate::locale::locale_strcoll(&x, &y)
        }
        SORT_STRING => {
            let x = key_to_bytes(a);
            let y = key_to_bytes(b);
            if ci {
                let lx: Vec<u8> = x.iter().map(|c| c.to_ascii_lowercase()).collect();
                let ly: Vec<u8> = y.iter().map(|c| c.to_ascii_lowercase()).collect();
                bytes_cmp(&lx, &ly)
            } else {
                bytes_cmp(&x, &y)
            }
        }
        SORT_NATURAL => crate::zsort::strnatcmp_ex(&key_to_bytes(a), &key_to_bytes(b), ci),
        _ => {
            if let (ArrayKey::Int(x), ArrayKey::Int(y)) = (a, b) {
                return if x > y { 1 } else { -1 };
            }
            let r = sort_regular_cmp(vm, &key_to_value(a), &key_to_value(b));
            if r < 0 {
                -1
            } else if r > 0 {
                1
            } else {
                0
            }
        }
    }
}

/// Shared driver for sort/rsort/asort/arsort/ksort/krsort.
fn php_builtin_sort(
    vm: &mut Vm,
    args: &[Value],
    fname: &str,
    by_key: bool,
    reverse: bool,
    renumber: bool,
) -> Result<Value, VmError> {
    let r = php_builtin_sort_inner(vm, args, fname, by_key, reverse, renumber);
    // Sorting arrays nested into themselves does not raise the comparison
    // recursion error (bug63882_2).
    let _ = goro_core::value::compare_recursion_take();
    r
}

fn php_builtin_sort_inner(
    vm: &mut Vm,
    args: &[Value],
    fname: &str,
    by_key: bool,
    reverse: bool,
    renumber: bool,
) -> Result<Value, VmError> {
    let first = args.first().unwrap_or(&Value::Null);
    require_array_arg(vm, first, fname, "array", 1)?;
    if let Some(Value::Array(arr)) = args.first() {
        let next_free = arr.borrow().next_free_key();
        let flags = args.get(1).map(|v| v.to_long()).unwrap_or(0);
        let entries: Vec<(ArrayKey, Value)> = arr
            .borrow()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        if entries.len() <= 1 && !(renumber && !entries.is_empty()) {
            return Ok(Value::True);
        }
        // A comparison (__toString()) writing to the array separates it
        // from the one being sorted in php-src: the sorted copy is dropped.
        let before = arr.borrow().identity();
        let sorted = crate::zsort::zend_sort_stable(entries, |x, y| {
            let r = if by_key {
                php_key_cmp(vm, &x.0, &y.0, flags)
            } else {
                php_data_cmp(vm, &x.1, &y.1, flags)
            };
            if reverse { -r } else { r }
        });
        if arr.borrow().identity() != before {
            return Ok(Value::True);
        }
        let mut new_arr = PhpArray::new();
        for (k, v) in sorted {
            if renumber {
                new_arr.push(v);
            } else {
                new_arr.set(k, v);
            }
        }
        if !renumber {
            new_arr.set_next_free_key(next_free);
        }
        *arr.borrow_mut() = new_arr;
    }
    Ok(Value::True)
}

fn sort_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    php_builtin_sort(vm, args, "sort", false, false, true)
}
fn rsort_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    php_builtin_sort(vm, args, "rsort", false, true, true)
}
fn asort_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    php_builtin_sort(vm, args, "asort", false, false, false)
}
fn arsort_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    php_builtin_sort(vm, args, "arsort", false, true, false)
}
fn ksort_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    php_builtin_sort(vm, args, "ksort", true, false, false)
}
fn krsort_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    php_builtin_sort(vm, args, "krsort", true, true, false)
}
fn shuffle_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let arr = match args.first() {
        Some(Value::Array(arr)) => arr.clone(),
        Some(Value::Reference(r)) => match &*r.borrow() {
            Value::Array(arr) => arr.clone(),
            _ => return Ok(Value::True),
        },
        _ => return Ok(Value::True),
    };
    // php_array_data_shuffle(): the values are shuffled in place and the
    // array becomes a list.
    let mut values: Vec<Value> = arr.borrow().values().cloned().collect();
    let engine = goro_core::random::default_engine();
    goro_core::random::shuffle_items(vm, &engine, &mut values);
    let mut new_arr = PhpArray::with_capacity(values.len());
    for v in values {
        new_arr.push(v);
    }
    *arr.borrow_mut() = new_arr;
    Ok(Value::True)
}

/// PHP_FUNCTION(range) (ext/standard/array.c)
fn range_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    const HT_MAX_SIZE: u64 = 0x40000000;
    const T_LONG: u8 = 4;
    const T_DOUBLE: u8 = 5;
    const T_STRING: u8 = 6;
    const T_ARRAY: u8 = 7; // single-byte numeric string
    fn verr(vm: &mut Vm, msg: String) -> Result<Value, VmError> {
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        Err(VmError {
            message: msg,
            line: vm.current_line,
        })
    }
    fn terr(vm: &mut Vm, msg: String) -> Result<Value, VmError> {
        let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        Err(VmError {
            message: msg,
            line: vm.current_line,
        })
    }
    let strict = vm.caller_strict_types;
    // Z_PARAM_NUMBER_OR_STR
    let num_or_str = |vm: &mut Vm, idx: usize, name: &str| -> Result<Value, VmError> {
        let v = args.get(idx).cloned().unwrap_or(Value::Null).deref();
        match &v {
            Value::Long(_) | Value::Double(_) | Value::String(_) => Ok(v),
            Value::Null | Value::Undef | Value::False | Value::True if !strict => {
                if matches!(v, Value::Null | Value::Undef) {
                    vm.emit_deprecated(&format!("range(): Passing null to parameter #{} (${}) of type string|int|float is deprecated", idx + 1, name));
                }
                Ok(Value::Long(matches!(v, Value::True) as i64))
            }
            Value::Object(o)
                if !strict
                    && vm
                        .get_class_def(&o.borrow().class_name.to_ascii_lowercase())
                        .map(|c| c.get_method(b"__tostring").is_some())
                        .unwrap_or(false) =>
            {
                Ok(Value::String(vm.value_to_string(&v)))
            }
            _ => {
                let tn = match &v {
                    Value::True => "true".to_string(),
                    Value::False => "false".to_string(),
                    _ => Vm::value_type_name(&v).to_string(),
                };
                terr(
                    vm,
                    format!(
                        "range(): Argument #{} (${}) must be of type string|int|float, {} given",
                        idx + 1,
                        name,
                        tn
                    ),
                )
                .map(|_| Value::Null)
            }
        }
    };
    let user_start = num_or_str(vm, 0, "start")?;
    let user_end = num_or_str(vm, 1, "end")?;
    let mut is_step_double = false;
    let mut is_step_negative = false;
    let mut step_double = 1.0f64;
    let mut step: i64 = 1;
    if let Some(sv) = args.get(2) {
        // Z_PARAM_NUMBER
        let v = sv.deref();
        let v = match &v {
            Value::Long(_) | Value::Double(_) => v.clone(),
            Value::String(s) if !strict => {
                match goro_core::value::parse_numeric_str(s.as_bytes(), false) {
                    Some((goro_core::value::NumStr::Long(l), _)) => Value::Long(l),
                    Some((goro_core::value::NumStr::Double(d), _)) => Value::Double(d),
                    None => {
                        return terr(
                            vm,
                            "range(): Argument #3 ($step) must be of type int|float, string given"
                                .to_string(),
                        );
                    }
                }
            }
            Value::Null | Value::Undef | Value::False | Value::True if !strict => {
                if matches!(v, Value::Null | Value::Undef) {
                    vm.emit_deprecated("range(): Passing null to parameter #3 ($step) of type int|float is deprecated");
                }
                Value::Long(matches!(v, Value::True) as i64)
            }
            _ => {
                let tn = match &v {
                    Value::True => "true".to_string(),
                    Value::False => "false".to_string(),
                    _ => Vm::value_type_name(&v).to_string(),
                };
                return terr(
                    vm,
                    format!(
                        "range(): Argument #3 ($step) must be of type int|float, {} given",
                        tn
                    ),
                );
            }
        };
        match v {
            Value::Double(d) => {
                step_double = d;
                if d.is_infinite() {
                    return verr(
                        vm,
                        "range(): Argument #3 ($step) must be a finite number, INF provided".into(),
                    );
                }
                if d.is_nan() {
                    return verr(
                        vm,
                        "range(): Argument #3 ($step) must be a finite number, NAN provided".into(),
                    );
                }
                if step_double < 0.0 {
                    is_step_negative = true;
                    step_double = -step_double;
                }
                step = if (-9223372036854775808.0..9223372036854775808.0).contains(&step_double) {
                    step_double as i64
                } else {
                    0
                };
                if step as f64 != step_double {
                    is_step_double = true;
                }
            }
            Value::Long(l) => {
                step = l;
                if step < 0 {
                    if step == i64::MIN {
                        return verr(
                            vm,
                            format!("range(): Argument #3 ($step) must be greater than {}", step),
                        );
                    }
                    is_step_negative = true;
                    step = -step;
                }
                step_double = step as f64;
            }
            _ => {}
        }
        if step_double == 0.0 {
            return verr(vm, "range(): Argument #3 ($step) cannot be 0".into());
        }
    }
    // php_range_process_input
    let process = |vm: &mut Vm,
                   input: &Value,
                   arg_num: usize,
                   name: &str|
     -> Result<(u8, i64, f64), VmError> {
        match input {
            Value::Long(l) => Ok((T_LONG, *l, *l as f64)),
            Value::Double(d) => {
                if d.is_infinite() {
                    return verr(
                        vm,
                        format!(
                            "range(): Argument #{} (${}) must be a finite number, INF provided",
                            arg_num, name
                        ),
                    )
                    .map(|_| (0, 0, 0.0));
                }
                if d.is_nan() {
                    return verr(
                        vm,
                        format!(
                            "range(): Argument #{} (${}) must be a finite number, NAN provided",
                            arg_num, name
                        ),
                    )
                    .map(|_| (0, 0, 0.0));
                }
                Ok((T_DOUBLE, 0, *d))
            }
            Value::String(s) => {
                let b = s.as_bytes();
                if b.is_empty() {
                    vm.emit_warning(&format!(
                        "range(): Argument #{} (${}) must not be empty, casted to 0",
                        arg_num, name
                    ));
                    return Ok((T_LONG, 0, 0.0));
                }
                match goro_core::value::parse_numeric_str(b, false) {
                    Some((goro_core::value::NumStr::Double(d), _)) => {
                        if d.is_infinite() {
                            return verr(vm, format!("range(): Argument #{} (${}) must be a finite number, INF provided", arg_num, name)).map(|_| (0, 0, 0.0));
                        }
                        if d.is_nan() {
                            return verr(vm, format!("range(): Argument #{} (${}) must be a finite number, NAN provided", arg_num, name)).map(|_| (0, 0, 0.0));
                        }
                        Ok((T_DOUBLE, 0, d))
                    }
                    Some((goro_core::value::NumStr::Long(l), _)) => {
                        Ok((if b.len() == 1 { T_ARRAY } else { T_LONG }, l, l as f64))
                    }
                    None => {
                        if b.len() != 1 {
                            vm.emit_warning(&format!("range(): Argument #{} (${}) must be a single byte, subsequent bytes are ignored", arg_num, name));
                        }
                        Ok((T_STRING, 0, 0.0))
                    }
                }
            }
            _ => Ok((T_LONG, 0, 0.0)),
        }
    };
    let (mut start_type, start_long, start_double) = process(vm, &user_start, 1, "start")?;
    let (mut end_type, end_long, end_double) = process(vm, &user_end, 2, "end")?;
    let boundary = "range(): Argument #3 ($step) must be less than the range spanned by argument #1 ($start) and argument #2 ($end)";
    let negative = "range(): Argument #3 ($step) must be greater than 0 for increasing ranges";
    let mut result = PhpArray::new();
    let mut chars = false;
    if start_type >= T_STRING || end_type >= T_STRING {
        if start_type < T_STRING || end_type < T_STRING {
            if start_type < T_STRING {
                if end_type != T_ARRAY {
                    vm.emit_warning("range(): Argument #1 ($start) must be a single byte string if argument #2 ($end) is a single byte string, argument #2 ($end) converted to 0");
                }
                end_type = T_LONG;
            } else if end_type < T_STRING {
                if start_type != T_ARRAY {
                    vm.emit_warning("range(): Argument #2 ($end) must be a single byte string if argument #1 ($start) is a single byte string, argument #1 ($start) converted to 0");
                }
                start_type = T_LONG;
            }
        } else if is_step_double {
            if start_type == T_STRING || end_type == T_STRING {
                vm.emit_warning("range(): Argument #3 ($step) must be of type int when generating an array of characters, inputs converted to 0");
            }
            end_type = T_LONG;
            start_type = T_LONG;
        } else {
            chars = true;
        }
    }
    // Inputs "converted to 0" keep their zeroed numeric values.
    let (start_long, start_double) = if start_type == T_LONG
        && matches!(user_start, Value::String(ref s) if goro_core::value::parse_numeric_str(s.as_bytes(), false).is_none())
    {
        (0, 0.0)
    } else {
        (start_long, start_double)
    };
    let (end_long, end_double) = if end_type == T_LONG
        && matches!(user_end, Value::String(ref s) if goro_core::value::parse_numeric_str(s.as_bytes(), false).is_none())
    {
        (0, 0.0)
    } else {
        (end_long, end_double)
    };
    if chars {
        let mut low = match &user_start {
            Value::String(s) => s.as_bytes()[0] as i64,
            _ => 0,
        };
        let high = match &user_end {
            Value::String(s) => s.as_bytes()[0] as i64,
            _ => 0,
        };
        if low > high {
            if low - high < step {
                return verr(vm, boundary.into());
            }
            while low >= high {
                result.push(Value::String(PhpString::from_vec(vec![low as u8])));
                if low - step < 0 {
                    break;
                }
                low -= step;
            }
        } else if high > low {
            if is_step_negative {
                return verr(vm, negative.into());
            }
            if high - low < step {
                return verr(vm, boundary.into());
            }
            while low <= high {
                result.push(Value::String(PhpString::from_vec(vec![low as u8])));
                if low + step > 255 {
                    break;
                }
                low += step;
            }
        } else {
            result.push(Value::String(PhpString::from_vec(vec![low as u8])));
        }
        return Ok(Value::Array(Rc::new(RefCell::new(result))));
    }
    let fmt1 = |f: f64| format!("{:.1}", f);
    if start_type == T_DOUBLE || end_type == T_DOUBLE || is_step_double {
        let (hi, lo, dec) = if start_double > end_double {
            (start_double, end_double, true)
        } else {
            (end_double, start_double, false)
        };
        if start_double == end_double {
            result.push(Value::Double(start_double));
        } else {
            if !dec && is_step_negative {
                return verr(vm, negative.into());
            }
            if hi - lo < step_double {
                return verr(vm, boundary.into());
            }
            let calc = ((hi - lo) / step_double) + 1.0;
            if calc >= HT_MAX_SIZE as f64 {
                return verr(
                    vm,
                    format!(
                        "The supplied range exceeds the maximum array size by {} elements: start={}, end={}, step={}. Max size: {:.0}",
                        fmt1(calc - HT_MAX_SIZE as f64),
                        fmt1(lo),
                        fmt1(hi),
                        fmt1(step_double),
                        HT_MAX_SIZE as f64
                    ),
                );
            }
            let size = crate::math::php_math_round(calc, 0, 1) as u64;
            let mut i = 0u64;
            loop {
                // (the first element is start itself: -0.0 stays -0.0)
                let element = if i == 0 {
                    start_double
                } else if dec {
                    start_double - (i as f64 * step_double)
                } else {
                    start_double + (i as f64 * step_double)
                };
                if i >= size || (dec && element < end_double) || (!dec && element > end_double) {
                    break;
                }
                result.push(Value::Double(element));
                i += 1;
            }
        }
    } else {
        let ustep = step as u64;
        if start_long == end_long {
            result.push(Value::Long(start_long));
        } else {
            let dec = start_long > end_long;
            if !dec && is_step_negative {
                return verr(vm, negative.into());
            }
            let span = if dec {
                (start_long as u64).wrapping_sub(end_long as u64)
            } else {
                (end_long as u64).wrapping_sub(start_long as u64)
            };
            if span < ustep {
                return verr(vm, boundary.into());
            }
            let calc = span / ustep;
            if calc >= HT_MAX_SIZE - 1 {
                let (a, b) = if dec {
                    (start_long, end_long)
                } else {
                    (end_long, start_long)
                };
                return verr(
                    vm,
                    format!(
                        "The supplied range exceeds the maximum array size by {} elements: start={}, end={}, step={}. Calculated size: {}. Maximum size: {}.",
                        calc - (HT_MAX_SIZE - 1),
                        b,
                        a,
                        ustep,
                        calc,
                        HT_MAX_SIZE
                    ),
                );
            }
            for i in 0..=calc {
                let off = i.wrapping_mul(ustep);
                let v = if dec {
                    (start_long as u64).wrapping_sub(off) as i64
                } else {
                    (start_long as u64).wrapping_add(off) as i64
                };
                result.push(Value::Long(v));
            }
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn compact(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let msg = "Cannot call compact() dynamically";
    let exc = vm.create_exception(b"Error", msg, vm.current_line);
    vm.current_exception = Some(exc);
    Err(VmError {
        message: format!("Uncaught Error: {}", msg),
        line: vm.current_line,
    })
}

thread_local! {
    /// Internal pointers of objects walked with current()/next()/...
    /// (their property table's nInternalPointer), by object id.
    static OBJ_POINTERS: RefCell<std::collections::HashMap<u64, usize>> =
        RefCell::new(std::collections::HashMap::new());
}

/// The property table slots of an object: (name, value), None for an
/// uninitialized (UNDEF) typed property.
fn obj_slots(obj: &Rc<RefCell<PhpObject>>) -> Vec<(Vec<u8>, Option<Value>)> {
    obj.borrow()
        .properties
        .iter()
        .filter(|(k, _)| {
            !(k.starts_with(b"__spl_")
                || k.starts_with(b"__reflection_")
                || k.starts_with(b"__enum_")
                || k.starts_with(b"__ctor_")
                || k.starts_with(b"__clone_")
                || k.starts_with(b"__destructed")
                || k.starts_with(b"__fiber_")
                || k.starts_with(b"__lazy_"))
        })
        .map(|(k, v)| {
            (
                k.clone(),
                if matches!(v, Value::Undef) {
                    None
                } else {
                    Some(v.deref())
                },
            )
        })
        .collect()
}

/// The array-pointer functions on an object (deprecated): `op` is the
/// function name; the property table is walked skipping UNDEF slots.
fn obj_pointer_op(vm: &mut Vm, obj: &Rc<RefCell<PhpObject>>, op: &str) -> Value {
    vm.emit_deprecated(&format!(
        "{}(): Calling {}() on an object is deprecated",
        op, op
    ));
    let slots = obj_slots(obj);
    let id = obj.borrow().object_id;
    let mut ptr = OBJ_POINTERS.with(|p| p.borrow().get(&id).copied().unwrap_or(0));
    let valid_from = |p: usize| (p..slots.len()).find(|&i| slots[i].1.is_some());
    let valid_before = |p: usize| {
        (0..p.min(slots.len()))
            .rev()
            .find(|&i| slots[i].1.is_some())
    };
    // _zend_hash_get_valid_pos(): the pointer settles on a valid slot (or
    // past the end).
    let cur = match op {
        "reset" => valid_from(0),
        "end" => valid_before(slots.len()),
        "next" => valid_from(ptr).and_then(|c| valid_from(c + 1)),
        "prev" => valid_from(ptr).and_then(valid_before),
        _ => valid_from(ptr),
    };
    ptr = cur.unwrap_or(slots.len());
    OBJ_POINTERS.with(|p| p.borrow_mut().insert(id, ptr));
    match (op, cur) {
        ("key", Some(i)) => Value::String(PhpString::from_vec(slots[i].0.clone())),
        ("key", None) => Value::Null,
        (_, Some(i)) => slots[i].1.clone().unwrap_or(Value::False),
        (_, None) => Value::False,
    }
}

fn current(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Object(o)) = args.first().map(|v| v.deref()) {
        return Ok(obj_pointer_op(vm, &o, "current"));
    }
    if let Some(Value::Array(arr)) = args.first() {
        let arr = arr.borrow();
        let pos = arr.pointer;
        Ok(arr
            .entry_at(pos)
            .map(|(_, v)| v.deref())
            .unwrap_or(Value::False))
    } else {
        Ok(Value::False)
    }
}

fn next_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Object(o)) = args.first().map(|v| v.deref()) {
        return Ok(obj_pointer_op(vm, &o, "next"));
    }
    if let Some(Value::Array(arr)) = args.first() {
        let mut arr = arr.borrow_mut();
        // The pointer stops past the end (the next append's slot).
        arr.pointer = (arr.pointer + 1).min(arr.len());
        let pos = arr.pointer;
        Ok(arr
            .entry_at(pos)
            .map(|(_, v)| v.deref())
            .unwrap_or(Value::False))
    } else {
        Ok(Value::False)
    }
}

fn prev_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Object(o)) = args.first().map(|v| v.deref()) {
        return Ok(obj_pointer_op(vm, &o, "prev"));
    }
    if let Some(Value::Array(arr)) = args.first() {
        let mut arr = arr.borrow_mut();
        let len = arr.len();
        if arr.pointer == 0 || arr.pointer >= len {
            // Already at (or before) beginning or past end: return false
            // Move pointer past end so current() also returns false
            arr.pointer = len;
            Ok(Value::False)
        } else {
            arr.pointer -= 1;
            let pos = arr.pointer;
            Ok(arr
                .entry_at(pos)
                .map(|(_, v)| v.deref())
                .unwrap_or(Value::False))
        }
    } else {
        Ok(Value::False)
    }
}

fn reset_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Object(o)) = args.first().map(|v| v.deref()) {
        return Ok(obj_pointer_op(vm, &o, "reset"));
    }
    if let Some(Value::Array(arr)) = args.first() {
        let mut arr = arr.borrow_mut();
        arr.pointer = 0;
        Ok(arr
            .iter()
            .next()
            .map(|(_, v)| v.deref())
            .unwrap_or(Value::False))
    } else {
        Ok(Value::False)
    }
}

fn end_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Object(o)) = args.first().map(|v| v.deref()) {
        return Ok(obj_pointer_op(vm, &o, "end"));
    }
    if let Some(Value::Array(arr)) = args.first() {
        let mut arr = arr.borrow_mut();
        let len = arr.len();
        arr.pointer = if len > 0 { len - 1 } else { 0 };
        Ok(arr
            .iter()
            .last()
            .map(|(_, v)| v.deref())
            .unwrap_or(Value::False))
    } else {
        Ok(Value::False)
    }
}

fn key_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Object(o)) = args.first().map(|v| v.deref()) {
        return Ok(obj_pointer_op(vm, &o, "key"));
    }
    if let Some(Value::Array(arr)) = args.first() {
        let arr = arr.borrow();
        let pos = arr.pointer;
        Ok(arr
            .entry_at(pos)
            .map(|(k, _)| match k {
                goro_core::array::ArrayKey::Int(n) => Value::Long(*n),
                goro_core::array::ArrayKey::String(s) => Value::String(s.clone()),
            })
            .unwrap_or(Value::Null))
    } else {
        Ok(Value::Null)
    }
}

// === Misc ===

fn ini_set(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.len() != 2 {
        let msg = format!(
            "ini_set() expects exactly 2 arguments, {} given",
            args.len()
        );
        let exc = vm.create_exception(b"ArgumentCountError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    let name = args.first().unwrap_or(&Value::Null).to_php_string();
    let value = args.get(1).map(|v| v.deref()).unwrap_or(Value::Null);
    if matches!(
        value,
        Value::Array(_) | Value::Object(_) | Value::Resource(_)
    ) {
        return Err(throw_type_error_msg(
            vm,
            "ini_set(): Argument #2 ($value) must be of type string|int|float|bool|null"
                .to_string(),
        ));
    }
    let key = name.as_bytes().to_vec();
    // ini_set() returns the old value as a string.
    let old = vm
        .constants
        .get(&key)
        .map(|v| Value::String(v.to_php_string()));
    // zend_alter_ini_entry_ex(): an entry no extension registered (or one
    // that was removed, like session.hash_function) cannot be set.
    if old.is_none() {
        return Ok(Value::False);
    }
    // OnUpdateDefaultCharset / OnUpdateDefaultMimeTye: no header injection
    if key == b"default_charset" || key == b"default_mimetype" {
        let v = value.to_php_string();
        if v.as_bytes().iter().any(|&c| matches!(c, 0 | b'\r' | b'\n')) {
            return Ok(Value::False);
        }
    }
    // OnUpdateStrNotEmpty / OnUpdateStringUnempty refuse an empty value.
    if matches!(
        key.as_slice(),
        b"arg_separator.input" | b"arg_separator.output" | b"include_path"
    ) && value.to_php_string().as_bytes().is_empty()
    {
        return Ok(Value::False);
    }
    ini_remember(vm, &key);
    // Extension-registered INI validators (e.g. date.timezone)
    if let Some(validate) = vm.ini_validators.get(&key).copied() {
        let new_bytes = value.to_php_string().as_bytes().to_vec();
        if !validate(vm, &new_bytes, false) {
            return Ok(Value::False);
        }
    }
    // Extension on-modify handler (may reject the value)
    if let Some(hook) = vm.ini_set_hooks.get(&key).copied() {
        if !hook(vm, &key, &value) {
            return Ok(Value::False);
        }
    }
    // OnUpdateTimeout: changing max_execution_time restarts the timer.
    if key == b"max_execution_time" {
        set_execution_timeout(value.to_long());
    }
    // Handle precision specially to update thread-local
    if key == b"precision" {
        if let Value::Long(p) = &value {
            goro_core::value::set_php_precision(*p as i32);
        } else if let Value::String(s) = &value {
            if let Ok(p) = s.to_string_lossy().parse::<i32>() {
                goro_core::value::set_php_precision(p);
            }
        }
    }
    // OnUpdateFiberStackSize
    if key == b"fiber.stack_size" {
        let vs = value.to_php_string();
        let (v, err) = goro_core::ini_quantity::parse_quantity(vs.as_bytes(), true);
        if let Some(e) = err {
            vm.emit_warning(&e);
        }
        if (v as i64) < 0 {
            vm.emit_warning("fiber.stack_size must be a positive number");
            return Ok(Value::False);
        }
    }
    // max_memory_limit, the stack limits and the mail.* entries are
    // PHP_INI_SYSTEM (|PHP_INI_PERDIR)
    if key == b"max_memory_limit"
        || key == b"zend.max_allowed_stack_size"
        || key == b"zend.reserved_stack_size"
        || key == b"mail.cr_lf_mode"
        || key == b"mail.add_x_header"
        || key == b"mail.mixed_lf_and_crlf"
        || key == b"mail.log"
        || key == b"mail.force_extra_parameters"
        || goro_core::vm::CORE_INI_SYSTEM.contains(&key.as_slice())
    {
        return Ok(Value::False);
    }
    // Handle memory_limit (OnChangeMemoryLimit: clamp to max_memory_limit)
    if key == b"memory_limit" {
        let vs = value.to_php_string();
        let (v, err) = goro_core::ini_quantity::parse_quantity(vs.as_bytes(), false);
        if let Some(e) = err {
            vm.emit_warning(&e);
        }
        let max_s = vm
            .constants
            .get(b"max_memory_limit".as_ref())
            .map(|m| m.to_php_string());
        if let Some(max_s) = max_s {
            let (max, _) = goro_core::ini_quantity::parse_quantity(max_s.as_bytes(), false);
            if v > max {
                if v != u64::MAX {
                    vm.emit_warning(&format!(
                        "Failed to set memory_limit to {} bytes. Setting to max_memory_limit instead (currently: {} bytes)",
                        v as i64, max as i64
                    ));
                }
                goro_core::value::set_memory_limit(max as i64);
                vm.constants.insert(key, Value::String(max_s));
                return Ok(old.unwrap_or(Value::False));
            }
        }
        if v != u64::MAX {
            // zend_set_memory_limit(): the limit cannot go below the memory
            // already reserved.
            let real =
                goro_core::value::memory_real_size(goro_core::value::memory_get_usage()) as u64;
            if v < real {
                vm.emit_warning(&format!(
                    "Failed to set memory limit to {} bytes (Current memory usage is {} bytes)",
                    v, real
                ));
                return Ok(Value::False);
            }
        }
        goro_core::value::set_memory_limit(if v == u64::MAX { -1 } else { v as i64 });
    }
    // Handle error_reporting
    if key == b"error_reporting" {
        let level = match &value {
            Value::Long(n) => *n,
            Value::String(s) => {
                let s_str = s.to_string_lossy();
                if let Ok(n) = s_str.parse::<i64>() {
                    n
                } else {
                    vm.error_reporting
                }
            }
            _ => value.to_long(),
        };
        vm.error_reporting = level;
    }
    // Handle serialize_precision
    if key == b"serialize_precision" {
        if let Value::Long(p) = &value {
            goro_core::value::set_php_serialize_precision(*p as i32);
        } else if let Value::String(s) = &value {
            if let Ok(p) = s.to_string_lossy().parse::<i32>() {
                goro_core::value::set_php_serialize_precision(p);
            }
        }
    }
    // Handle zend.assertions: warn when trying to change it if currently -1 (only allowed in php.ini)
    // Also warn when trying to set it to -1 at runtime
    if key == b"zend.assertions" {
        let new_val = match &value {
            Value::Long(n) => *n,
            Value::String(s) => s.to_string_lossy().parse::<i64>().unwrap_or(0),
            _ => value.to_long(),
        };
        let cur_val = vm
            .constants
            .get(b"zend.assertions".as_ref())
            .map(|v| v.to_long())
            .unwrap_or(1);
        if new_val == -1 || cur_val == -1 {
            vm.emit_warning(
                "zend.assertions may be completely enabled or disabled only in php.ini",
            );
            // When zend.assertions is -1 (set in php.ini), don't allow runtime changes.
            // Keep the value as -1 so subsequent ini_set calls also warn.
            if cur_val == -1 {
                return Ok(old.unwrap_or(Value::False));
            }
        }
    }
    if let Some(msg) = ini_handler_deprecation(&key, &value) {
        vm.emit_deprecated(&msg);
    }
    if key == b"report_memleaks" {
        if let Some(msg) = assert_ini_deprecation(&key, &value) {
            vm.emit_deprecated(&format!("ini_set(): {}", msg));
        }
    }
    if key.starts_with(b"assert.") {
        if key == b"assert.callback" {
            ASSERT_CALLBACK.with(|c| *c.borrow_mut() = None);
        }
        if let Some(msg) = assert_ini_deprecation(&key, &value) {
            vm.emit_deprecated(&format!("ini_set(): {}", msg));
        }
        // ini values are stored as strings
        let sv = match &value {
            Value::True => Value::String(PhpString::from_bytes(b"1")),
            Value::False | Value::Null => Value::String(PhpString::empty()),
            Value::Long(_) | Value::Double(_) => Value::String(value.to_php_string()),
            v => v.clone(),
        };
        vm.constants.insert(key, sv);
        return Ok(old
            .map(|v| Value::String(v.to_php_string()))
            .unwrap_or(Value::False));
    }
    // Actually update the value
    vm.constants.insert(key, value);
    // Return old value or false if not previously set
    Ok(old.unwrap_or(Value::False))
}
/// ini_get_all(): the INI entries of an extension (or of all the loaded
/// ones), sorted by name, with their global and local values and access
/// level, or just the values.
fn ini_get_all(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let details = args.get(1).is_none_or(|v| v.is_truthy());
    let mut names: Vec<&'static str> = Vec::new();
    match args.first() {
        Some(v) if !matches!(v, Value::Null) => {
            let ext = v.to_php_string().to_string_lossy();
            let lower = ext.to_ascii_lowercase();
            let loaded =
                extension_loaded_fn(vm, &[Value::String(PhpString::from_string(lower.clone()))])?;
            if !loaded.is_truthy() {
                vm.emit_warning(&format!(
                    "ini_get_all(): Extension \"{}\" cannot be found",
                    ext
                ));
                return Ok(Value::False);
            }
            names.extend_from_slice(goro_core::reflection::extension_ini_entries(&lower));
        }
        _ => {
            let mut mods: Vec<String> = MODULE_ORDER
                .iter()
                .map(|m| m.to_ascii_lowercase())
                .collect();
            mods.push("standard".into());
            for m in mods {
                let loaded =
                    extension_loaded_fn(vm, &[Value::String(PhpString::from_string(m.clone()))])?;
                if loaded.is_truthy() {
                    for n in goro_core::reflection::extension_ini_entries(&m) {
                        if !names.contains(n) {
                            names.push(n);
                        }
                    }
                }
            }
        }
    }
    names.sort_unstable();
    let mut out = PhpArray::new();
    for name in names {
        let local = vm
            .constants
            .get(name.as_bytes())
            .map(|v| Value::String(v.to_php_string()))
            .unwrap_or(Value::Null);
        let key = ArrayKey::String(PhpString::from_bytes(name.as_bytes()));
        if !details {
            out.set(key, local);
            continue;
        }
        let global = match vm.ini_originals.get(name.as_bytes()) {
            Some(Some(v)) => Value::String(v.to_php_string()),
            Some(None) => Value::Null,
            None => local.clone(),
        };
        let mut e = PhpArray::new();
        e.set(
            ArrayKey::String(PhpString::from_bytes(b"global_value")),
            global,
        );
        e.set(
            ArrayKey::String(PhpString::from_bytes(b"local_value")),
            local,
        );
        e.set(
            ArrayKey::String(PhpString::from_bytes(b"access")),
            Value::Long(goro_core::reflection::ini_access(name)),
        );
        out.set(key, Value::Array(Rc::new(RefCell::new(e))));
    }
    Ok(Value::Array(Rc::new(RefCell::new(out))))
}

fn ini_get(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let name = args.first().unwrap_or(&Value::Null).to_php_string();
    match vm.constants.get(name.as_bytes()) {
        Some(val) => {
            // ini_get always returns string values
            Ok(Value::String(val.to_php_string()))
        }
        None => Ok(Value::False),
    }
}
/// Record the startup value of an INI entry before it is first modified.
fn ini_remember(vm: &mut Vm, key: &[u8]) {
    if !vm.ini_originals.contains_key(key) {
        let cur = vm.constants.get(key).cloned();
        vm.ini_originals.insert(key.to_vec(), cur);
    }
}

fn ini_restore(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let name = args.first().unwrap_or(&Value::Null).to_php_string();
    let key = name.as_bytes().to_vec();
    match vm.ini_originals.remove(&key) {
        Some(Some(orig)) => {
            let _ = ini_set(vm, &[Value::String(name), orig]);
            vm.ini_originals.remove(&key);
        }
        Some(None) => {
            vm.constants.shift_remove(&key);
        }
        None => {}
    }
    Ok(Value::Null)
}
fn set_time_limit(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let secs = args.first().map(|v| v.to_long()).unwrap_or(0);
    vm.constants.insert(
        b"max_execution_time".to_vec(),
        Value::String(PhpString::from_string(secs.to_string())),
    );
    set_execution_timeout(secs);
    Ok(Value::True)
}

/// SIGPROF handler (zend_timeout_handler): flag the timeout for the VM, or
/// kill the process when the hard timeout after an earlier one expires.
#[cfg(unix)]
extern "C" fn execution_timeout_handler(_sig: libc::c_int) {
    use goro_core::value::{HARD_TIMEOUT, TIMED_OUT, TIMEOUT_SECONDS};
    use std::sync::atomic::Ordering;
    if TIMED_OUT.load(Ordering::Relaxed) {
        // Async-signal-safe formatting into a stack buffer.
        let mut buf = [0u8; 160];
        let mut n = 0;
        let mut put = |bytes: &[u8]| {
            for &b in bytes {
                if n < buf.len() {
                    buf[n] = b;
                    n += 1;
                }
            }
        };
        let num = |v: i64, out: &mut [u8; 20]| -> usize {
            let mut v = v.unsigned_abs();
            let mut i = out.len();
            loop {
                i -= 1;
                out[i] = b'0' + (v % 10) as u8;
                v /= 10;
                if v == 0 {
                    break;
                }
            }
            i
        };
        let mut d = [0u8; 20];
        put(b"\nFatal error: Maximum execution time of ");
        let i = num(TIMEOUT_SECONDS.load(Ordering::Relaxed), &mut d);
        put(&d[i..]);
        put(b"+");
        let i = num(HARD_TIMEOUT.load(Ordering::Relaxed), &mut d);
        put(&d[i..]);
        put(b" seconds exceeded (terminated) in Unknown on line 0\n");
        unsafe {
            libc::write(2, buf.as_ptr() as *const libc::c_void, n);
            libc::_exit(124);
        }
    }
    TIMED_OUT.store(true, Ordering::Relaxed);
    goro_core::value::VM_INTERRUPT.store(true, Ordering::Relaxed);
    let hard = HARD_TIMEOUT.load(Ordering::Relaxed);
    if hard > 0 {
        arm_prof_timer(hard);
    }
}

#[cfg(unix)]
fn arm_prof_timer(secs: i64) {
    let t = libc::itimerval {
        it_interval: libc::timeval {
            tv_sec: 0,
            tv_usec: 0,
        },
        it_value: libc::timeval {
            tv_sec: secs as libc::time_t,
            tv_usec: 0,
        },
    };
    unsafe {
        libc::setitimer(libc::ITIMER_PROF, &t, std::ptr::null_mut());
    }
}

/// Set by SAPIs running requests on several threads: the max_execution_time
/// timer (ITIMER_PROF, SIGPROF) is per process, so one request's
/// set_time_limit() would end another's (and the hard timeout would end
/// the server). The directive and set_time_limit() are then accepted but
/// not enforced.
static NO_EXECUTION_TIMEOUTS: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Disable the max_execution_time timer for the process (see above).
pub fn disable_execution_timeouts() {
    NO_EXECUTION_TIMEOUTS.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// zend_set_timeout(): (re)arm the max_execution_time CPU timer; 0 disarms.
pub fn set_execution_timeout(secs: i64) {
    use goro_core::value::{TIMED_OUT, TIMEOUT_SECONDS};
    use std::sync::atomic::Ordering;
    if NO_EXECUTION_TIMEOUTS.load(Ordering::Relaxed) {
        return;
    }
    let secs = if (0..=999_999_999).contains(&secs) {
        secs
    } else {
        0
    };
    TIMEOUT_SECONDS.store(secs, Ordering::Relaxed);
    TIMED_OUT.store(false, Ordering::Relaxed);
    #[cfg(unix)]
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = execution_timeout_handler as *const () as usize;
        sa.sa_flags = libc::SA_RESTART;
        libc::sigemptyset(&mut sa.sa_mask);
        libc::sigaction(libc::SIGPROF, &sa, std::ptr::null_mut());
        arm_prof_timer(secs);
    }
}
thread_local! {
    /// ASSERTG(callback): set by assert_options(ASSERT_CALLBACK) / ini_set().
    static ASSERT_CALLBACK: RefCell<Option<Value>> = const { RefCell::new(None) };
}

/// zend_ini_parse_bool()
fn ini_parse_bool(v: &Value) -> bool {
    match v {
        Value::True => true,
        Value::False | Value::Null | Value::Undef => false,
        Value::Long(n) => *n != 0,
        other => {
            let s = other.to_php_string().to_string_lossy();
            let t = s.trim();
            if t.eq_ignore_ascii_case("true")
                || t.eq_ignore_ascii_case("yes")
                || t.eq_ignore_ascii_case("on")
            {
                true
            } else {
                let digits: String = t
                    .chars()
                    .take_while(|c| c.is_ascii_digit() || *c == '-' || *c == '+')
                    .collect();
                digits.parse::<i64>().map(|n| n != 0).unwrap_or(false)
            }
        }
    }
}

fn assert_ini_bool(vm: &Vm, key: &[u8], default: bool) -> bool {
    vm.constants.get(key).map(ini_parse_bool).unwrap_or(default)
}

/// E_DEPRECATED raised (with zend_error(), no function prefix) by an INI
/// handler for the new value.
pub fn ini_handler_deprecation(key: &[u8], value: &Value) -> Option<String> {
    match key {
        // OnUpdateAutoDetectLineEndings
        b"auto_detect_line_endings" if ini_parse_bool(value) => {
            Some("auto_detect_line_endings is deprecated".to_string())
        }
        _ => None,
    }
}

/// Deprecations emitted by the assert.* INI handlers when a value is changed
/// (not from assert_options()). Returns the message, if any.
pub fn assert_ini_deprecation(key: &[u8], value: &Value) -> Option<String> {
    let name = String::from_utf8_lossy(key).to_string();
    let deprecated = match key {
        b"assert.active" | b"assert.warning" | b"assert.exception" => !ini_parse_bool(value),
        b"assert.bail" => ini_parse_bool(value),
        b"assert.callback" => !value.to_php_string().as_bytes().is_empty(),
        // OnUpdateReportMemleaks
        b"report_memleaks" => {
            return (!ini_parse_bool(value))
                .then(|| "Directive 'report_memleaks' is deprecated".to_string());
        }
        _ => false,
    };
    if deprecated {
        Some(format!("{} INI setting is deprecated", name))
    } else {
        None
    }
}

fn php_assert(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let zend_assertions = vm
        .constants
        .get(b"zend.assertions".as_ref())
        .map(|v| v.to_long())
        .unwrap_or(1);
    if !assert_ini_bool(vm, b"assert.active", true) || zend_assertions <= 0 {
        return Ok(Value::True);
    }
    let assertion = args.first().map(|v| v.deref()).unwrap_or(Value::Null);
    let description = args.get(1).map(|v| v.deref());
    if let Some(d) = &description {
        match d {
            Value::Null | Value::String(_) | Value::Object(_) => {}
            other => {
                let msg = format!(
                    "assert(): Argument #2 ($description) must be of type Throwable|string|null, {} given",
                    Vm::value_type_name(other)
                );
                let exc = vm.throw_type_error(msg.clone());
                vm.current_exception = Some(exc);
                return Err(VmError {
                    message: format!("Uncaught TypeError: {}", msg),
                    line: vm.current_line,
                });
            }
        }
    }
    if assertion.is_truthy() {
        return Ok(Value::True);
    }
    if let Some(Value::Object(obj)) = &description {
        vm.current_exception = Some(Value::Object(obj.clone()));
        return Err(VmError {
            message: "assert() failed".into(),
            line: vm.current_line,
        });
    }
    let desc_str: Option<PhpString> = match &description {
        Some(Value::String(s)) => Some(s.clone()),
        _ => None,
    };
    let line = vm.current_line;
    // Callback: an explicitly set callback, else the assert.callback INI string.
    let mut callback = ASSERT_CALLBACK.with(|c| c.borrow().clone());
    if callback.is_none() {
        if let Some(cb) = vm.constants.get(b"assert.callback".as_ref()) {
            let cbs = cb.to_php_string();
            if !cbs.as_bytes().is_empty() {
                callback = Some(Value::String(cbs));
            }
        }
    }
    if let Some(cb) = callback {
        // [&$obj, "method"] callables: unwrap element references.
        let cb = match &cb {
            Value::Array(a) => {
                let mut c = PhpArray::new();
                for (k, v) in a.borrow().iter() {
                    c.set(k.clone(), v.deref());
                }
                Value::Array(Rc::new(RefCell::new(c)))
            }
            _ => cb,
        };
        let mut cargs = vec![
            Value::String(PhpString::from_string(vm.current_file.to_string())),
            Value::Long(line as i64),
            Value::Null,
        ];
        if let Some(d) = &desc_str {
            cargs.push(Value::String(d.clone()));
        }
        // zend_call_function(): an uncallable callback throws.
        let chk = vm.zend_is_callable_ex(&cb, 0);
        if !chk.ok && vm.current_exception.is_none() {
            let msg = format!(
                "Invalid callback {}, {}",
                goro_core::string::msg_str(&chk.name),
                chk.error.unwrap_or_default()
            );
            let exc = vm.create_exception(b"Error", &msg, line);
            vm.current_exception = Some(exc);
        } else if chk.ok {
            let mut full = vec![cb.clone()];
            full.extend(cargs);
            let _ = call_user_func(vm, &full);
        }
        vm.current_line = line;
        if vm.current_exception.is_some() {
            // With assert.bail, the callback's exception is reported as a
            // warning after the assertion's, and the script exits.
            if !assert_ini_bool(vm, b"assert.exception", true)
                && assert_ini_bool(vm, b"assert.bail", false)
            {
                if assert_ini_bool(vm, b"assert.warning", true) {
                    let d = desc_str
                        .as_ref()
                        .map(|d| d.to_string_lossy())
                        .unwrap_or_else(|| "Assertion".to_string());
                    vm.emit_warning_at(&format!("assert(): {} failed", d), line);
                }
                let fatal_before = vm.fatal_emitted;
                vm.uncaught_severity = Some(2);
                let e = VmError {
                    message: "Uncaught exception".into(),
                    line,
                };
                vm.report_uncaught(&e);
                vm.uncaught_severity = None;
                vm.fatal_emitted = fatal_before;
                vm.current_exception = None;
                vm.exit_requested = true;
                return Ok(Value::Null);
            }
            return Err(VmError {
                message: "Uncaught exception".into(),
                line,
            });
        }
    }
    if assert_ini_bool(vm, b"assert.exception", true) {
        let msg = desc_str
            .as_ref()
            .map(|d| d.to_msg_string())
            .unwrap_or_default();
        // The dispatcher may already have pushed our frame.
        let pushed = vm
            .call_stack
            .last()
            .map(|f| f.0 != "assert")
            .unwrap_or(true);
        if pushed {
            vm.call_stack.push((
                "assert".to_string(),
                vm.current_file.clone(),
                line,
                args.to_vec(),
                false,
            ));
        }
        let exc = vm.create_exception(b"AssertionError", &msg, line);
        if pushed {
            vm.call_stack.pop();
        }
        if let Value::Object(o) = &exc {
            o.borrow_mut()
                .set_property(b"code".to_vec(), Value::Long(1));
        }
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: format!("Uncaught AssertionError: {}", msg),
            line,
        });
    } else if assert_ini_bool(vm, b"assert.warning", true) {
        let d = desc_str
            .as_ref()
            .map(|d| d.to_string_lossy())
            .unwrap_or_else(|| "Assertion".to_string());
        vm.emit_warning_at(&format!("assert(): {} failed", d), line);
    }
    if assert_ini_bool(vm, b"assert.bail", false) {
        // zend_throw_unwind_exit()
        vm.exit_requested = true;
        return Ok(Value::Null);
    }
    Ok(Value::False)
}
/// class_exists_impl(): look the name up in the class table (autoloading
/// unless disabled) and check its kind.
fn class_exists_impl(
    vm: &mut Vm,
    args: &[Value],
    accept: fn(goro_core::class_table::ClassKind) -> bool,
) -> Result<Value, VmError> {
    let name = args.first().unwrap_or(&Value::Null).to_php_string();
    let raw_bytes = name.as_bytes();
    let name_bytes = raw_bytes.strip_prefix(b"\\").unwrap_or(raw_bytes);
    let name_lower = name_bytes.to_ascii_lowercase();
    let mut kind = vm.class_kind(&name_lower);
    if kind.is_none() && args.get(1).is_none_or(|v| v.to_bool()) {
        vm.try_autoload_class(name_bytes);
        kind = vm.class_kind(&name_lower);
    }
    Ok(if kind.is_some_and(accept) {
        Value::True
    } else {
        Value::False
    })
}
fn class_exists(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    use goro_core::class_table::ClassKind;
    class_exists_impl(vm, args, |k| {
        matches!(k, ClassKind::Class | ClassKind::Enum)
    })
}
fn get_declared_classes(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    use goro_core::class_table::ClassKind;
    let mut result = PhpArray::new();
    for name in vm.declared_class_names(&[ClassKind::Class, ClassKind::Enum]) {
        result.push(Value::String(PhpString::from_vec(name)));
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn get_declared_traits_fn(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    use goro_core::class_table::ClassKind;
    let mut result = PhpArray::new();
    for name in vm.declared_class_names(&[ClassKind::Trait]) {
        result.push(Value::String(PhpString::from_vec(name)));
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn get_declared_interfaces_fn(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    use goro_core::class_table::ClassKind;
    let mut result = PhpArray::new();
    for name in vm.declared_class_names(&[ClassKind::Interface]) {
        result.push(Value::String(PhpString::from_vec(name)));
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}
fn method_exists(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let class_or_obj = args.first().unwrap_or(&Value::Null);
    let method_name = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let method_lower: Vec<u8> = method_name
        .as_bytes()
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect();

    let class_lower: Vec<u8> = match class_or_obj {
        Value::Object(obj) => {
            let obj = obj.borrow();
            // Closures have __invoke method
            let cn_lower: Vec<u8> = obj
                .class_name
                .iter()
                .map(|b| b.to_ascii_lowercase())
                .collect();
            if cn_lower.starts_with(b"__closure") || cn_lower == b"closure" {
                if method_lower == b"__invoke" {
                    return Ok(Value::True);
                }
                return Ok(Value::False);
            }
            obj.class_name
                .iter()
                .map(|c| c.to_ascii_lowercase())
                .collect()
        }
        Value::String(s) => {
            // Closures are stored as strings starting with "__closure"
            if s.as_bytes().starts_with(b"__closure") && method_lower == b"__invoke" {
                return Ok(Value::True);
            }
            let lower: Vec<u8> = s
                .as_bytes()
                .iter()
                .map(|c| c.to_ascii_lowercase())
                .collect();
            if !vm.classes.contains_key(&lower) && !vm.is_known_builtin_class(&lower) {
                let name = s.as_bytes().to_vec();
                vm.try_autoload_class(&name);
                if vm.current_exception.is_some() {
                    return Err(VmError {
                        message: "Uncaught exception".into(),
                        line: vm.current_line,
                    });
                }
            }
            lower
        }
        Value::Reference(r) => {
            let inner = r.borrow().clone();
            let mut a = args.to_vec();
            a[0] = inner;
            return method_exists(vm, &a);
        }
        other => {
            return Err(throw_type_error_msg(
                vm,
                format!(
                    "method_exists(): Argument #1 ($object_or_class) must be of type object|string, {} given",
                    Vm::value_type_name(other)
                ),
            ));
        }
    };
    let is_object = matches!(class_or_obj, Value::Object(_));

    // Walk parent chain to check for method
    let mut current = class_lower.clone();
    for _ in 0..50 {
        if let Some(class) = vm.classes.get(&current) {
            if let Some(m) = class.methods.get(&method_lower) {
                // Ignores visibility, except inherited private methods looked up by class name.
                let private_inherited = m.visibility == Visibility::Private
                    && lower_bytes(&m.declaring_class) != class_lower;
                return Ok(if is_object || !private_inherited {
                    Value::True
                } else {
                    Value::False
                });
            }
            if let Some(ref parent) = class.parent {
                current = parent.iter().map(|b| b.to_ascii_lowercase()).collect();
            } else {
                break;
            }
        } else {
            break;
        }
    }

    // An enum's implicit UnitEnum / BackedEnum methods.
    if let Some(ce) = vm.classes.get(&class_lower).filter(|c| c.is_enum) {
        let backed = ce.enum_backing_type.is_some();
        if method_lower == b"cases"
            || backed && matches!(method_lower.as_slice(), b"from" | b"tryfrom")
        {
            return Ok(Value::True);
        }
    }
    // An internal class (or the internal ancestor of a user class): its
    // stub's methods, inherited ones included.
    if goro_core::stubs::find_method(&current, &method_lower).is_some() {
        return Ok(Value::True);
    }
    // Extension-provided built-in classes
    if let Some((native, h)) = vm.find_native_class_handler(&class_lower) {
        if (h.has_method)(&native, &method_lower, false)
            || (h.has_method)(&native, &method_lower, true)
            || (method_lower == b"__construct"
                && vm.object_construct_handlers.contains_key(&native))
        {
            return Ok(Value::True);
        }
    }
    // An internal class declared by php-src's stubs (e.g. ReflectionClass,
    // whose methods are dispatched natively).
    if vm.is_known_builtin_class(&current) {
        if let Some((decl, m)) = goro_core::stubs::find_method(&current, &method_lower) {
            // An inherited private method is not found by class name.
            let private = m.flags & goro_core::stubs::F_PRIVATE != 0;
            if is_object || !private || decl.name.as_bytes().eq_ignore_ascii_case(&class_lower) {
                return Ok(Value::True);
            }
        }
    }

    // Check built-in class methods
    let has_builtin_method = match class_lower.as_slice() {
        b"closure" => matches!(
            method_lower.as_slice(),
            b"__invoke" | b"call" | b"bind" | b"bindto" | b"fromcallable"
        ),
        b"exception"
        | b"error"
        | b"runtimeexception"
        | b"logicexception"
        | b"typeerror"
        | b"valueerror"
        | b"argumentcounterror"
        | b"rangeerror"
        | b"arithmeticerror"
        | b"divisionbyzeroerror"
        | b"invalidargumentexception"
        | b"badmethodcallexception"
        | b"overflowexception"
        | b"underflowexception"
        | b"domainexception"
        | b"unexpectedvalueexception"
        | b"lengthexception"
        | b"outofrangeexception"
        | b"outofboundsexception"
        | b"errorexception"
        | b"assertionerror"
        | b"unhandledmatcherror"
        | b"closedgeneratorexception" => matches!(
            method_lower.as_slice(),
            b"getmessage"
                | b"getcode"
                | b"getfile"
                | b"getline"
                | b"gettrace"
                | b"gettraceAsstring"
                | b"gettraceasstring"
                | b"getprevious"
                | b"__tostring"
                | b"__construct"
        ),
        b"generator" => matches!(
            method_lower.as_slice(),
            b"current"
                | b"key"
                | b"next"
                | b"rewind"
                | b"send"
                | b"throw"
                | b"valid"
                | b"getreturn"
        ),
        b"stdclass" => method_lower == b"__construct",
        b"arrayobject" | b"arrayiterator" | b"recursivearrayiterator" => matches!(
            method_lower.as_slice(),
            b"__construct"
                | b"offsetexists"
                | b"offsetget"
                | b"offsetset"
                | b"offsetunset"
                | b"count"
                | b"getiterator"
                | b"append"
                | b"getarraycopy"
                | b"getflags"
                | b"setflags"
        ),
        b"splfixedarray" => matches!(
            method_lower.as_slice(),
            b"__construct"
                | b"count"
                | b"offsetexists"
                | b"offsetget"
                | b"offsetset"
                | b"offsetunset"
                | b"fromarray"
                | b"toarray"
                | b"getsize"
                | b"setsize"
        ),
        b"spldoublylinkedlist" | b"splstack" | b"splqueue" => matches!(
            method_lower.as_slice(),
            b"__construct"
                | b"count"
                | b"push"
                | b"pop"
                | b"shift"
                | b"unshift"
                | b"top"
                | b"bottom"
                | b"isempty"
                | b"current"
                | b"key"
                | b"next"
                | b"prev"
                | b"rewind"
                | b"valid"
                | b"offsetexists"
                | b"offsetget"
                | b"offsetset"
                | b"offsetunset"
        ),
        _ => false,
    };
    // An IteratorIterator forwards unknown methods to its inner iterator.
    let forwarded = !has_builtin_method
        && matches!(class_or_obj, Value::Object(o) if vm.spl_forwarded_method_exists(o, &method_lower));
    Ok(if has_builtin_method || forwarded {
        Value::True
    } else {
        Value::False
    })
}
fn is_object(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // (closures are Closure objects: a string or array never is one)
    Ok(match args.first().map(|v| v.deref()) {
        Some(Value::Object(_) | Value::Generator(_)) => Value::True,
        _ => Value::False,
    })
}
fn debug_zval_refcount(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Null)
}
fn extract_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // extract() is compiled to the dedicated OpCode::Extract since it modifies
    // the caller's local scope. This stub exists only so that
    // function_exists('extract') and call_user_func('extract', ...) don't fail.
    Ok(Value::Long(0))
}
/// array_column_fetch_prop()
fn array_column_fetch(vm: &mut Vm, data: &Value, name: &Value) -> Option<Value> {
    match data {
        Value::Object(o) => {
            let pname: Vec<u8> = name.to_php_string().as_bytes().to_vec();
            // has_property(ZEND_PROPERTY_EXISTS) + read_property of objects
            // with their own handlers
            if let Some(h) = goro_core::vm::native_handlers(o) {
                if let (Some(has), Some(read)) = (h.has_property, h.read_property) {
                    match has(vm, o, &pname, 2) {
                        Some(true) => {
                            if let Some(v) = read(vm, o, &pname, 2) {
                                return Some(v.deref());
                            }
                        }
                        Some(false) => return None,
                        None => {}
                    }
                }
            }
            let class_lower = o.borrow().class_name.to_ascii_lowercase();
            let scope = vm.current_class_scope().map(|c| c.to_ascii_lowercase());
            let (accessible, declared) = match vm.get_class_def(&class_lower).and_then(|c| {
                c.properties
                    .iter()
                    .find(|p| p.name == pname && !p.is_static)
                    .map(|p| (p.visibility, p.declaring_class.clone()))
            }) {
                Some((vis, decl)) => (
                    match vis {
                        goro_core::object::Visibility::Public => true,
                        goro_core::object::Visibility::Private => {
                            scope.as_deref() == Some(decl.as_slice())
                        }
                        goro_core::object::Visibility::Protected => scope
                            .as_ref()
                            .map(|sc| {
                                sc.as_slice() == decl.as_slice()
                                    || vm.class_extends(sc, &decl)
                                    || vm.class_extends(&decl, sc)
                            })
                            .unwrap_or(false),
                    },
                    true,
                ),
                None => (true, false),
            };
            let present = {
                let ob = o.borrow();
                ob.has_property(&pname) && !matches!(ob.get_property(&pname), Value::Undef)
            };
            if accessible && present {
                return Some(o.borrow().get_property(&pname).deref());
            }
            let _ = declared;
            let has_isset = vm
                .get_class_def(&class_lower)
                .map(|c| c.get_method(b"__isset").is_some())
                .unwrap_or(false);
            if !has_isset {
                return None;
            }
            let pv = Value::String(PhpString::from_vec(pname.clone()));
            let isset = vm
                .call_object_method(data, b"__isset", std::slice::from_ref(&pv))
                .unwrap_or(Value::False);
            if !isset.is_truthy() {
                return None;
            }
            let has_get = vm
                .get_class_def(&class_lower)
                .map(|c| c.get_method(b"__get").is_some())
                .unwrap_or(false);
            if has_get {
                vm.call_object_method(data, b"__get", &[pv])
                    .map(|v| v.deref())
            } else if present {
                Some(o.borrow().get_property(&pname).deref())
            } else {
                None
            }
        }
        Value::Array(a) => {
            let key = match name {
                Value::Long(n) => ArrayKey::Int(*n),
                other => value_to_array_key(other),
            };
            a.borrow().get(&key).map(|v| v.deref())
        }
        _ => None,
    }
}

fn array_column(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let first = args.first().unwrap_or(&Value::Null).deref();
    require_array_arg(vm, &first, "array_column", "array", 1)?;
    let input: Vec<Value> = match &first {
        Value::Array(a) => a.borrow().values().cloned().collect(),
        _ => return Ok(Value::Null),
    };
    // Z_PARAM_STR_OR_LONG_OR_NULL
    let key_param = |vm: &mut Vm, idx: usize, pname: &str| -> Result<Option<Value>, VmError> {
        let v = match args.get(idx) {
            Some(v) => v.deref(),
            None => return Ok(None),
        };
        match &v {
            Value::Null | Value::Undef => Ok(None),
            Value::Long(_) | Value::String(_) => Ok(Some(v.clone())),
            Value::Double(f) if !vm.caller_strict_types => {
                if f.is_finite() && f.fract() != 0.0 {
                    vm.emit_deprecated(&format!(
                        "Implicit conversion from float {} to int loses precision",
                        goro_core::value::format_php_float_repr(*f)
                    ));
                }
                Ok(Some(Value::Long(*f as i64)))
            }
            Value::True | Value::False if !vm.caller_strict_types => {
                Ok(Some(Value::Long(matches!(v, Value::True) as i64)))
            }
            Value::Object(o)
                if !vm.caller_strict_types
                    && vm
                        .get_class_def(&o.borrow().class_name.to_ascii_lowercase())
                        .map(|c| c.get_method(b"__tostring").is_some())
                        .unwrap_or(false) =>
            {
                Ok(Some(Value::String(vm.value_to_string(&v))))
            }
            _ => {
                let tn = match &v {
                    Value::True => "true".to_string(),
                    Value::False => "false".to_string(),
                    _ => Vm::value_type_name(&v).to_string(),
                };
                let msg = format!(
                    "array_column(): Argument #{} (${}) must be of type string|int|null, {} given",
                    idx + 1,
                    pname,
                    tn
                );
                let exc = vm.throw_type_error(msg.clone());
                vm.current_exception = Some(exc);
                Err(VmError {
                    message: msg,
                    line: vm.current_line,
                })
            }
        }
    };
    let column = key_param(vm, 1, "column_key")?;
    let index = key_param(vm, 2, "index_key")?;
    let mut result = PhpArray::new();
    for row in input {
        let data = row.deref();
        let colval = match &column {
            None => data.clone(),
            Some(c) => match array_column_fetch(vm, &data, c) {
                Some(v) => v,
                None => continue,
            },
        };
        match &index {
            None => result.push(colval),
            Some(ik) => match array_column_fetch(vm, &data, ik) {
                Some(kv) => {
                    // array_set_zval_key()
                    let line = vm.current_line;
                    match &kv {
                        Value::Array(_) | Value::Object(_) => {
                            let tn = if let Value::Object(o) = &kv {
                                goro_core::value::display_class_name(&o.borrow().class_name)
                            } else {
                                "array".to_string()
                            };
                            let msg = format!("Cannot access offset of type {} on array", tn);
                            let exc = vm.throw_type_error(msg.clone());
                            vm.current_exception = Some(exc);
                            return Err(VmError { message: msg, line });
                        }
                        _ => {
                            let key = vm.value_to_array_key_checked(&kv, line);
                            result.set(key, colval);
                        }
                    }
                }
                None => result.push(colval),
            },
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}
fn value_to_array_key(val: &Value) -> goro_core::array::ArrayKey {
    match val {
        Value::Long(n) => goro_core::array::ArrayKey::Int(*n),
        Value::String(s) => {
            // Try to parse as integer
            if let Ok(n) = s.to_string_lossy().parse::<i64>() {
                goro_core::array::ArrayKey::Int(n)
            } else {
                goro_core::array::ArrayKey::String(s.clone())
            }
        }
        Value::True => goro_core::array::ArrayKey::Int(1),
        Value::False => goro_core::array::ArrayKey::Int(0),
        Value::Null => goro_core::array::ArrayKey::String(PhpString::empty()),
        Value::Double(f) => goro_core::array::ArrayKey::Int(*f as i64),
        _ => goro_core::array::ArrayKey::String(val.to_php_string()),
    }
}
fn array_count_values(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let first = args.first().unwrap_or(&Value::Null);
    require_array_arg(vm, first, "array_count_values", "array", 1)?;
    if let Some(Value::Array(arr)) = args.first() {
        let arr = arr.borrow();
        let mut result = PhpArray::new();
        for (_, val) in arr.iter() {
            let key = match val.deref() {
                Value::Long(n) => ArrayKey::Int(n),
                v @ Value::String(_) => Vm::value_to_array_key(v),
                _ => {
                    // PHP warns and skips non-int/non-string values
                    vm.emit_warning("array_count_values(): Can only count string and integer values, entry skipped");
                    continue;
                }
            };
            if let Some(existing) = result.get(&key) {
                let count = existing.to_long() + 1;
                result.set(key, Value::Long(count));
            } else {
                result.set(key, Value::Long(1));
            }
        }
        Ok(Value::Array(Rc::new(RefCell::new(result))))
    } else {
        Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
    }
}
fn array_rand(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let arr = match args.first() {
        Some(Value::Array(a)) => a.clone(),
        _ => return Ok(Value::Null),
    };
    let num = args.get(1).map(|v| v.to_long()).unwrap_or(1);
    let keys: Vec<goro_core::array::ArrayKey> = arr.borrow().keys().cloned().collect();
    let engine = goro_core::random::default_engine();
    match goro_core::random::pick_keys(vm, &engine, &keys, num, "array_rand") {
        Some(v) => Ok(v),
        None => {
            let message = match &vm.current_exception {
                Some(Value::Object(o)) => o
                    .borrow()
                    .get_property(b"message")
                    .to_php_string()
                    .to_msg_string(),
                _ => String::new(),
            };
            Err(VmError {
                message,
                line: vm.current_line,
            })
        }
    }
}

// === String extras ===

fn str_split(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = crate::strings::zpp_str(vm, args, 0, "str_split", "string")?;
    let len = args.get(1).map(|v| v.to_long()).unwrap_or(1);
    if len < 1 {
        let msg = "str_split(): Argument #2 ($length) must be greater than 0".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    let len = len as usize;
    let bytes = s.as_bytes();
    let mut result = PhpArray::new();
    // PHP 8.3+: empty string returns empty array
    if !bytes.is_empty() {
        for chunk in bytes.chunks(len) {
            result.push(Value::String(PhpString::from_vec(chunk.to_vec())));
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn number_format(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // (the arguments were parsed by the generic zpp layer: Z_PARAM_NUMBER,
    // Z_PARAM_LONG, Z_PARAM_STRING_OR_NULL x2)
    let num = match args.first().map(|v| v.deref()) {
        Some(v @ (Value::Long(_) | Value::Double(_))) => v,
        Some(v) => Value::Double(v.to_double()),
        None => Value::Long(0),
    };
    let dec = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let dec_point = match args.get(2).map(|v| v.deref()) {
        Some(Value::Null) | Some(Value::Undef) | None => PhpString::from_bytes(b"."),
        Some(v) => v.to_php_string(),
    };
    let thousands_sep = match args.get(3).map(|v| v.deref()) {
        Some(Value::Null) | Some(Value::Undef) | None => PhpString::from_bytes(b","),
        Some(v) => v.to_php_string(),
    };

    // _php_math_number_format_long(): integers (and doubles >= 2^52 that
    // fit a long) are formatted exactly.
    let as_long = match num {
        Value::Long(n) => Some(n),
        Value::Double(d)
            if (d >= 4503599627370496.0 || d <= -4503599627370496.0)
                && (-9223372036854775808.0..9223372036854775808.0).contains(&d) =>
        {
            Some(d as i64)
        }
        _ => None,
    };
    let out = if let Some(n) = as_long {
        crate::math::number_format_long(vm, n, dec, dec_point.as_bytes(), thousands_sep.as_bytes())?
    } else {
        let dec = dec.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        number_format_ex(
            vm,
            num.to_double(),
            dec,
            dec_point.as_bytes(),
            thousands_sep.as_bytes(),
        )?
    };
    Ok(Value::String(PhpString::from_vec(out)))
}

/// _php_math_number_format_ex() (ext/standard/math.c).
pub(crate) fn number_format_ex(
    vm: &mut Vm,
    d: f64,
    dec: i32,
    dec_point: &[u8],
    thousand_sep: &[u8],
) -> Result<Vec<u8>, VmError> {
    let mut is_negative = d < 0.0;
    let d = crate::math::php_math_round(d.abs(), dec, 1);
    let dec = dec.max(0) as usize;
    // strpprintf("%.*F"): php_conv_fp() keeps at most NDIG - 2 decimals.
    let tmpbuf: Vec<u8> = if d.is_nan() {
        b"nan".to_vec()
    } else if d.is_infinite() {
        b"inf".to_vec()
    } else {
        format!("{:.*}", dec.min(318), d).into_bytes()
    };
    if !tmpbuf.first().is_some_and(|c| c.is_ascii_digit()) {
        return Ok(tmpbuf);
    }
    if is_negative && d == 0.0 {
        is_negative = false;
    }
    let dp = if dec > 0 {
        tmpbuf.iter().position(|&c| c == b'.' || c == b',')
    } else {
        None
    };
    let int_part = &tmpbuf[..dp.unwrap_or(tmpbuf.len())];
    // reslen, as allocated
    let mut reslen = int_part.len() + (int_part.len() - 1) / 3 * thousand_sep.len();
    if dec > 0 {
        reslen = reslen.saturating_add(dec).saturating_add(dec_point.len());
    }
    reslen += is_negative as usize;
    vm.memory_check(crate::math::zend_string_alloc_size(reslen))?;
    let mut out = Vec::with_capacity(reslen);
    if is_negative {
        out.push(b'-');
    }
    for (i, &c) in int_part.iter().enumerate() {
        if i > 0 && (int_part.len() - i).is_multiple_of(3) {
            out.extend_from_slice(thousand_sep);
        }
        out.push(c);
    }
    if dec > 0 {
        out.extend_from_slice(dec_point);
        let frac: &[u8] = match dp {
            Some(p) => &tmpbuf[p + 1..],
            None => b"",
        };
        out.extend_from_slice(frac);
        out.extend(std::iter::repeat_n(b'0', dec.saturating_sub(frac.len())));
    }
    Ok(out)
}

// hex2bin and bin2hex moved to strings.rs

fn base64_encode(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = data.as_bytes();
    const B64: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = Vec::with_capacity(bytes.len().div_ceil(3) * 4);
    let mut i = 0;
    while i + 2 < bytes.len() {
        let n = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8) | (bytes[i + 2] as u32);
        result.push(B64[((n >> 18) & 0x3F) as usize]);
        result.push(B64[((n >> 12) & 0x3F) as usize]);
        result.push(B64[((n >> 6) & 0x3F) as usize]);
        result.push(B64[(n & 0x3F) as usize]);
        i += 3;
    }
    let remaining = bytes.len() - i;
    if remaining == 2 {
        let n = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8);
        result.push(B64[((n >> 18) & 0x3F) as usize]);
        result.push(B64[((n >> 12) & 0x3F) as usize]);
        result.push(B64[((n >> 6) & 0x3F) as usize]);
        result.push(b'=');
    } else if remaining == 1 {
        let n = (bytes[i] as u32) << 16;
        result.push(B64[((n >> 18) & 0x3F) as usize]);
        result.push(B64[((n >> 12) & 0x3F) as usize]);
        result.push(b'=');
        result.push(b'=');
    }
    Ok(Value::String(PhpString::from_vec(result)))
}

#[allow(clippy::if_same_then_else)] // branches mirror php-src
fn base64_decode(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = args.first().unwrap_or(&Value::Null).to_php_string();
    let strict = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
    let input = data.as_bytes();

    fn b64_val(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }

    // Filter out whitespace (and invalid chars in non-strict mode)
    let mut filtered = Vec::with_capacity(input.len());
    for &b in input {
        if b == b'=' {
            filtered.push(b);
        } else if b64_val(b).is_some() {
            filtered.push(b);
        } else if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
            // whitespace is always ignored
        } else if strict {
            return Ok(Value::False);
        }
        // non-strict: skip invalid chars
    }

    // Validate padding in strict mode
    if strict {
        let content_len = filtered.iter().filter(|&&b| b != b'=').count();
        let pad_len = filtered.iter().filter(|&&b| b == b'=').count();
        // Check padding position (must be at end)
        let first_pad = filtered
            .iter()
            .position(|&b| b == b'=')
            .unwrap_or(filtered.len());
        if first_pad + pad_len != filtered.len() {
            return Ok(Value::False);
        }
        // Check valid content length and padding combination
        // PHP strict mode accepts: no padding (if content length mod 4 is 0, 2, or 3)
        // or correctly padded input
        let total = content_len + pad_len;
        match content_len % 4 {
            0 => {
                // Full groups: padding must be 0
                if pad_len != 0 {
                    return Ok(Value::False);
                }
            }
            2 => {
                // 2 extra chars: accepts 0 or 2 padding chars
                if pad_len != 0 && pad_len != 2 {
                    return Ok(Value::False);
                }
                if pad_len != 0 && total % 4 != 0 {
                    return Ok(Value::False);
                }
            }
            3 => {
                // 3 extra chars: accepts 0 or 1 padding char
                if pad_len != 0 && pad_len != 1 {
                    return Ok(Value::False);
                }
                if pad_len != 0 && total % 4 != 0 {
                    return Ok(Value::False);
                }
            }
            1 => {
                // 1 extra char can't decode to anything valid
                return Ok(Value::False);
            }
            _ => return Ok(Value::False),
        }
    }

    // Decode
    let mut result = Vec::new();
    let vals: Vec<u8> = filtered.iter().filter_map(|&b| b64_val(b)).collect();
    let mut i = 0;
    while i + 3 < vals.len() {
        let n = ((vals[i] as u32) << 18)
            | ((vals[i + 1] as u32) << 12)
            | ((vals[i + 2] as u32) << 6)
            | (vals[i + 3] as u32);
        result.push((n >> 16) as u8);
        result.push((n >> 8) as u8);
        result.push(n as u8);
        i += 4;
    }
    let remaining = vals.len() - i;
    if remaining == 3 {
        let n =
            ((vals[i] as u32) << 18) | ((vals[i + 1] as u32) << 12) | ((vals[i + 2] as u32) << 6);
        result.push((n >> 16) as u8);
        result.push((n >> 8) as u8);
    } else if remaining == 2 {
        let n = ((vals[i] as u32) << 18) | ((vals[i + 1] as u32) << 12);
        result.push((n >> 16) as u8);
    }
    Ok(Value::String(PhpString::from_vec(result)))
}
fn urlencode(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let mut result = Vec::new();
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => result.push(b),
            b' ' => result.push(b'+'),
            _ => {
                result.extend_from_slice(format!("%{:02X}", b).as_bytes());
            }
        }
    }
    Ok(Value::String(PhpString::from_vec(result)))
}
fn urldecode(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = s.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'+' {
            result.push(b' ');
            i += 1;
        } else if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = bytes[i + 1];
            let lo = bytes[i + 2];
            if hi.is_ascii_hexdigit() && lo.is_ascii_hexdigit() {
                let val = (hex_val_misc(hi) << 4) | hex_val_misc(lo);
                result.push(val);
                i += 3;
            } else {
                result.push(bytes[i]);
                i += 1;
            }
        } else {
            result.push(bytes[i]);
            i += 1;
        }
    }
    Ok(Value::String(PhpString::from_vec(result)))
}

fn hex_val_misc(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        _ => 0,
    }
}
fn rawurlencode(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let mut result = Vec::new();
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => result.push(b),
            _ => {
                result.extend_from_slice(format!("%{:02X}", b).as_bytes());
            }
        }
    }
    Ok(Value::String(PhpString::from_vec(result)))
}
fn rawurldecode(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = s.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = bytes[i + 1];
            let lo = bytes[i + 2];
            if hi.is_ascii_hexdigit() && lo.is_ascii_hexdigit() {
                let val = (hex_val_misc(hi) << 4) | hex_val_misc(lo);
                result.push(val);
                i += 3;
                continue;
            }
        }
        result.push(bytes[i]);
        i += 1;
    }
    Ok(Value::String(PhpString::from_vec(result)))
}

// htmlspecialchars/htmlentities/html_entity_decode/htmlspecialchars_decode
// moved to the html module.

/// PHP-style string comparison returning the byte difference (not just -1/0/1)
pub fn php_strcmp(a: &[u8], b: &[u8]) -> i64 {
    let min_len = a.len().min(b.len());
    for i in 0..min_len {
        if a[i] != b[i] {
            return (a[i] as i64) - (b[i] as i64);
        }
    }
    (a.len() as i64) - (b.len() as i64)
}
/// Validate that the format argument is a string (for fprintf/vfprintf where
/// `$stream` is arg #1 and `$format` is arg #2).
fn validate_fmt_arg2(vm: &mut Vm, func_name: &str, format_arg: &Value) -> Result<(), VmError> {
    let derefed = format_arg.deref();
    let type_str = match &derefed {
        Value::Array(_) => Some("array"),
        Value::Resource(_) => Some("resource"),
        Value::Object(obj) => {
            // Object with __toString is OK.
            let class_name = obj.borrow().class_name.clone();
            let class_lower: Vec<u8> = class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
            let has_tostring = vm
                .classes
                .get(&class_lower)
                .map(|c| c.get_method(b"__tostring").is_some())
                .unwrap_or(false);
            if has_tostring {
                None
            } else {
                let class_display = goro_core::string::msg_str(&class_name).to_string();
                let msg = format!(
                    "{}(): Argument #2 ($format) must be of type string, {} given",
                    func_name, class_display
                );
                let exc = vm.create_exception(b"TypeError", &msg, 0);
                vm.current_exception = Some(exc);
                return Err(VmError {
                    message: msg,
                    line: vm.current_line,
                });
            }
        }
        _ => None,
    };
    if let Some(t) = type_str {
        let msg = format!(
            "{}(): Argument #2 ($format) must be of type string, {} given",
            func_name, t
        );
        let exc = vm.create_exception(b"TypeError", &msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    Ok(())
}

/// Validate that the values argument is an array (for vfprintf where `$values`
/// is arg #3).
fn validate_values_arg3(vm: &mut Vm, func_name: &str, values_arg: &Value) -> Result<(), VmError> {
    let derefed = values_arg.deref();
    let type_name = match &derefed {
        Value::Array(_) => return Ok(()),
        Value::Long(_) => "int",
        Value::Double(_) => "float",
        Value::Null | Value::Undef => "null",
        Value::True => "true",
        Value::False => "false",
        Value::String(_) => "string",
        Value::Resource(_) => "resource",
        Value::Object(obj) => {
            let name = String::from_utf8_lossy(&obj.borrow().class_name).to_string();
            let msg = format!(
                "{}(): Argument #3 ($values) must be of type array, {} given",
                func_name, name
            );
            let exc = vm.create_exception(b"TypeError", &msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
        _ => "unknown",
    };
    let msg = format!(
        "{}(): Argument #3 ($values) must be of type array, {} given",
        func_name, type_name
    );
    let exc = vm.create_exception(b"TypeError", &msg, 0);
    vm.current_exception = Some(exc);
    Err(VmError {
        message: msg,
        line: vm.current_line,
    })
}

/// Validate that arg #1 is a resource (TypeError otherwise).
fn validate_stream_arg1(vm: &mut Vm, func_name: &str, stream_arg: &Value) -> Result<(), VmError> {
    // Z_PARAM_RESOURCE
    let derefed = stream_arg.deref();
    if matches!(derefed, Value::Resource(_)) {
        return Ok(());
    }
    let msg = format!(
        "{}(): Argument #1 ($stream) must be of type resource, {} given",
        func_name,
        Vm::value_type_name(&derefed)
    );
    let exc = vm.create_exception(b"TypeError", &msg, 0);
    vm.current_exception = Some(exc);
    Err(VmError {
        message: msg,
        line: vm.current_line,
    })
}

fn fprintf_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // fprintf($handle, $format, ...$args) - write formatted to file handle
    if args.len() < 2 {
        let msg = format!(
            "fprintf() expects at least 2 arguments, {} given",
            args.len()
        );
        let exc = vm.create_exception(b"ArgumentCountError", &msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    validate_stream_arg1(vm, "fprintf", &args[0])?;
    validate_fmt_arg2(vm, "fprintf", &args[1])?;
    crate::strings::deprecate_null_format(vm, "fprintf", 2, args.get(1));
    let fid = args[0].to_long();
    let format_args = &args[1..]; // skip the handle, pass format + args
    let (formatted, pending) =
        crate::strings::do_sprintf_vm_named_partial(vm, "fprintf", format_args)?;
    let len = formatted.len();

    // php_stream_write() on the stream (filters, read buffer, sockets).
    if has_file_handle(fid) {
        stream_write(vm, "fprintf", &args[0], &formatted);
    } else {
        vm.write_output(&formatted);
    }
    pending.map_or(Ok(Value::Long(len as i64)), Err)
}

fn vfprintf_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // vfprintf($handle, $format, $args_array)
    if args.len() != 3 {
        let msg = format!(
            "vfprintf() expects exactly 3 arguments, {} given",
            args.len()
        );
        let exc = vm.create_exception(b"ArgumentCountError", &msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    validate_stream_arg1(vm, "vfprintf", &args[0])?;
    validate_fmt_arg2(vm, "vfprintf", &args[1])?;
    crate::strings::deprecate_null_format(vm, "vfprintf", 2, args.get(1));
    validate_values_arg3(vm, "vfprintf", &args[2])?;
    let fid = args[0].to_long();
    let format = vm.value_to_string(&args[1]);
    let mut format_args = vec![Value::String(format)];
    if let Value::Array(arr) = &args[2] {
        for (_, v) in arr.borrow().iter() {
            format_args.push(v.clone());
        }
    }
    let (formatted, pending) =
        crate::strings::do_sprintf_vm_named_partial(vm, "vfprintf", &format_args)?;
    let len = formatted.len();

    if has_file_handle(fid) {
        stream_write(vm, "vfprintf", &args[0], &formatted);
    } else {
        vm.write_output(&formatted);
    }
    pending.map_or(Ok(Value::Long(len as i64)), Err)
}

// === Shell ===

/// Throw a ValueError from a builtin.
fn throw_value_error_msg(vm: &mut Vm, msg: &str) -> VmError {
    throw_value_error_in(vm, "", &[], msg)
}

/// Throw a ValueError with the builtin's own frame in the stack trace.
fn throw_value_error_in(vm: &mut Vm, func: &str, args: &[Value], msg: &str) -> VmError {
    if !func.is_empty() {
        vm.call_stack.push((
            func.to_string(),
            vm.current_file.clone(),
            vm.current_line,
            args.to_vec(),
            false,
        ));
    }
    let exc = vm.create_exception(b"ValueError", msg, vm.current_line);
    if !func.is_empty() {
        vm.call_stack.pop();
    }
    vm.current_exception = Some(exc);
    VmError {
        message: format!("Uncaught ValueError: {}", msg),
        line: vm.current_line,
    }
}

/// sysconf(_SC_ARG_MAX) on Linux.
const SHELL_CMD_MAX_LEN: usize = 2097152;

fn escapeshellarg_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    if s.as_bytes().contains(&0) {
        return Err(throw_value_error_in(
            vm,
            "escapeshellarg",
            args,
            "escapeshellarg(): Argument #1 ($arg) must not contain any null bytes",
        ));
    }
    if s.len() > SHELL_CMD_MAX_LEN - 3 {
        return Err(throw_value_error_msg(
            vm,
            &format!(
                "Argument exceeds the allowed length of {} bytes",
                SHELL_CMD_MAX_LEN
            ),
        ));
    }
    let mut result = Vec::new();
    result.push(b'\'');
    for &byte in s.as_bytes() {
        if byte == b'\'' {
            result.extend_from_slice(b"'\\''");
        } else {
            result.push(byte);
        }
    }
    result.push(b'\'');
    Ok(Value::String(PhpString::from_vec(result)))
}

fn escapeshellcmd_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let str = s.as_bytes();
    if str.contains(&0) {
        return Err(throw_value_error_in(
            vm,
            "escapeshellcmd",
            args,
            "escapeshellcmd(): Argument #1 ($command) must not contain any null bytes",
        ));
    }
    if str.len() > SHELL_CMD_MAX_LEN - 3 {
        return Err(throw_value_error_msg(
            vm,
            &format!(
                "Command exceeds the allowed length of {} bytes",
                SHELL_CMD_MAX_LEN
            ),
        ));
    }
    let mut result = Vec::new();
    // Quotes are only escaped when unpaired (php_escape_shell_cmd).
    let mut pending: Option<usize> = None;
    for (x, &byte) in str.iter().enumerate() {
        match byte {
            b'"' | b'\'' => {
                if pending.is_none() {
                    if let Some(off) = str[x + 1..].iter().position(|&c| c == byte) {
                        pending = Some(x + 1 + off);
                    } else {
                        result.push(b'\\');
                    }
                } else if pending.map(|p| str[p] == byte).unwrap_or(false) {
                    pending = None;
                } else {
                    result.push(b'\\');
                }
                result.push(byte);
            }
            b'#' | b'&' | b';' | b'`' | b'|' | b'*' | b'?' | b'~' | b'<' | b'>' | b'^' | b'('
            | b')' | b'[' | b']' | b'{' | b'}' | b'$' | b'\\' | b'\x0A' | b'\xFF' => {
                result.push(b'\\');
                result.push(byte);
            }
            _ => result.push(byte),
        }
    }
    if result.len() > SHELL_CMD_MAX_LEN + 1 {
        return Err(throw_value_error_msg(
            vm,
            &format!(
                "Escaped command exceeds the allowed length of {} bytes",
                SHELL_CMD_MAX_LEN
            ),
        ));
    }
    Ok(Value::String(PhpString::from_vec(result)))
}

/// PHP_FUNCTION(parse_str)
fn parse_str_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.len() < 2 {
        let msg = format!(
            "parse_str() expects exactly 2 arguments, {} given",
            args.len()
        );
        let exc = vm.create_exception(b"ArgumentCountError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    let s = crate::strings::zpp_str(vm, args, 0, "parse_str", "string")?;
    let arr = Rc::new(RefCell::new(PhpArray::new()));
    crate::request::parse_str_into(vm, s.as_bytes(), &arr);
    if let Value::Reference(r) = &args[1] {
        *r.borrow_mut() = Value::Array(arr);
    }
    Ok(Value::Null)
}

// === URL ===

#[derive(Default)]
struct PhpUrl {
    scheme: Option<Vec<u8>>,
    user: Option<Vec<u8>>,
    pass: Option<Vec<u8>>,
    host: Option<Vec<u8>>,
    port: u16,
    has_port: bool,
    path: Option<Vec<u8>>,
    query: Option<Vec<u8>>,
    fragment: Option<Vec<u8>>,
}

/// php_url_parse_ex2() as ext/uri's parse_url() parser uses it.
pub(crate) fn uri_parse_url(b: &[u8]) -> Option<goro_core::uri::PhpUrl> {
    let u = php_url_parse_ex2(b)?;
    Some(goro_core::uri::PhpUrl {
        scheme: u.scheme,
        user: u.user,
        pass: u.pass,
        host: u.host,
        port: if u.has_port { u.port } else { 0 },
        path: u.path,
        query: u.query,
        fragment: u.fragment,
    })
}

/// php_url_parse_ex2() (ext/standard/url.c)
fn php_url_parse_ex2(b: &[u8]) -> Option<PhpUrl> {
    fn ctl(v: &[u8]) -> Vec<u8> {
        v.iter()
            .map(|&c| if c < 0x20 || c == 0x7f { b'_' } else { c })
            .collect()
    }
    fn memchr(b: &[u8], from: usize, to: usize, c: u8) -> Option<usize> {
        b[from..to].iter().position(|&x| x == c).map(|p| p + from)
    }
    fn memrchr(b: &[u8], from: usize, to: usize, c: u8) -> Option<usize> {
        b[from..to].iter().rposition(|&x| x == c).map(|p| p + from)
    }
    fn strcspn(b: &[u8], from: usize, to: usize, chars: &[u8]) -> usize {
        let mut e = to;
        for &c in chars {
            if let Some(p) = memchr(b, from, e, c) {
                e = p;
            }
        }
        e
    }
    fn parse_port_num(digits: &[u8]) -> Option<u16> {
        // ZEND_STRTOL: leading digits only (the caller already checked them)
        let mut v: i64 = 0;
        let mut any = false;
        for &c in digits {
            if !c.is_ascii_digit() {
                break;
            }
            any = true;
            v = v * 10 + (c - b'0') as i64;
            if v > 100000 {
                break;
            }
        }
        if any && (0..=65535).contains(&v) {
            Some(v as u16)
        } else {
            None
        }
    }
    #[derive(PartialEq)]
    enum L {
        ParsePort,
        ParseHost,
        JustPath,
    }
    let mut ret = PhpUrl::default();
    let ue = b.len();
    let mut s = 0usize;
    let mut e_opt = memchr(b, 0, ue, b':');
    let label: L;
    'scheme: {
        if let Some(e) = e_opt.filter(|&e| e != s) {
            // validate scheme
            let mut p = s;
            while p < e {
                let c = b[p];
                if !c.is_ascii_alphabetic()
                    && !c.is_ascii_digit()
                    && c != b'+'
                    && c != b'.'
                    && c != b'-'
                {
                    if e + 1 < ue && e < strcspn(b, s, ue, b"?#") {
                        label = L::ParsePort;
                    } else if s + 1 < ue && b[s] == b'/' && b[s + 1] == b'/' {
                        s += 2;
                        e_opt = None;
                        label = L::ParseHost;
                    } else {
                        label = L::JustPath;
                    }
                    break 'scheme;
                }
                p += 1;
            }
            if e + 1 == ue {
                ret.scheme = Some(ctl(&b[s..e]));
                return Some(ret);
            }
            if b[e + 1] != b'/' {
                let mut p = e + 1;
                while p < ue && b[p].is_ascii_digit() {
                    p += 1;
                }
                if (p == ue || b[p] == b'/') && (p - e) < 7 {
                    label = L::ParsePort;
                    break 'scheme;
                }
                ret.scheme = Some(ctl(&b[s..e]));
                s = e + 1;
                label = L::JustPath;
                break 'scheme;
            } else {
                ret.scheme = Some(ctl(&b[s..e]));
                if e + 2 < ue && b[e + 2] == b'/' {
                    s = e + 3;
                    if ret
                        .scheme
                        .as_deref()
                        .map(|x| x.eq_ignore_ascii_case(b"file"))
                        .unwrap_or(false)
                        && e + 3 < ue
                        && b[e + 3] == b'/'
                    {
                        if e + 5 < ue && b[e + 5] == b':' {
                            s = e + 4;
                        }
                        label = L::JustPath;
                        break 'scheme;
                    }
                    label = L::ParseHost;
                } else {
                    s = e + 1;
                    label = L::JustPath;
                }
                break 'scheme;
            }
        } else if e_opt.is_some() {
            label = L::ParsePort;
        } else if s + 1 < ue && b[s] == b'/' && b[s + 1] == b'/' {
            s += 2;
            label = L::ParseHost;
        } else {
            label = L::JustPath;
        }
    }
    let mut label = label;
    if label == L::ParsePort {
        let e = e_opt.unwrap();
        let p = e + 1;
        let mut pp = p;
        while pp < ue && pp - p < 6 && b[pp].is_ascii_digit() {
            pp += 1;
        }
        if pp - p > 0 && pp - p < 6 && (pp == ue || b[pp] == b'/') {
            {
                let port = parse_port_num(&b[p..pp])?;
                ret.has_port = true;
                ret.port = port;
                if s + 1 < ue && b[s] == b'/' && b[s + 1] == b'/' {
                    s += 2;
                }
                label = L::ParseHost;
            }
        } else if p == pp && pp == ue {
            return None;
        } else if s + 1 < ue && b[s] == b'/' && b[s + 1] == b'/' {
            s += 2;
            label = L::ParseHost;
        } else {
            label = L::JustPath;
        }
    }
    if label == L::ParseHost {
        let e = strcspn(b, s, ue, b"/?#");
        if let Some(p) = memrchr(b, s, e, b'@') {
            if let Some(pp) = memchr(b, s, p, b':') {
                ret.user = Some(ctl(&b[s..pp]));
                ret.pass = Some(ctl(&b[pp + 1..p]));
            } else {
                ret.user = Some(ctl(&b[s..p]));
            }
            s = p + 1;
        }
        let pcolon = if s < ue && b[s] == b'[' && e > 0 && b[e - 1] == b']' {
            None
        } else {
            memrchr(b, s, e.max(s), b':')
        };
        let p = match pcolon {
            Some(pc) => {
                if ret.port == 0 {
                    let pd = pc + 1;
                    if e - pd > 5 {
                        return None;
                    } else if e - pd > 0 {
                        {
                            let port = parse_port_num(&b[pd..e])?;
                            ret.has_port = true;
                            ret.port = port;
                        }
                    }
                }
                pc
            }
            None => e,
        };
        if p < s + 1 {
            return None;
        }
        ret.host = Some(ctl(&b[s..p]));
        if e == ue {
            return Some(ret);
        }
        s = e;
    }
    // just_path
    let mut e = ue;
    if let Some(p) = memchr(b, s, e, b'#') {
        let p1 = p + 1;
        ret.fragment = Some(if p1 < e { ctl(&b[p1..e]) } else { Vec::new() });
        e = p;
    }
    if let Some(p) = memchr(b, s, e, b'?') {
        let p1 = p + 1;
        ret.query = Some(if p1 < e { ctl(&b[p1..e]) } else { Vec::new() });
        e = p;
    }
    if s < e || s == ue {
        ret.path = Some(ctl(&b[s..e]));
    }
    Some(ret)
}

fn parse_url_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let st = vm.value_to_string(args.first().unwrap_or(&Value::Null));
    let key = args.get(1).map(|v| v.to_long()).unwrap_or(-1);
    let res = match php_url_parse_ex2(st.as_bytes()) {
        Some(r) => r,
        None => return Ok(Value::False),
    };
    let sv = |v: &Option<Vec<u8>>| {
        v.as_ref()
            .map(|x| Value::String(PhpString::from_vec(x.clone())))
            .unwrap_or(Value::Null)
    };
    if key > -1 {
        return Ok(match key {
            0 => sv(&res.scheme),
            1 => sv(&res.host),
            2 => {
                if res.has_port {
                    Value::Long(res.port as i64)
                } else {
                    Value::Null
                }
            }
            3 => sv(&res.user),
            4 => sv(&res.pass),
            5 => sv(&res.path),
            6 => sv(&res.query),
            7 => sv(&res.fragment),
            _ => {
                let msg = format!(
                    "parse_url(): Argument #2 ($component) must be a valid URL component identifier, {} given",
                    key
                );
                let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
                vm.current_exception = Some(exc);
                return Err(VmError {
                    message: msg,
                    line: vm.current_line,
                });
            }
        });
    }
    let mut out = PhpArray::new();
    let mut put = |k: &[u8], v: Value| out.set(ArrayKey::String(PhpString::from_bytes(k)), v);
    if res.scheme.is_some() {
        put(b"scheme", sv(&res.scheme));
    }
    if res.host.is_some() {
        put(b"host", sv(&res.host));
    }
    if res.has_port {
        put(b"port", Value::Long(res.port as i64));
    }
    if res.user.is_some() {
        put(b"user", sv(&res.user));
    }
    if res.pass.is_some() {
        put(b"pass", sv(&res.pass));
    }
    if res.path.is_some() {
        put(b"path", sv(&res.path));
    }
    if res.query.is_some() {
        put(b"query", sv(&res.query));
    }
    if res.fragment.is_some() {
        put(b"fragment", sv(&res.fragment));
    }
    Ok(Value::Array(Rc::new(RefCell::new(out))))
}

thread_local! {
    /// Arrays http_build_query() is currently encoding.
    static HBQ_ACTIVE: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}

/// Nesting levels the stack allows for http_build_query().
fn http_build_query_max_depth(vm: &Vm) -> usize {
    let q = |k: &[u8]| -> i64 {
        vm.constants
            .get(k)
            .map(|v| {
                goro_core::ini_quantity::parse_quantity(v.to_php_string().as_bytes(), true).0 as i64
            })
            .unwrap_or(0)
    };
    let max = q(b"zend.max_allowed_stack_size");
    if max < 0 {
        return usize::MAX;
    }
    let max = if max == 0 { 8 * 1024 * 1024 } else { max };
    let usable = (max - q(b"zend.reserved_stack_size")).max(0) as usize;
    usable / 256
}

/// php_url_encode_to_smart_str(): urlencode(), or rawurlencode() for
/// PHP_QUERY_RFC3986.
fn hbq_url_encode(out: &mut Vec<u8>, s: &[u8], raw: bool) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for &c in s {
        if c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.') || (raw && c == b'~') {
            out.push(c);
        } else if !raw && c == b' ' {
            out.push(b'+');
        } else {
            out.extend_from_slice(&[b'%', HEX[(c >> 4) as usize], HEX[(c & 15) as usize]]);
        }
    }
}

/// A key of the table php_url_encode_hash_ex() walks.
enum HbqKey {
    Int(i64),
    Str(Vec<u8>),
}

/// The entries of HASH_OF(`v`) as php_url_encode_hash_ex() sees them: an
/// array's elements, or the properties of an object accessible from the
/// calling scope (zend_check_property_access()) under their unmangled
/// names, with the table's identity. None when `v` has no table.
#[allow(clippy::type_complexity)]
fn hbq_entries(vm: &mut Vm, v: &Value) -> Option<(usize, Vec<(HbqKey, Value)>)> {
    match v {
        Value::Array(arr) => {
            let entries = arr
                .borrow()
                .iter()
                .map(|(k, v)| {
                    let k = match k {
                        goro_core::array::ArrayKey::Int(n) => HbqKey::Int(*n),
                        goro_core::array::ArrayKey::String(s) => HbqKey::Str(s.as_bytes().to_vec()),
                    };
                    (k, v.clone())
                })
                .collect();
            Some((Rc::as_ptr(arr) as *const u8 as usize, entries))
        }
        Value::Object(outer) => {
            vm.maybe_run_lazy_init(outer);
            if vm.current_exception.is_some() {
                return None;
            }
            let obj = vm.proxy_redirect(outer);
            let class_lower = obj.borrow().class_name.to_ascii_lowercase();
            let mut entries = Vec::new();
            for e in vm.object_prop_table(&obj) {
                let is_dynamic = e.slot.is_none();
                if is_dynamic
                    && (goro_core::proptable::is_internal_prop_name(&e.key)
                        || goro_core::foreach_obj::is_date_view_prop(&obj.borrow(), &e.key))
                {
                    continue;
                }
                // (IS_INDIRECT to an undefined slot: virtual / uninitialized)
                if matches!(e.value, Value::Undef) {
                    continue;
                }
                if !vm.check_property_access(&class_lower, &e.key, is_dynamic) {
                    continue;
                }
                let name = match e.slot {
                    Some(slot) => slot.name,
                    None => e.key,
                };
                entries.push((HbqKey::Str(name), e.value));
            }
            Some((Rc::as_ptr(&obj) as *const u8 as usize, entries))
        }
        // (a Generator's property table is empty)
        Value::Generator(g) => Some((Rc::as_ptr(g) as *const u8 as usize, Vec::new())),
        _ => None,
    }
}

/// Whether `v` (dereferenced) is an enum case.
fn hbq_is_enum(vm: &Vm, v: &Value) -> bool {
    match v {
        Value::Object(o) => {
            let lower = o.borrow().class_name.to_ascii_lowercase();
            vm.classes.get(&lower).is_some_and(|c| c.is_enum)
        }
        _ => false,
    }
}

/// php_url_encode_hash_ex(); `out` is the form string (None: nothing
/// appended yet, so no separator before the next pair).
#[allow(clippy::too_many_arguments)]
fn hbq_encode_hash(
    vm: &mut Vm,
    out: &mut Option<Vec<u8>>,
    data: &Value,
    num_prefix: Option<&[u8]>,
    key_prefix: Option<&[u8]>,
    arg_sep: &[u8],
    enc_type: i64,
    depth: usize,
) {
    let raw = enc_type == 2; // PHP_QUERY_RFC3986
    let Some((ptr, entries)) = hbq_entries(vm, data) else {
        return;
    };
    // GC_IS_RECURSIVE: a table already being encoded is skipped.
    if HBQ_ACTIVE.with(|a| a.borrow().contains(&ptr)) {
        return;
    }
    // zend_call_stack_overflowed(): nesting deeper than the stack allows
    // (zend.max_allowed_stack_size, ~256 bytes per level).
    if depth > http_build_query_max_depth(vm) {
        if vm.current_exception.is_none() {
            let exc = vm.create_exception(
                b"Error",
                "Maximum call stack size reached.",
                vm.current_line,
            );
            vm.current_exception = Some(exc);
        }
        return;
    }
    for (key, zdata) in entries {
        if vm.current_exception.is_some() {
            return;
        }
        let zdata = zdata.deref();
        let is_table = match &zdata {
            Value::Array(_) | Value::Generator(_) => true,
            Value::Object(_) => !hbq_is_enum(vm, &zdata),
            _ => false,
        };
        if is_table {
            let mut new_prefix = Vec::new();
            if let Some(kp) = key_prefix {
                new_prefix.extend_from_slice(kp);
            }
            match &key {
                HbqKey::Str(name) => hbq_url_encode(&mut new_prefix, name, raw),
                HbqKey::Int(idx) => {
                    if let Some(np) = num_prefix {
                        new_prefix.extend_from_slice(np);
                    }
                    new_prefix.extend_from_slice(idx.to_string().as_bytes());
                }
            }
            new_prefix.extend_from_slice(if key_prefix.is_some() {
                b"%5D%5B"
            } else {
                b"%5B"
            });
            HBQ_ACTIVE.with(|a| a.borrow_mut().push(ptr));
            hbq_encode_hash(
                vm,
                out,
                &zdata,
                None,
                Some(&new_prefix),
                arg_sep,
                enc_type,
                depth + 1,
            );
            HBQ_ACTIVE.with(|a| {
                a.borrow_mut().pop();
            });
            continue;
        }
        if matches!(zdata, Value::Null | Value::Undef | Value::Resource(_)) {
            continue;
        }
        // php_url_encode_scalar()
        let form = match out {
            Some(form) => {
                form.extend_from_slice(arg_sep);
                form
            }
            None => out.insert(Vec::new()),
        };
        if let Some(kp) = key_prefix {
            form.extend_from_slice(kp);
        }
        match &key {
            HbqKey::Str(name) => hbq_url_encode(form, name, raw),
            HbqKey::Int(idx) => {
                if let Some(np) = num_prefix {
                    form.extend_from_slice(np);
                }
                form.extend_from_slice(idx.to_string().as_bytes());
            }
        }
        if key_prefix.is_some() {
            form.extend_from_slice(b"%5D");
        }
        form.push(b'=');
        let mut scalar = zdata;
        if let Value::Object(o) = &scalar {
            let lower = o.borrow().class_name.to_ascii_lowercase();
            let (backed, name) = vm
                .classes
                .get(&lower)
                .map(|c| (c.enum_backing_type.is_some(), c.name.clone()))
                .unwrap_or((false, Vec::new()));
            if !backed {
                let msg = format!(
                    "Unbacked enum {} cannot be converted to a string",
                    goro_core::string::msg_str(&name)
                );
                let exc = vm.create_exception(b"ValueError", &msg, 0);
                vm.current_exception = Some(exc);
                return;
            }
            let v = o.borrow().get_property(b"value");
            scalar = v;
        }
        match &scalar {
            Value::String(s) => hbq_url_encode(form, s.as_bytes(), raw),
            Value::Long(n) => form.extend_from_slice(n.to_string().as_bytes()),
            Value::Double(d) => {
                let s = scalar.to_php_string();
                hbq_url_encode(form, s.as_bytes(), raw);
                // zend_double_to_str()
                if d.is_nan() {
                    vm.emit_warning_at(
                        "unexpected NAN value was coerced to string",
                        vm.current_line,
                    );
                }
            }
            Value::False => form.push(b'0'),
            Value::True => form.push(b'1'),
            _ => {}
        }
    }
}

fn http_build_query_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // (zpp checked the arguments: array|object, string, ?string, int)
    let data = args.first().map(|v| v.deref()).unwrap_or(Value::Null);
    if let (true, Value::Object(obj)) = (hbq_is_enum(vm, &data), &data) {
        let name = obj.borrow().class_name.clone();
        let msg = format!(
            "http_build_query(): Argument #1 ($data) must not be an enum, {} given",
            goro_core::string::msg_str(&name)
        );
        let exc = vm.create_exception(b"TypeError", &msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: "Uncaught TypeError".to_string(),
            line: 0,
        });
    }
    let num_prefix = match args.get(1).map(|v| v.deref()) {
        Some(Value::Null) | Some(Value::Undef) | None => None,
        Some(v) => Some(v.to_php_string()),
    };
    let arg_sep = match args.get(2).map(|v| v.deref()) {
        Some(Value::Null) | Some(Value::Undef) | None => {
            // PG(arg_separator).output, "&" when empty
            match vm.constants.get(b"arg_separator.output".as_slice()) {
                Some(v) if !v.to_php_string().is_empty() => v.to_php_string(),
                _ => PhpString::from_bytes(b"&"),
            }
        }
        Some(v) => v.to_php_string(),
    };
    let enc_type = args.get(3).map(|v| v.to_long()).unwrap_or(1); // PHP_QUERY_RFC1738
    let mut out = None;
    hbq_encode_hash(
        vm,
        &mut out,
        &data,
        num_prefix.as_ref().map(|p| p.as_bytes()),
        None,
        arg_sep.as_bytes(),
        enc_type,
        0,
    );
    if vm.current_exception.is_some() {
        return Ok(Value::Null);
    }
    Ok(Value::String(PhpString::from_vec(out.unwrap_or_default())))
}

// JSON functions moved to goro-ext-json

// === File stubs ===
fn file_exists_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    if !vm.check_open_basedir("file_exists", &path.to_string_lossy()) {
        return Ok(Value::False);
    }
    Ok(if std::path::Path::new(&path.to_string_lossy()).exists() {
        Value::True
    } else {
        Value::False
    })
}
fn is_file_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    if !vm.check_open_basedir("is_file", &path.to_string_lossy()) {
        return Ok(Value::False);
    }
    Ok(if std::path::Path::new(&path.to_string_lossy()).is_file() {
        Value::True
    } else {
        Value::False
    })
}
/// Strip the file:// scheme prefix from a path. Handles file:///, file://,
/// and rejects file://host/path (returns None for unsupported host components).
fn strip_file_scheme(path: &str) -> Option<String> {
    if let Some(rest) = path.strip_prefix("file://") {
        // file:///path -> /path (empty host)
        // file://localhost/path -> /path  (localhost-only)
        // file://otherhost/path -> not supported
        if rest.starts_with('/') {
            return Some(rest.to_string());
        }
        if let Some(after_localhost) = rest.strip_prefix("localhost/") {
            return Some(format!("/{}", after_localhost));
        }
        // Just file://path -> path (treat as plain path)
        return Some(rest.to_string());
    }
    None
}

fn is_dir_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let path_str = path.to_string_lossy();
    let path_resolved = strip_file_scheme(&path_str).unwrap_or_else(|| path_str.to_string());
    if !vm.check_open_basedir("is_dir", &path_resolved) {
        return Ok(Value::False);
    }
    Ok(if std::path::Path::new(&path_resolved).is_dir() {
        Value::True
    } else {
        Value::False
    })
}
fn is_readable_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    #[cfg(unix)]
    {
        Ok(
            if check_access_bit(&path.to_string_lossy(), 0o400, 0o040, 0o004) {
                Value::True
            } else {
                Value::False
            },
        )
    }
    #[cfg(not(unix))]
    {
        Ok(if std::path::Path::new(&path.to_string_lossy()).exists() {
            Value::True
        } else {
            Value::False
        })
    }
}
fn is_writable_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    #[cfg(unix)]
    {
        Ok(
            if check_access_bit(&path.to_string_lossy(), 0o200, 0o020, 0o002) {
                Value::True
            } else {
                Value::False
            },
        )
    }
    #[cfg(not(unix))]
    {
        Ok(if std::path::Path::new(&path.to_string_lossy()).exists() {
            Value::True
        } else {
            Value::False
        })
    }
}

/// PHP-style file mode access check: returns true if the appropriate
/// permission bit (owner/group/other) is set for the current process.
#[cfg(unix)]
fn check_access_bit(path: &str, owner_bit: u32, group_bit: u32, other_bit: u32) -> bool {
    use std::os::unix::fs::MetadataExt;
    let m = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return false,
    };
    // Root is checked specially: PHP relies on the kernel access(2) syscall.
    // We approximate: euid 0 → r/w always allowed; x requires any x bit.
    let euid = unsafe { libc::geteuid() };
    let egid = unsafe { libc::getegid() };
    let mode = m.mode();
    if euid == 0 {
        // Root: for read/write any access; for execute requires any x bit.
        if owner_bit == 0o400 || owner_bit == 0o200 {
            return true;
        }
        return mode & 0o111 != 0;
    }
    let bit = if m.uid() == euid {
        owner_bit
    } else if m.gid() == egid {
        group_bit
    } else {
        // Could also be in supplementary groups; ignore for now.
        other_bit
    };
    mode & bit != 0
}
/// Lexically normalize a filesystem path, collapsing `.` and `..` segments
/// without touching the filesystem. This mirrors PHP's behavior where
/// `foo/bar/../baz` resolves to `foo/baz` even if `bar` doesn't exist.
/// Preserves leading `/`, but not symlinks.
pub(crate) fn normalize_path_lexical(p: &str) -> String {
    if p.is_empty() {
        return p.to_string();
    }
    let is_absolute = p.starts_with('/');
    let mut out: Vec<&str> = Vec::new();
    for seg in p.split('/') {
        match seg {
            "" => {}  // skip empty parts (caused by //)
            "." => {} // skip current
            ".." => {
                if let Some(last) = out.last() {
                    if *last != ".." {
                        out.pop();
                        continue;
                    }
                }
                if !is_absolute {
                    out.push("..");
                }
            }
            other => out.push(other),
        }
    }
    let joined = out.join("/");
    let result = if is_absolute {
        format!("/{}", joined)
    } else if joined.is_empty() {
        ".".to_string()
    } else {
        joined
    };
    // Preserve trailing slash if original had one (matters for directory dispatch).
    if p.ends_with('/') && !result.ends_with('/') {
        format!("{}/", result)
    } else {
        result
    }
}

/// Read a whole file the way file_get_contents()/file()/readfile() do: the
/// opened stream takes a resource id (plus the inner one of a zlib stream).
pub(crate) fn read_path_with_wrappers(path_str: &str) -> std::io::Result<Vec<u8>> {
    let r = read_path_with_wrappers_inner(path_str);
    if r.is_ok() {
        goro_core::value::alloc_resource_id();
    }
    r
}

fn read_path_with_wrappers_inner(path_str: &str) -> std::io::Result<Vec<u8>> {
    // Apply lexical normalization for plain filesystem paths.
    let normalized = if !path_str.contains("://") {
        normalize_path_lexical(path_str)
    } else {
        path_str.to_string()
    };
    std::fs::read(&normalized)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MetaTok {
    Eof,
    OpenTag,
    CloseTag,
    Slash,
    Equal,
    Space,
    Id,
    Str,
    Other,
}

/// php_meta_tags_data: the tokenizer state of get_meta_tags().
struct MetaTokenizer<'a> {
    data: &'a [u8],
    pos: usize,
    eof: bool,
    ulc: bool,
    lc: i32,
    in_meta: bool,
    token: Vec<u8>,
}

impl MetaTokenizer<'_> {
    /// php_stream_getc(): EOF (-1) once the data is exhausted, which also
    /// sets the stream's EOF flag.
    fn getc(&mut self) -> i32 {
        match self.data.get(self.pos) {
            Some(&c) => {
                self.pos += 1;
                c as i32
            }
            None => {
                self.eof = true;
                -1
            }
        }
    }

    /// php_next_meta_token()
    fn next(&mut self) -> MetaTok {
        const BUFSIZE: usize = 256;
        let mut ch: i32 = 0;
        self.token.clear();
        loop {
            if !self.ulc {
                if self.eof {
                    break;
                }
                ch = self.getc();
                if ch == 0 {
                    break;
                }
            }
            if self.eof {
                break;
            }
            if self.ulc {
                ch = self.lc;
                self.ulc = false;
            }
            match ch {
                0x3c => return MetaTok::OpenTag,
                0x3e => return MetaTok::CloseTag,
                0x3d => return MetaTok::Equal,
                0x2f => return MetaTok::Slash,
                0x27 | 0x22 => {
                    let compliment = ch;
                    let mut buf = Vec::new();
                    while !self.eof {
                        ch = self.getc();
                        if ch == 0 || ch == compliment || ch == 0x3c || ch == 0x3e {
                            break;
                        }
                        buf.push(ch as u8);
                        if buf.len() == BUFSIZE {
                            break;
                        }
                    }
                    if ch == 0x3c || ch == 0x3e {
                        // Was just an apostrophe
                        self.ulc = true;
                        self.lc = ch;
                    }
                    if self.in_meta {
                        self.token = buf;
                    }
                    return MetaTok::Str;
                }
                0x0a | 0x0d | 0x09 => {}
                0x20 => return MetaTok::Space,
                _ => {
                    let is_alnum =
                        |c: i32| (0..128).contains(&c) && (c as u8).is_ascii_alphanumeric();
                    if is_alnum(ch) {
                        let mut buf = vec![ch as u8];
                        while !self.eof {
                            ch = self.getc();
                            if ch == 0 || !(is_alnum(ch) || b"-_.:".contains(&(ch as u8)) && ch > 0)
                            {
                                break;
                            }
                            buf.push(ch as u8);
                            if buf.len() == BUFSIZE {
                                break;
                            }
                        }
                        let is_alpha = (0..128).contains(&ch) && (ch as u8).is_ascii_alphabetic();
                        if !is_alpha && ch != 0x2d {
                            self.ulc = true;
                            self.lc = ch;
                        }
                        self.token = buf;
                        return MetaTok::Id;
                    }
                    return MetaTok::Other;
                }
            }
        }
        MetaTok::Eof
    }
}

/// get_meta_tags(): the content attributes of the <meta> tags of a file.
fn get_meta_tags_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let filename = args.first().cloned().unwrap_or(Value::Null);
    let use_include_path = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
    let contents = file_get_contents_fn(
        vm,
        &[
            filename,
            if use_include_path {
                Value::True
            } else {
                Value::False
            },
        ],
    )?;
    let Value::String(contents) = contents else {
        return Ok(Value::False);
    };
    const UNSAFE: &[u8] = b".\\+*?[^]$() ";
    let sanitize = |n: &[u8]| -> Vec<u8> {
        n.iter()
            .map(|&c| if UNSAFE.contains(&c) { b'_' } else { c })
            .collect()
    };
    let mut md = MetaTokenizer {
        data: contents.as_bytes(),
        pos: 0,
        eof: false,
        ulc: false,
        lc: 0,
        in_meta: false,
        token: Vec::new(),
    };
    let mut result = PhpArray::new();
    let (mut in_tag, mut done) = (false, false);
    let (mut looking_for_val, mut have_name, mut have_content) = (false, false, false);
    let (mut saw_name, mut saw_content) = (false, false);
    let mut name: Option<Vec<u8>> = None;
    let mut value: Option<Vec<u8>> = None;
    let mut tok_last = MetaTok::Eof;
    while !done {
        let tok = md.next();
        if tok == MetaTok::Eof {
            break;
        }
        let tdata = md.token.clone();
        if tok == MetaTok::Id {
            if tok_last == MetaTok::OpenTag {
                md.in_meta = tdata.eq_ignore_ascii_case(b"meta");
            } else if tok_last == MetaTok::Slash && in_tag {
                if tdata.eq_ignore_ascii_case(b"head") {
                    done = true;
                }
            } else if tok_last == MetaTok::Equal && looking_for_val {
                if saw_name {
                    name = Some(sanitize(&tdata));
                    have_name = true;
                } else if saw_content {
                    value = Some(tdata);
                    have_content = true;
                }
                looking_for_val = false;
            } else if md.in_meta {
                if tdata.eq_ignore_ascii_case(b"name") {
                    saw_name = true;
                    saw_content = false;
                    looking_for_val = true;
                } else if tdata.eq_ignore_ascii_case(b"content") {
                    saw_name = false;
                    saw_content = true;
                    looking_for_val = true;
                }
            }
        } else if tok == MetaTok::Str && tok_last == MetaTok::Equal && looking_for_val {
            if saw_name {
                name = Some(sanitize(&tdata));
                have_name = true;
            } else if saw_content {
                value = Some(tdata);
                have_content = true;
            }
            looking_for_val = false;
        } else if tok == MetaTok::OpenTag {
            if looking_for_val {
                looking_for_val = false;
                have_name = false;
                saw_name = false;
                have_content = false;
                saw_content = false;
            }
            in_tag = true;
        } else if tok == MetaTok::CloseTag {
            if have_name {
                // C string semantics: the name ends at its first NUL.
                let mut n = name.take().unwrap_or_default();
                if let Some(p) = n.iter().position(|&c| c == 0) {
                    n.truncate(p);
                }
                n.make_ascii_lowercase();
                let v = if have_content {
                    value.take().unwrap_or_default()
                } else {
                    Vec::new()
                };
                result.set(
                    Vm::value_to_array_key(Value::String(PhpString::from_vec(n))),
                    Value::String(PhpString::from_vec(v)),
                );
            }
            name = None;
            value = None;
            in_tag = false;
            looking_for_val = false;
            have_name = false;
            saw_name = false;
            have_content = false;
            saw_content = false;
            md.in_meta = false;
        }
        tok_last = tok;
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

/// Z_PARAM_PATH: a ValueError for a path with NUL bytes.
fn path_arg_nul_error(
    vm: &mut Vm,
    path: &[u8],
    func: &str,
    argn: u32,
    name: &str,
) -> Option<VmError> {
    if !path.contains(&0) {
        return None;
    }
    let msg = format!(
        "{}(): Argument #{} (${}) must not contain any null bytes",
        func, argn, name
    );
    let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
    vm.current_exception = Some(exc);
    Some(VmError {
        message: msg,
        line: vm.current_line,
    })
}

fn file_get_contents_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    if let Some(e) = path_arg_nul_error(vm, path.as_bytes(), "file_get_contents", 1, "filename") {
        return Err(e);
    }
    // Empty filename → ValueError, matching PHP's behavior.
    if path.as_bytes().is_empty() {
        let msg = "Path must not be empty";
        let exc = vm.create_exception(b"ValueError", msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg.to_string(),
            line: vm.current_line,
        });
    }
    let use_include_path = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
    let _context = args.get(2);
    let context = _context;
    let offset = args.get(3).map(|v| v.to_long()).unwrap_or(0);
    let length = args.get(4).map(|v| v.to_long());

    // If use_include_path and path is relative + plain (no scheme), look up
    // through include_path entries, then fall back to the script's directory.
    let path = {
        let path_str = path.to_string_lossy();
        let resolved = resolve_with_include_path(vm, &path_str, use_include_path);
        if resolved == path_str {
            path
        } else {
            PhpString::from_string(resolved)
        }
    };

    // Validate length parameter
    if let Some(len) = length {
        if len < 0 {
            let msg =
                "file_get_contents(): Argument #5 ($length) must be greater than or equal to 0";
            let exc = vm.create_exception(b"ValueError", msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg.to_string(),
                line: vm.current_line,
            });
        }
    }

    let path_str = path.to_string_lossy();
    // data: and data:// URIs: route through fopen (which already handles
    // base64-decoding, urldecoding, etc.) to avoid duplicating the logic.
    // Reject datafoo: and similar malformed schemes.
    if path.as_bytes().starts_with(b"data:") {
        let inner_args = vec![
            Value::String(path.clone()),
            Value::String(PhpString::from_bytes(b"rb")),
        ];
        let fp = fopen_fn(vm, &inner_args)?;
        let fid = match fp {
            Value::Resource(n) => n.id(),
            _ => return Ok(Value::False),
        };
        let get_args = if let Some(length) = length {
            vec![
                Value::resource(fid),
                Value::Long(length),
                Value::Long(offset),
            ]
        } else {
            vec![Value::resource(fid), Value::Long(-1), Value::Long(offset)]
        };
        let res = stream_get_contents_fn(vm, &get_args)?;
        let _ = fclose_fn(vm, &[Value::resource(fid)]);
        return Ok(res);
    }
    // php://filter routes through fopen so the filter chain is applied.
    // (Dispatch through the registered functions: the inner resource may be
    // served by a userspace stream wrapper.)
    let lower_path = path_str.to_ascii_lowercase();
    if lower_path.starts_with("php://filter/")
        || matches!(
            lower_path.as_str(),
            "php://stdin" | "php://stdout" | "php://stderr" | "php://memory"
        )
        || lower_path.starts_with("php://temp")
    {
        let mut inner_args = vec![
            Value::String(PhpString::from_string(path_str.to_string())),
            Value::String(PhpString::from_bytes(b"r")),
            Value::False,
        ];
        if let Some(ctx) = args.get(2) {
            inner_args.push(ctx.clone());
        }
        let fp = call_registered(vm, b"fopen", fopen_fn, &inner_args)?;
        let fid = match fp {
            Value::Resource(n) => n.id(),
            _ => return Ok(Value::False),
        };
        let get_args = if let Some(length) = length {
            vec![
                Value::resource(fid),
                Value::Long(length),
                Value::Long(offset),
            ]
        } else {
            vec![Value::resource(fid), Value::Long(-1), Value::Long(offset)]
        };
        let res = call_registered(
            vm,
            b"stream_get_contents",
            stream_get_contents_fn,
            &get_args,
        )?;
        let _ = call_registered(vm, b"fclose", fclose_fn, &[Value::resource(fid)]);
        return Ok(res);
    }
    // php://input - request body for HTTP requests served by the built-in CLI server.
    if path_str == "php://input" {
        let body = get_request_body();
        return Ok(Value::String(PhpString::from_vec(body)));
    }
    // http:// and https:// (http_fopen_wrapper.c)
    let lower_scheme =
        String::from_utf8_lossy(&path_str.as_bytes()[..path_str.len().min(8)]).to_ascii_lowercase();
    if lower_scheme.starts_with("http://") || lower_scheme.starts_with("https://") {
        let body = http_wrapper::http_open(vm, "file_get_contents", &path_str, context);
        if vm.current_exception.is_some() {
            return Err(VmError {
                message: "Uncaught exception".into(),
                line: vm.current_line,
            });
        }
        return Ok(match body {
            Some(b) => {
                let start = if offset >= 0 {
                    (offset as usize).min(b.len())
                } else {
                    b.len().saturating_sub((-offset) as usize)
                };
                let s = &b[start..];
                let s = match length {
                    Some(l) => &s[..s.len().min(l as usize)],
                    None => s,
                };
                Value::String(PhpString::from_vec(s.to_vec()))
            }
            None => Value::False,
        });
    }
    if !path_str.starts_with("php://")
        && !path_str.starts_with("data://")
        && !path_str.starts_with("compress.zlib://")
        && !path_str.starts_with("compress.bzip2://")
    {
        if !vm.check_open_basedir("file_get_contents", &path_str) {
            return Ok(Value::False);
        }
    }
    // If path is an existing directory, emit a notice rather than failing
    // silently. PHP says: "Read of N bytes failed with errno=21 Is a directory".
    if !path_str.contains("://") && std::path::Path::new(&*path_str).is_dir() {
        // The directory opens as a plain-file stream.
        goro_core::value::alloc_resource_id();
        let line = vm.current_line;
        vm.emit_notice_at(
            "file_get_contents(): Read of 8192 bytes failed with errno=21 Is a directory",
            line,
        );
        return Ok(Value::String(PhpString::empty()));
    }
    match read_path_with_wrappers(&path_str) {
        Ok(data) => {
            // Negative offset: count from end of data (PHP semantics).
            let start = if offset >= 0 {
                offset as usize
            } else {
                let from_end = (-offset) as usize;
                data.len().saturating_sub(from_end)
            };
            if start >= data.len() && start > 0 {
                return Ok(Value::String(PhpString::empty()));
            }
            let slice = &data[start.min(data.len())..];
            let result = if let Some(len) = length {
                &slice[..slice.len().min(len as usize)]
            } else {
                slice
            };
            Ok(Value::String(PhpString::from_vec(result.to_vec())))
        }
        Err(_) => {
            vm.emit_warning(&format!(
                "file_get_contents({}): Failed to open stream: No such file or directory",
                path_str
            ));
            Ok(Value::False)
        }
    }
}

fn file_put_contents_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    if let Some(e) = path_arg_nul_error(vm, path.as_bytes(), "file_put_contents", 1, "filename") {
        return Err(e);
    }
    // php_stream_open_wrapper_ex(): an empty path is a ValueError.
    if path.as_bytes().is_empty() {
        let msg = "Path must not be empty";
        let exc = vm.create_exception(b"ValueError", msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg.to_string(),
            line: vm.current_line,
        });
    }
    if !vm.check_open_basedir("file_put_contents", &path.to_string_lossy()) {
        return Ok(Value::False);
    }
    // data: streams have no write op.
    if path.as_bytes().starts_with(b"data:") {
        let fp = fopen_fn(
            vm,
            &[
                Value::String(path.clone()),
                Value::String(PhpString::from_bytes(b"wb")),
            ],
        )?;
        if let Value::Resource(fid) = fp {
            let _ = fclose_fn(vm, &[Value::resource(&fid)]);
            let line = vm.current_line;
            vm.emit_notice_at("file_put_contents(): Stream is not writable", line);
        }
        return Ok(Value::False);
    }
    let data_val = args.get(1).unwrap_or(&Value::Null);
    // If $data is a resource, it should be a stream resource.
    if let Value::Resource(rid) = data_val {
        let is_open_stream = FILE_HANDLES.with(|h| h.borrow().contains_key(&rid.id()));
        let rtype = goro_core::value::resource_type_of(rid.id());
        let is_stream = is_open_stream || rtype.as_deref() == Some(b"stream".as_ref());
        if !is_stream {
            let msg =
                "file_put_contents(): supplied resource is not a valid stream resource".to_string();
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
    }
    // If $context (arg 3) is a resource, it should be a valid Stream-Context.
    if let Some(Value::Resource(rid)) = args.get(3) {
        let bytes = goro_core::value::resource_type_of(rid.id());
        if bytes.as_deref() != Some(b"stream-context".as_ref()) {
            let msg =
                "file_put_contents(): supplied resource is not a valid Stream-Context resource"
                    .to_string();
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
    }
    let flags = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    let append = flags & 8 != 0; // FILE_APPEND
    let lock_ex = flags & 2 != 0; // LOCK_EX

    // An object without __toString(): the file is still opened (and
    // truncated) before file_put_contents() fails.
    let mut fail_after_open = false;
    let data_bytes = match data_val {
        Value::Array(arr) => {
            let vals: Vec<Value> = arr.borrow().values().cloned().collect();
            let mut buf = Vec::new();
            for v in vals {
                // zval_get_tmp_string()
                let s = crate::strings::zval_get_string(vm, &v)?;
                buf.extend_from_slice(s.as_bytes());
            }
            buf
        }
        Value::Object(obj) => {
            // PHP attempts __toString. If absent, emit warning and return false.
            let class_name = obj.borrow().class_name.clone();
            let class_lower: Vec<u8> = class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
            let has_tostring = vm
                .classes
                .get(&class_lower)
                .map(|c| c.get_method(b"__tostring").is_some())
                .unwrap_or(false);
            if !has_tostring {
                let _ = class_name;
                fail_after_open = true;
                Vec::new()
            } else {
                match vm.call_object_method(data_val, b"__tostring", &[]) {
                    Some(result) => result.to_php_string().as_bytes().to_vec(),
                    None => return Ok(Value::False),
                }
            }
        }
        // A stream: what remains of it (php_stream_copy_to_stream_ex()).
        Value::Resource(_) => match vm.functions.get(b"stream_get_contents".as_slice()).copied() {
            Some(f) => match f(vm, std::slice::from_ref(data_val))? {
                Value::String(s) => s.as_bytes().to_vec(),
                _ => return Ok(Value::False),
            },
            None => return Ok(Value::False),
        },
        _ => data_val.to_php_string().as_bytes().to_vec(),
    };
    let path_str = path.to_string_lossy();

    // php:// streams (php://stdout, php://fd/N, php://output, ...): open the
    // stream and write to it (php_stream_open_wrapper_ex + php_stream_write).
    if path.as_bytes().len() > 6 && path.as_bytes()[..6].eq_ignore_ascii_case(b"php://") {
        let mode: &[u8] = if append { b"ab" } else { b"wb" };
        let fp = fopen_fn(
            vm,
            &[
                Value::String(path.clone()),
                Value::String(PhpString::from_bytes(mode)),
            ],
        )?;
        let Value::Resource(fid) = fp else {
            return Ok(Value::False);
        };
        let r = fwrite_fn(
            vm,
            &[
                Value::resource(&fid),
                Value::String(PhpString::from_vec(data_bytes)),
            ],
        );
        let _ = fclose_fn(vm, &[Value::resource(&fid)]);
        return r;
    }

    let p = std::path::Path::new(&*path_str);
    if let Some(parent) = p.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            vm.emit_warning(&format!(
                "file_put_contents({}): Failed to open stream: No such file or directory",
                path_str
            ));
            return Ok(Value::False);
        }
    }
    let result = {
        use std::io::Write;
        let file_result = if append {
            std::fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(&*path_str)
        } else {
            std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&*path_str)
        };
        if file_result.is_ok() {
            // The opened stream's resource id.
            goro_core::value::alloc_resource_id();
        }
        file_result.and_then(|mut f| {
            if fail_after_open {
                return Ok(usize::MAX);
            }
            if lock_ex {
                #[cfg(unix)]
                {
                    use std::os::unix::io::AsRawFd;
                    unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX) };
                }
            }
            f.write_all(&data_bytes).map(|_| data_bytes.len())
        })
    };
    match result {
        Ok(usize::MAX) => Ok(Value::False),
        Ok(len) => Ok(Value::Long(len as i64)),
        Err(e) => {
            vm.emit_warning(&format!(
                "file_put_contents({}): Failed to open stream: {}",
                path_str, e
            ));
            Ok(Value::False)
        }
    }
}
/// A realpath cache entry (realpath_cache_bucket).
struct RealpathEntry {
    path: Vec<u8>,
    realpath: Vec<u8>,
    is_dir: bool,
    expires: i64,
}

thread_local! {
    /// The realpath cache (zend_virtual_cwd.c): resolved paths are reused
    /// until they expire or clearstatcache(true) drops them.
    static REALPATH_CACHE: RefCell<Vec<RealpathEntry>> = const { RefCell::new(Vec::new()) };
    static REALPATH_SEEDED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn realpath_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn realpath_cache_ttl(vm: &Vm) -> i64 {
    vm.constants
        .get(b"realpath_cache_ttl".as_ref())
        .map(|v| v.to_long())
        .unwrap_or(120)
}

/// realpath_cache_find()
fn realpath_cache_find(path: &[u8], now: i64) -> Option<(Vec<u8>, bool)> {
    REALPATH_CACHE.with(|c| {
        let mut c = c.borrow_mut();
        c.retain(|e| e.expires >= now);
        c.iter()
            .find(|e| e.path == path)
            .map(|e| (e.realpath.clone(), e.is_dir))
    })
}

/// realpath_cache_add()
fn realpath_cache_add(path: &[u8], realpath: &[u8], is_dir: bool, expires: i64) {
    REALPATH_CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.iter().any(|e| e.path == path) {
            return;
        }
        c.push(RealpathEntry {
            path: path.to_vec(),
            realpath: realpath.to_vec(),
            is_dir,
            expires,
        });
    })
}

/// Resolve an absolute path through the realpath cache, caching every
/// leading component on the way like tsrm_realpath_r() does.
fn realpath_cached(vm: &Vm, abs: &[u8]) -> Option<Vec<u8>> {
    use std::os::unix::ffi::OsStrExt;
    let now = realpath_now();
    if let Some((rp, _)) = realpath_cache_find(abs, now) {
        return Some(rp);
    }
    let expires = now + realpath_cache_ttl(vm);
    // Lexically split into components; "." and ".." are resolved by the
    // filesystem lookup of each prefix.
    let mut prefix: Vec<u8> = Vec::new();
    let mut result = None;
    let comps: Vec<&[u8]> = abs
        .split(|&c| c == b'/')
        .filter(|c| !c.is_empty())
        .collect();
    if comps.is_empty() {
        return Some(b"/".to_vec());
    }
    for comp in comps {
        prefix.push(b'/');
        prefix.extend_from_slice(comp);
        if let Some((rp, _)) = realpath_cache_find(&prefix, now) {
            result = Some(rp);
            continue;
        }
        let p = std::path::Path::new(std::ffi::OsStr::from_bytes(&prefix));
        match std::fs::canonicalize(p) {
            Ok(rp) => {
                let rp = rp.as_os_str().as_bytes().to_vec();
                let is_dir = std::fs::metadata(p).map(|m| m.is_dir()).unwrap_or(false);
                realpath_cache_add(&prefix, &rp, is_dir, expires);
                result = Some(rp);
            }
            Err(_) => return None,
        }
    }
    result
}

/// The startup resolution of the main script (php_execute_script) fills
/// the cache with its directories.
fn realpath_cache_seed(vm: &Vm) {
    if REALPATH_SEEDED.with(|s| s.replace(true)) {
        return;
    }
    let script = vm.current_file.clone();
    if script.starts_with('/') {
        realpath_cached(vm, script.as_bytes());
    }
}

fn realpath_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let p = path.as_bytes();
    realpath_cache_seed(vm);
    let abs: Vec<u8> = if p.first() == Some(&b'/') {
        p.to_vec()
    } else {
        match std::env::current_dir() {
            Ok(cwd) if std::fs::metadata(&cwd).is_ok() => {
                use std::os::unix::ffi::OsStrExt;
                let mut a = cwd.as_os_str().as_bytes().to_vec();
                if !p.is_empty() {
                    a.push(b'/');
                    a.extend_from_slice(p);
                }
                a
            }
            _ => {
                // No usable working directory (bug #77484): the path is
                // resolved relative to nothing.
                if p.is_empty() {
                    return Ok(Value::False);
                }
                let norm: Vec<&[u8]> = p
                    .split(|&c| c == b'/')
                    .filter(|c| !c.is_empty() && *c != b".")
                    .collect();
                if norm.is_empty() {
                    return Ok(Value::String(PhpString::from_bytes(b".")));
                }
                return Ok(Value::False);
            }
        }
    };
    match realpath_cached(vm, &abs) {
        Some(rp) => Ok(Value::String(PhpString::from_vec(rp))),
        None => Ok(Value::False),
    }
}

fn realpath_cache_size_fn(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    realpath_cache_seed(vm);
    let now = realpath_now();
    let size: usize = REALPATH_CACHE.with(|c| {
        c.borrow()
            .iter()
            .filter(|e| e.expires >= now)
            .map(|e| {
                // sizeof(realpath_cache_bucket) + path (+ realpath when it
                // differs), NUL-terminated.
                let mut sz = 56 + e.path.len() + 1;
                if e.realpath != e.path {
                    sz += e.realpath.len() + 1;
                }
                sz
            })
            .sum()
    });
    Ok(Value::Long(size as i64))
}

fn realpath_cache_get_fn(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    realpath_cache_seed(vm);
    let now = realpath_now();
    let mut result = PhpArray::new();
    REALPATH_CACHE.with(|c| {
        for e in c.borrow().iter().filter(|e| e.expires >= now) {
            // realpath_cache_key(): the path's hash.
            let mut h: u64 = 0xcbf29ce484222325;
            for &b in &e.path {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
            let mut entry = PhpArray::new();
            entry.set(
                ArrayKey::String(PhpString::from_bytes(b"key")),
                if h > i64::MAX as u64 {
                    Value::Double(h as f64)
                } else {
                    Value::Long(h as i64)
                },
            );
            entry.set(
                ArrayKey::String(PhpString::from_bytes(b"is_dir")),
                if e.is_dir { Value::True } else { Value::False },
            );
            entry.set(
                ArrayKey::String(PhpString::from_bytes(b"realpath")),
                Value::String(PhpString::from_vec(e.realpath.clone())),
            );
            entry.set(
                ArrayKey::String(PhpString::from_bytes(b"expires")),
                Value::Long(e.expires),
            );
            result.set(
                ArrayKey::String(PhpString::from_vec(e.path.clone())),
                Value::Array(Rc::new(RefCell::new(entry))),
            );
        }
    });
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn getcwd_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    match std::env::current_dir() {
        Ok(p) => Ok(Value::String(PhpString::from_string(
            p.to_string_lossy().to_string(),
        ))),
        Err(_) => Ok(Value::False),
    }
}
fn chdir_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    if let Some(e) = path_arg_nul_error(vm, path.as_bytes(), "chdir", 1, "directory") {
        return Err(e);
    }
    match std::env::set_current_dir(&*path.to_string_lossy() as &str) {
        Ok(_) => Ok(Value::True),
        Err(e) => {
            let errno = e.raw_os_error().unwrap_or(0);
            vm.emit_warning(&format!(
                "chdir(): {} (errno {})",
                xp_socket::strerror(errno),
                errno
            ));
            Ok(Value::False)
        }
    }
}
/// chroot(string $directory): bool (CLI): chroot() then chdir("/").
fn chroot_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    if let Some(e) = path_arg_nul_error(vm, path.as_bytes(), "chroot", 1, "directory") {
        return Err(e);
    }
    let Ok(c) = std::ffi::CString::new(path.as_bytes().to_vec()) else {
        return Ok(Value::False);
    };
    let fail = |vm: &mut Vm| {
        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        vm.emit_warning(&format!(
            "chroot(): {} (errno {})",
            xp_socket::strerror(errno),
            errno
        ));
        Ok(Value::False)
    };
    // SAFETY: c is a valid NUL-terminated path.
    if unsafe { libc::chroot(c.as_ptr()) } != 0 {
        return fail(vm);
    }
    let _ = clearstatcache_fn(vm, &[Value::True]);
    if std::env::set_current_dir("/").is_err() {
        return fail(vm);
    }
    Ok(Value::True)
}
fn filesize_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let path_str = path.to_string_lossy();
    if path_str.is_empty() {
        return Ok(Value::False);
    }
    if !vm.check_open_basedir("filesize", &path_str) {
        return Ok(Value::False);
    }
    match std::fs::metadata(&*path_str as &str) {
        Ok(m) => Ok(Value::Long(m.len() as i64)),
        Err(_) => {
            vm.emit_warning(&format!("filesize(): stat failed for {}", path_str));
            Ok(Value::False)
        }
    }
}
fn touch_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let path_str = path.to_string_lossy();
    // PHP 8+: when $atime is an integer but $mtime is null, throw ValueError.
    if args.len() >= 3
        && matches!(args.get(1), Some(Value::Null))
        && !matches!(args.get(2), Some(Value::Null))
    {
        let msg =
            "touch(): Argument #2 ($mtime) cannot be null when argument #3 ($atime) is an integer"
                .to_string();
        let exc = vm.create_exception(b"ValueError", &msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    // Empty path: silently return false (matches PHP behavior)
    if path_str.is_empty() {
        return Ok(Value::False);
    }
    if !vm.check_open_basedir("touch", &path_str) {
        return Ok(Value::False);
    }
    let p = std::path::Path::new(&*path_str);
    if !p.exists() {
        if let Some(parent) = p.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                vm.emit_warning(&format!(
                    "touch(): Unable to create file {} because No such file or directory",
                    path_str
                ));
                return Ok(Value::False);
            }
        }
        if let Err(e) = std::fs::write(p, b"") {
            let msg = strip_os_error(&e.to_string());
            vm.emit_warning(&format!(
                "touch(): Unable to create file {} because {}",
                path_str, msg
            ));
            return Ok(Value::False);
        }
    }
    // Determine mtime/atime: if either provided, use them; else "now" so
    // ctime/mtime get bumped (matches PHP's expected behaviour).
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let mtime = args.get(1).map(|v| v.to_long()).unwrap_or(now);
    // PHP: $atime defaults to $mtime if only $mtime given.
    let atime = args.get(2).map(|v| v.to_long()).unwrap_or(mtime);

    // Use libc::utimes (atime, mtime) to set both.
    use std::ffi::CString;
    let c_path = match CString::new(path_str.as_bytes()) {
        Ok(c) => c,
        Err(_) => return Ok(Value::True),
    };
    let times = [
        libc::timeval {
            tv_sec: atime,
            tv_usec: 0,
        },
        libc::timeval {
            tv_sec: mtime,
            tv_usec: 0,
        },
    ];
    let _ = unsafe { libc::utimes(c_path.as_ptr(), times.as_ptr()) };
    Ok(Value::True)
}
fn is_link_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    Ok(
        if std::path::Path::new(&*path.to_string_lossy() as &str)
            .read_link()
            .is_ok()
        {
            Value::True
        } else {
            Value::False
        },
    )
}
fn stat_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let path_str = path.to_string_lossy();
    let res = stat_path(&path_str, false)?;
    // php_stat(): an empty filename fails silently.
    if matches!(res, Value::False) && !path_str.is_empty() {
        vm.emit_warning(&format!("stat(): stat failed for {}", path_str));
    }
    Ok(res)
}

fn stat_path(path_str: &str, is_lstat: bool) -> Result<Value, VmError> {
    #[cfg(unix)]
    {
        use std::ffi::CString;
        let c_path = CString::new(path_str.as_bytes()).unwrap_or_default();
        let mut stat_buf: libc::stat = unsafe { std::mem::zeroed() };
        let ret = if is_lstat {
            unsafe { libc::lstat(c_path.as_ptr(), &mut stat_buf) }
        } else {
            unsafe { libc::stat(c_path.as_ptr(), &mut stat_buf) }
        };
        if ret != 0 {
            return Ok(Value::False);
        }
        let mut result = PhpArray::new();
        let fields: [(i64, &[u8], i64); 13] = [
            (0, b"dev", stat_buf.st_dev as i64),
            (1, b"ino", stat_buf.st_ino as i64),
            (2, b"mode", stat_buf.st_mode as i64),
            (3, b"nlink", stat_buf.st_nlink as i64),
            (4, b"uid", stat_buf.st_uid as i64),
            (5, b"gid", stat_buf.st_gid as i64),
            (6, b"rdev", stat_buf.st_rdev as i64),
            (7, b"size", stat_buf.st_size as i64),
            (8, b"atime", stat_buf.st_atime as i64),
            (9, b"mtime", stat_buf.st_mtime as i64),
            (10, b"ctime", stat_buf.st_ctime as i64),
            (11, b"blksize", stat_buf.st_blksize as i64),
            (12, b"blocks", stat_buf.st_blocks as i64),
        ];
        // PHP stat returns all numeric keys first (0-12), then all string keys
        for (idx, _name, val) in &fields {
            result.set(ArrayKey::Int(*idx), Value::Long(*val));
        }
        for (_idx, name, val) in &fields {
            result.set(
                ArrayKey::String(PhpString::from_bytes(name)),
                Value::Long(*val),
            );
        }
        Ok(Value::Array(Rc::new(RefCell::new(result))))
    }
    #[cfg(not(unix))]
    {
        match std::fs::metadata(path_str) {
            Ok(m) => {
                let mut result = PhpArray::new();
                let size = m.len() as i64;
                let fields: [(i64, &[u8], i64); 13] = [
                    (0, b"dev", 0),
                    (1, b"ino", 0),
                    (2, b"mode", 0),
                    (3, b"nlink", 0),
                    (4, b"uid", 0),
                    (5, b"gid", 0),
                    (6, b"rdev", 0),
                    (7, b"size", size),
                    (8, b"atime", 0),
                    (9, b"mtime", 0),
                    (10, b"ctime", 0),
                    (11, b"blksize", -1),
                    (12, b"blocks", -1),
                ];
                // PHP stat returns all numeric keys first (0-12), then all string keys
                for (idx, _name, val) in &fields {
                    result.set(ArrayKey::Int(*idx), Value::Long(*val));
                }
                for (_idx, name, val) in &fields {
                    result.set(
                        ArrayKey::String(PhpString::from_bytes(name)),
                        Value::Long(*val),
                    );
                }
                Ok(Value::Array(Rc::new(RefCell::new(result))))
            }
            Err(_) => Ok(Value::False),
        }
    }
}
fn is_numeric_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        let msg = "is_numeric() expects exactly 1 argument, 0 given";
        let exc = vm.create_exception(b"ArgumentCountError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg.into(),
            line: vm.current_line,
        });
    }
    if args.len() > 1 {
        let msg = format!(
            "is_numeric() expects exactly 1 argument, {} given",
            args.len()
        );
        let exc = vm.create_exception(b"ArgumentCountError", &msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    let val = &args[0];
    Ok(match val {
        Value::Long(_) | Value::Double(_) => Value::True,
        Value::String(s) => {
            if php_is_numeric_string(s.as_bytes()) {
                Value::True
            } else {
                Value::False
            }
        }
        _ => Value::False,
    })
}

/// Check if a byte string is a valid PHP numeric string.
/// PHP allows leading and trailing whitespace.
fn php_is_numeric_string(s: &[u8]) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut i = 0;
    // Skip leading whitespace
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
        i += 1;
    }
    if i >= s.len() {
        return false;
    }
    // Optional sign
    if s[i] == b'+' || s[i] == b'-' {
        i += 1;
    }
    if i >= s.len() {
        return false;
    }
    let mut has_digits = false;
    while i < s.len() && s[i].is_ascii_digit() {
        has_digits = true;
        i += 1;
    }
    if i < s.len() && s[i] == b'.' {
        i += 1;
        while i < s.len() && s[i].is_ascii_digit() {
            has_digits = true;
            i += 1;
        }
    }
    if !has_digits {
        return false;
    }
    if i < s.len() && (s[i] == b'e' || s[i] == b'E') {
        i += 1;
        if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
            i += 1;
        }
        if i >= s.len() || !s[i].is_ascii_digit() {
            return false;
        }
        while i < s.len() && s[i].is_ascii_digit() {
            i += 1;
        }
    }
    // Skip trailing whitespace
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
        i += 1;
    }
    i == s.len()
}
fn dirname_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let s = path.to_string_lossy();
    let levels_raw = args.get(1).map(|v| v.to_long()).unwrap_or(1);
    if levels_raw < 1 {
        let msg = "dirname(): Argument #2 ($levels) must be greater than or equal to 1".to_string();
        let exc = vm.throw_type_error(msg.clone());
        if let Value::Object(obj) = &exc {
            obj.borrow_mut().class_name = b"ValueError".to_vec();
        }
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    let levels = levels_raw as usize;
    let mut result = s.to_string();
    // PHP: dirname("") returns ""
    if result.is_empty() {
        return Ok(Value::String(PhpString::from_string(String::new())));
    }
    for _ in 0..levels {
        // Once result is a terminal value ("/" or "."), further iterations
        // can't change it — short-circuit so we don't loop PHP_INT_MAX times.
        if result == "/" || result == "." {
            break;
        }
        // Strip trailing slashes (but not the root /)
        while result.len() > 1 && result.ends_with('/') {
            result.pop();
        }
        // Find last separator
        if let Some(pos) = result.rfind('/') {
            if pos == 0 {
                result = "/".to_string();
            } else {
                result.truncate(pos);
                // Strip trailing slashes from the result, preserving '/' if absolute.
                while result.len() > 1 && result.ends_with('/') {
                    result.pop();
                }
                if result.is_empty() {
                    result = "/".to_string();
                }
            }
        } else {
            result = ".".to_string();
        }
    }
    Ok(Value::String(PhpString::from_string(result)))
}

fn basename_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let suffix = args.get(1).map(|v| v.to_php_string());
    // Operate on raw bytes to preserve non-UTF-8 input (PHP is binary-safe).
    let bytes = path.as_bytes();
    // Strip trailing '/'
    let mut end = bytes.len();
    while end > 0 && bytes[end - 1] == b'/' {
        end -= 1;
    }
    let trimmed = &bytes[..end];
    let start = match trimmed.iter().rposition(|&b| b == b'/') {
        Some(pos) => pos + 1,
        None => 0,
    };
    let mut base: Vec<u8> = trimmed[start..].to_vec();
    if let Some(suf) = suffix {
        let suf_bytes = suf.as_bytes();
        if !suf_bytes.is_empty() && base.len() > suf_bytes.len() && base.ends_with(suf_bytes) {
            base.truncate(base.len() - suf_bytes.len());
        }
    }
    Ok(Value::String(PhpString::from_vec(base)))
}

/// PHP zend_dirname: returns the dirname portion (or empty string for len 0).
/// Operates on bytes; only '/' is a separator (Unix).
fn php_dirname_bytes(path: &[u8]) -> Vec<u8> {
    if path.is_empty() {
        return Vec::new();
    }
    let mut bytes = path.to_vec();
    let mut end = bytes.len() - 1;
    // Strip trailing slashes
    loop {
        if bytes[end] != b'/' {
            break;
        }
        if end == 0 {
            return vec![b'/'];
        }
        end -= 1;
    }
    // Strip filename (non-slash chars)
    loop {
        if bytes[end] == b'/' {
            break;
        }
        if end == 0 {
            return vec![b'.'];
        }
        end -= 1;
    }
    // Strip slashes that came before the filename
    loop {
        if bytes[end] != b'/' {
            break;
        }
        if end == 0 {
            return vec![b'/'];
        }
        end -= 1;
    }
    bytes.truncate(end + 1);
    bytes
}

/// PHP php_basename for Unix: strips trailing slashes, then returns the
/// substring from the last slash onward.
pub(crate) fn php_basename_bytes(path: &[u8]) -> Vec<u8> {
    if path.is_empty() {
        return Vec::new();
    }
    // Find last byte that is not a trailing slash.
    let mut end = path.len();
    while end > 0 && path[end - 1] == b'/' {
        end -= 1;
    }
    if end == 0 {
        return Vec::new();
    }
    // Find the previous '/' before end.
    let mut start = end;
    while start > 0 && path[start - 1] != b'/' {
        start -= 1;
    }
    path[start..end].to_vec()
}

fn pathinfo_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let opt = args.get(1).map(|v| v.to_long()).unwrap_or(15); // PATHINFO_ALL
    let bytes = path.as_bytes();

    let want_dirname = opt & 1 != 0;
    let want_basename = opt & 2 != 0;
    let want_extension = opt & 4 != 0;
    let want_filename = opt & 8 != 0;

    let dirname_bytes = if want_dirname {
        Some(php_dirname_bytes(bytes))
    } else {
        None
    };
    let basename_bytes = php_basename_bytes(bytes);

    let extension_bytes: Option<Vec<u8>> = if want_extension {
        basename_bytes
            .iter()
            .rposition(|&c| c == b'.')
            .map(|p| basename_bytes[p + 1..].to_vec())
    } else {
        None
    };
    let filename_bytes: Vec<u8> = if want_filename {
        match basename_bytes.iter().rposition(|&c| c == b'.') {
            Some(p) => basename_bytes[..p].to_vec(),
            None => basename_bytes.clone(),
        }
    } else {
        Vec::new()
    };

    // For PATHINFO_ALL (== 15) PHP returns the full array.
    if opt == 15 {
        let mut result = PhpArray::new();
        // dirname only added when non-empty.
        if let Some(d) = dirname_bytes.as_ref() {
            if !d.is_empty() {
                result.set(
                    ArrayKey::String(PhpString::from_bytes(b"dirname")),
                    Value::String(PhpString::from_bytes(d)),
                );
            }
        }
        result.set(
            ArrayKey::String(PhpString::from_bytes(b"basename")),
            Value::String(PhpString::from_bytes(&basename_bytes)),
        );
        // Extension only present when basename contains '.'.
        if let Some(e) = extension_bytes.as_ref() {
            result.set(
                ArrayKey::String(PhpString::from_bytes(b"extension")),
                Value::String(PhpString::from_bytes(e)),
            );
        }
        result.set(
            ArrayKey::String(PhpString::from_bytes(b"filename")),
            Value::String(PhpString::from_bytes(&filename_bytes)),
        );
        return Ok(Value::Array(Rc::new(RefCell::new(result))));
    }

    // For specific opt with single bit set, return string corresponding to that bit.
    // PHP behaviour: builds the array with only the requested keys, then returns the
    // first value if any, else empty string.
    let mut entries: Vec<Vec<u8>> = Vec::new();
    if want_dirname {
        if let Some(d) = dirname_bytes.as_ref() {
            if !d.is_empty() {
                entries.push(d.clone());
            }
        }
    }
    if want_basename {
        entries.push(basename_bytes.clone());
    }
    if want_extension {
        if let Some(e) = extension_bytes.as_ref() {
            entries.push(e.clone());
        }
    }
    if want_filename {
        entries.push(filename_bytes.clone());
    }
    if let Some(first) = entries.into_iter().next() {
        Ok(Value::String(PhpString::from_bytes(&first)))
    } else {
        Ok(Value::String(PhpString::from_bytes(b"")))
    }
}

// Additional commonly needed stubs
fn register_shutdown_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        return Ok(Value::Null);
    }
    let callback = args[0].clone();
    // Z_PARAM_FUNC
    let (ok, _, err) = check_callable(vm, &callback, false);
    if vm.current_exception.is_some() {
        return Err(VmError {
            message: "Uncaught exception".into(),
            line: vm.current_line,
        });
    }
    if !ok {
        return Err(throw_type_error_msg(
            vm,
            format!(
                "register_shutdown_function(): Argument #1 ($callback) must be a valid callback, {}",
                err.unwrap_or_default()
            ),
        ));
    }
    let extra_args: Vec<Value> = args[1..].to_vec();
    let callback = resolve_stored_callable(vm, callback);
    vm.shutdown_functions.push((callback, extra_args));
    Ok(Value::Null)
}

/// A callable stored for later (zend_fcall_info_cache): `self`/`parent`/
/// `static` are resolved now, and a non-static method named by class
/// keeps the calling `$this`.
fn resolve_stored_callable(vm: &mut Vm, callback: Value) -> Value {
    let (class, method) = match callback.deref() {
        Value::String(s) => {
            let b = s.as_bytes();
            match b.windows(2).position(|w| w == b"::") {
                Some(p) => (b[..p].to_vec(), b[p + 2..].to_vec()),
                None => return callback,
            }
        }
        Value::Array(a) => {
            let a = a.borrow();
            let mut it = a.values();
            match (it.next().map(|v| v.deref()), it.next().map(|v| v.deref())) {
                (Some(Value::String(c)), Some(Value::String(m)))
                    if !m.as_bytes().windows(2).any(|w| w == b"::") =>
                {
                    (c.as_bytes().to_vec(), m.as_bytes().to_vec())
                }
                _ => return callback,
            }
        }
        _ => return callback,
    };
    let r = vm.is_callable_pure(&callback, 0);
    if !r.ok || r.trampoline.is_some() {
        return callback;
    }
    let mk = |a: Value, m: &[u8]| {
        let mut arr = goro_core::array::PhpArray::new();
        arr.push(a);
        arr.push(Value::String(PhpString::from_vec(m.to_vec())));
        Value::Array(std::rc::Rc::new(std::cell::RefCell::new(arr)))
    };
    if let Some(obj) = r.object {
        return mk(obj, &method);
    }
    let kw = class.to_ascii_lowercase();
    if matches!(kw.as_slice(), b"self" | b"parent" | b"static") {
        if let Some(cl) = r.calling_scope {
            let name = vm.classes.get(&cl).map(|c| c.name.clone()).unwrap_or(cl);
            return mk(Value::String(PhpString::from_vec(name)), &method);
        }
    }
    callback
}
fn interface_exists_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    class_exists_impl(vm, args, |k| {
        k == goro_core::class_table::ClassKind::Interface
    })
}
fn trait_exists_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    class_exists_impl(vm, args, |k| k == goro_core::class_table::ClassKind::Trait)
}
fn enum_exists_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    class_exists_impl(vm, args, |k| k == goro_core::class_table::ClassKind::Enum)
}
/// exit()/die() called as a function (named or unpacked arguments,
/// `exit(...)`, or a callable string); the bare construct compiles to ExitOp.
fn exit_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.len() > 1 {
        let msg = format!("exit() expects at most 1 argument, {} given", args.len());
        let exc = vm.create_exception(b"ArgumentCountError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: format!("Uncaught ArgumentCountError: {}", msg),
            line: vm.current_line,
        });
    }
    let val = args.first().cloned().unwrap_or(Value::Long(0));
    let line = vm.current_line;
    match vm.exit_status_of(&val, line) {
        Ok(code) => {
            vm.exit_code = code;
            vm.exit_requested = true;
            Ok(Value::Null)
        }
        Err(msg) => Err(throw_type_error_msg(vm, msg)),
    }
}

fn gc_collect_fn(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(vm.gc_collect_cycles()))
}
fn gc_enabled_fn(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let enabled = goro_core::gc::ini_enabled(vm.constants.get(b"zend.enable_gc".as_ref()));
    Ok(if enabled { Value::True } else { Value::False })
}
fn gc_disable_fn(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    vm.constants
        .insert(b"zend.enable_gc".to_vec(), Value::Long(0));
    Ok(Value::Null)
}
fn gc_enable_fn(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    vm.constants
        .insert(b"zend.enable_gc".to_vec(), Value::Long(1));
    goro_core::gc::gc_enabled_changed();
    Ok(Value::Null)
}
fn get_object_vars_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Object(outer)) = args.first() {
        if let Some(f) = goro_core::vm::native_handlers(outer).and_then(|h| h.get_properties) {
            if let Some(a) = f(vm, outer, 1) {
                return Ok(Value::Array(std::rc::Rc::new(std::cell::RefCell::new(a))));
            }
        }
        // Trigger lazy init first so properties are visible.
        vm.maybe_run_lazy_init(outer);
        if vm.current_exception.is_some() {
            return Err(VmError {
                message: "Uncaught exception".to_string(),
                line: vm.current_line,
            });
        }
        // Redirect lazy proxy to real backing object.
        let obj_rc = vm.proxy_redirect(outer);
        let class_lower = obj_rc.borrow().class_name.to_ascii_lowercase();
        let table = vm.object_prop_table_info_order(&obj_rc);
        let mut arr = PhpArray::new();
        for e in table {
            let is_dynamic = e.slot.is_none();
            if is_dynamic && goro_core::foreach_obj::is_date_view_prop(&obj_rc.borrow(), &e.key) {
                continue;
            }
            if !vm.check_property_access(&class_lower, &e.key, is_dynamic) {
                continue;
            }
            let mut value = e.value;
            if let Some(slot) = &e.slot {
                if slot.has_get_hook {
                    // Hooked properties are read through their get hook.
                    match vm.call_get_hook_if_accessible(&obj_rc, &slot.name) {
                        Some(Ok(v)) => value = v,
                        Some(Err(err)) => return Err(err),
                        None => {
                            if slot.is_virtual || matches!(value, Value::Undef) {
                                continue;
                            }
                        }
                    }
                }
            }
            // A reference nothing else holds (but the slot `value` was
            // copied from) is copied as its value.
            let unref = match &value {
                Value::Reference(r) if Rc::strong_count(r) <= 2 => Some(r.borrow().clone()),
                _ => None,
            };
            if let Some(v) = unref {
                value = v;
            }
            let key = match &e.slot {
                Some(slot) => ArrayKey::String(PhpString::from_vec(slot.name.clone())),
                None => Vm::value_to_array_key(Value::String(PhpString::from_vec(e.key))),
            };
            if !arr.contains_key(&key) {
                arr.set(key, value);
            }
        }
        Ok(Value::Array(Rc::new(RefCell::new(arr))))
    } else {
        Ok(Value::False)
    }
}
fn get_class_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    // get_class() with no argument (or null) uses the current class scope
    if args.is_empty() {
        // Return the current class scope (the declaring class)
        if let Some(scope) = _vm.current_class_scope() {
            _vm.emit_deprecated("Calling get_class() without arguments is deprecated");
            if _vm.current_exception.is_some() {
                return Err(VmError {
                    message: _vm.uncaught_exception_message(),
                    line: _vm.current_line,
                });
            }
            // Find the original-case class name
            let scope_lower: Vec<u8> = scope.iter().map(|b| b.to_ascii_lowercase()).collect();
            let class_name = _vm
                .classes
                .get(&scope_lower)
                .map(|c| c.name.clone())
                .unwrap_or(scope);
            return Ok(Value::String(PhpString::from_vec(class_name)));
        }
        let msg = "get_class() without arguments must be called from within a class";
        let line = _vm.current_line;
        let exc = _vm.create_exception(b"Error", msg, line);
        _vm.current_exception = Some(exc);
        return Err(VmError {
            message: format!("Uncaught Error: {}", msg),
            line,
        });
    }
    if let Value::Object(obj) = val {
        let obj = obj.borrow();
        // Return the full class name including NUL byte for anonymous classes
        // PHP's get_class() returns the full internal name; callers use strstr(..., "\0", true)
        // to get the display part.
        Ok(Value::String(PhpString::from_vec(obj.class_name.clone())))
    } else if let Value::Generator(_) = val {
        Ok(Value::String(PhpString::from_bytes(b"Generator")))
    } else if let Value::String(s) = val {
        let b = s.as_bytes();
        if b.starts_with(b"__closure_")
            || b.starts_with(b"__arrow_")
            || b.starts_with(b"__bound_closure_")
            || b.starts_with(b"__closure_fcc_")
        {
            Ok(Value::String(PhpString::from_bytes(b"Closure")))
        } else {
            // PHP 8: TypeError for non-object
            let type_name = Vm::value_type_name(val);
            Err(throw_type_error_msg(
                _vm,
                format!(
                    "get_class(): Argument #1 ($object) must be of type object, {} given",
                    type_name
                ),
            ))
        }
    } else if let Value::Array(arr) = val {
        let arr = arr.borrow();
        if let Some(first) = arr.values().next() {
            if let Value::String(s) = first {
                let b = s.as_bytes();
                if b.starts_with(b"__closure_")
                    || b.starts_with(b"__arrow_")
                    || b.starts_with(b"__bound_closure_")
                    || b.starts_with(b"__closure_fcc_")
                {
                    return Ok(Value::String(PhpString::from_bytes(b"Closure")));
                }
            }
        }
        // PHP 8: TypeError for non-object
        let type_name = Vm::value_type_name(val);
        Err(throw_type_error_msg(
            _vm,
            format!(
                "get_class(): Argument #1 ($object) must be of type object, {} given",
                type_name
            ),
        ))
    } else {
        // PHP 8: TypeError for non-object
        let type_name = Vm::value_type_name(val);
        Err(throw_type_error_msg(
            _vm,
            format!(
                "get_class(): Argument #1 ($object) must be of type object, {} given",
                type_name
            ),
        ))
    }
}
fn memory_get_usage_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let cur = goro_core::value::memory_get_usage();
    let real = args.first().is_some_and(|v| v.is_truthy());
    Ok(Value::Long(if real {
        goro_core::value::memory_real_size(cur)
    } else {
        cur
    } as i64))
}
fn memory_get_peak_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let peak = goro_core::value::memory_get_peak();
    let real = args.first().is_some_and(|v| v.is_truthy());
    Ok(Value::Long(if real {
        goro_core::value::memory_real_size(peak)
    } else {
        peak
    } as i64))
}
fn memory_reset_peak_usage_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    goro_core::value::memory_reset_peak();
    Ok(Value::Null)
}
fn sleep_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let secs = args.first().unwrap_or(&Value::Null).to_long();
    if secs < 0 {
        return Err(throw_value_error_in(
            _vm,
            "sleep",
            args,
            "sleep(): Argument #1 ($seconds) must be greater than or equal to 0",
        ));
    }
    // Scripts rely on the full duration (e.g. outliving a server's
    // wait_timeout).
    _vm.flush_cli_output();
    // php_sleep(): a signal ends the sleep early; the unslept seconds are
    // returned.
    let mut left = 0;
    if secs > 0 {
        let req = libc::timespec {
            tv_sec: secs as libc::time_t,
            tv_nsec: 0,
        };
        if let Some(rem) = nanosleep_interruptible(_vm, req) {
            left = rem.tv_sec + if rem.tv_nsec >= 500_000_000 { 1 } else { 0 };
        }
    }
    Ok(Value::Long(left))
}
fn usleep_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let usecs = args.first().unwrap_or(&Value::Null).to_long();
    if usecs < 0 {
        return Err(throw_value_error_in(
            _vm,
            "usleep",
            args,
            "usleep(): Argument #1 ($microseconds) must be greater than or equal to 0",
        ));
    }
    _vm.flush_cli_output();
    if usecs > 0 {
        // Sleep the actual duration, capped at 2s (like sleep()).
        let actual = (usecs as u64).min(2_000_000);
        // usleep(): a signal ends it early.
        let req = libc::timespec {
            tv_sec: (actual / 1_000_000) as libc::time_t,
            tv_nsec: ((actual % 1_000_000) * 1000) as libc::c_long,
        };
        let _ = nanosleep_interruptible(_vm, req);
    }
    Ok(Value::Null)
}

/// nanosleep(): None when the whole time was slept, else what is left
/// (a signal ended it). A transparent interrupt (see
/// goro_core::value::TRANSPARENT_INTERRUPT) runs the interrupt hook and
/// the sleep goes on.
fn nanosleep_interruptible(vm: &mut Vm, mut req: libc::timespec) -> Option<libc::timespec> {
    use std::sync::atomic::Ordering;
    loop {
        let mut rem = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        if unsafe { libc::nanosleep(&req, &mut rem) } == 0 {
            return None;
        }
        if goro_core::value::TRANSPARENT_INTERRUPT.swap(false, Ordering::Relaxed) {
            if let Some(h) = vm.interrupt_hook {
                h(vm);
            }
            req = rem;
            continue;
        }
        return Some(rem);
    }
}
fn uniqid_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    use std::time::SystemTime;
    let prefix = args
        .first()
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_default();
    let more_entropy = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
    let t = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let base = format!(
        "{}{:08x}{:05x}",
        prefix,
        t.as_secs() as u32,
        t.subsec_micros()
    );
    if more_entropy {
        // PHP uses %.8F format which produces "N.XXXXXXXX" (10 chars for the random part)
        // e.g. "0.12345678" making total length = prefix_len + 13 + 10 = prefix_len + 23
        let random_part: u32 = (t.subsec_nanos() ^ 0xDEADBEEF) % 100_000_000;
        let random_val = random_part as f64 / 100_000_000.0;
        Ok(Value::String(PhpString::from_string(format!(
            "{}{:.8}",
            base, random_val
        ))))
    } else {
        Ok(Value::String(PhpString::from_string(base)))
    }
}
fn getenv_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    match args.first().map(|v| v.deref()) {
        None | Some(Value::Null) => {
            // All environment variables.
            let mut arr = PhpArray::new();
            for (k, v) in std::env::vars_os() {
                arr.set(
                    Vm::value_to_array_key(Value::String(PhpString::from_vec(k.into_vec()))),
                    Value::String(PhpString::from_vec(v.into_vec())),
                );
            }
            Ok(Value::Array(Rc::new(RefCell::new(arr))))
        }
        Some(name) => {
            let name = name.to_php_string();
            let key = std::ffi::OsStr::from_bytes(name.as_bytes());
            if name.as_bytes().is_empty()
                || name.as_bytes().contains(&b'=')
                || name.as_bytes().contains(&0)
            {
                return Ok(Value::False);
            }
            match std::env::var_os(key) {
                Some(val) => Ok(Value::String(PhpString::from_vec(val.into_vec()))),
                None => Ok(Value::False),
            }
        }
    }
}
fn putenv_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    use std::os::unix::ffi::OsStrExt;
    let setting = args.first().unwrap_or(&Value::Null).to_php_string();
    let b = setting.as_bytes();
    if b.is_empty() || b[0] == b'=' {
        return Err(throw_value_error_in(
            vm,
            "putenv",
            args,
            "putenv(): Argument #1 ($assignment) must have a valid syntax",
        ));
    }
    if b.contains(&0) {
        return Err(throw_value_error_in(
            vm,
            "putenv",
            args,
            "putenv(): Argument #1 ($assignment) must not contain any null bytes",
        ));
    }
    match b.iter().position(|&c| c == b'=') {
        Some(eq) => {
            let key = std::ffi::OsStr::from_bytes(&b[..eq]);
            let value = std::ffi::OsStr::from_bytes(&b[eq + 1..]);
            unsafe {
                std::env::set_var(key, value);
            }
        }
        None => unsafe {
            std::env::remove_var(std::ffi::OsStr::from_bytes(b));
        },
    }
    Ok(Value::True)
}
// spl_autoload_register, spl_autoload_functions, spl_autoload_unregister moved to goro-ext-spl

fn class_alias_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let original = args.first().unwrap_or(&Value::Null).to_php_string();
    let alias = args.get(1).unwrap_or(&Value::Null).to_php_string();
    // zend_lookup_class_ex() / zend_register_class_alias_ex(): a leading
    // backslash is not part of the name.
    let original_lower: Vec<u8> = original
        .as_bytes()
        .strip_prefix(b"\\")
        .unwrap_or(original.as_bytes())
        .to_ascii_lowercase();
    let alias_lower: Vec<u8> = alias
        .as_bytes()
        .strip_prefix(b"\\")
        .unwrap_or(alias.as_bytes())
        .to_ascii_lowercase();
    // Single-underscore alias was deprecated in 8.4.
    if alias.as_bytes() == b"_" {
        vm.emit_deprecated("Using \"_\" as a class alias is deprecated since 8.4");
    }
    // zend_assert_valid_class_name(lcname): reserved class names (e.g.
    // "array", "callable", "bool") cannot be used as aliases, in any
    // namespace; the message shows the lowercased name.
    let unqualified = alias_lower
        .rsplit(|&b| b == b'\\')
        .next()
        .unwrap_or(&alias_lower);
    if goro_core::compiler::is_reserved_class_name(unqualified) {
        let line = vm.current_line;
        let msg = format!(
            "Cannot use \"{}\" as a class alias as it is reserved",
            goro_core::string::msg_str(&alias_lower)
        );
        return Err(VmError { message: msg, line });
    }
    let autoload = args.get(2).map(|v| v.is_truthy()).unwrap_or(true);
    if autoload
        && !vm.classes.contains_key(&original_lower)
        && !vm.is_known_builtin_class(&original_lower)
    {
        vm.try_autoload_class(original.as_bytes());
    }
    if !vm.classes.contains_key(&original_lower) && !vm.is_known_builtin_class(&original_lower) {
        vm.emit_warning(&format!("Class \"{}\" not found", original.to_msg_string()));
        return Ok(Value::False);
    }
    if vm.classes.contains_key(&alias_lower) {
        // zend_class_redeclaration_error_ex(E_WARNING, alias, ce): the kind
        // and declaration site of the class being aliased.
        let original_class = vm.classes.get(&original_lower);
        let kind = match original_class {
            Some(o) if o.is_interface => "interface",
            Some(o) if o.is_trait => "trait",
            Some(o) if o.is_enum => "enum",
            _ => "class",
        };
        let location = match original_class {
            Some(o) if o.filename.is_some() && o.start_line > 0 => format!(
                " (previously declared in {}:{})",
                o.filename.clone().unwrap_or_default(),
                o.start_line
            ),
            _ => String::new(),
        };
        let msg = format!(
            "Cannot redeclare {} {}{}",
            kind,
            alias.to_msg_string(),
            location
        );
        vm.emit_warning(&msg);
        return Ok(Value::False);
    }
    if let Some(class) = vm.classes.get(&original_lower).cloned() {
        // Use the original class name (not alias) for the class entry
        let aliased_class = class;
        // `Alias::m()` reaches the methods as `Class::m()` does.
        for method in aliased_class.methods.values() {
            let mut key = alias_lower.clone();
            key.extend_from_slice(b"::");
            key.extend(method.name.iter().map(|b| b.to_ascii_lowercase()));
            let mut op = method.op_array.clone();
            if op.attributes.is_empty() && !method.attributes.is_empty() {
                std::rc::Rc::make_mut(&mut op).attributes = method.attributes.clone();
            }
            vm.user_functions.entry(key).or_insert(op);
        }
        // Keep original class name so get_class() returns the canonical name
        vm.classes.insert(alias_lower.clone(), aliased_class);
        // Register bidirectional alias mappings so instanceof works correctly
        vm.class_aliases
            .insert(alias_lower.clone(), original_lower.clone());
        vm.class_aliases.insert(original_lower, alias_lower);
        Ok(Value::True)
    } else if vm.is_known_builtin_class(&original_lower) {
        // Create a ClassEntry for the alias pointing to the builtin
        // (named as declared, not as the alias call spelled it)
        let canonical = vm
            .internal_class(&original_lower)
            .map(|c| c.name.as_bytes().to_vec())
            .unwrap_or_else(|| original.as_bytes().to_vec());
        let mut class = goro_core::object::ClassEntry::new(canonical);
        class.allow_dynamic_properties = true; // builtins generally allow this
        // An alias of an internal interface / trait / enum is one too.
        match vm.class_kind(&original_lower) {
            Some(goro_core::class_table::ClassKind::Interface) => class.is_interface = true,
            Some(goro_core::class_table::ClassKind::Trait) => class.is_trait = true,
            Some(goro_core::class_table::ClassKind::Enum) => class.is_enum = true,
            _ => {}
        }
        vm.classes.insert(alias_lower.clone(), class);
        vm.class_aliases
            .insert(alias_lower.clone(), original_lower.clone());
        vm.class_aliases.insert(original_lower, alias_lower);
        Ok(Value::True)
    } else {
        Ok(Value::False)
    }
}
fn is_a_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let (class_name, check_name) = match (args.first(), args.get(1)) {
        (Some(Value::Object(obj)), Some(Value::String(s))) => {
            (obj.borrow().class_name.clone(), s.as_bytes().to_vec())
        }
        (Some(Value::String(obj_class)), Some(Value::String(s))) => {
            // is_a with string class name + allow_string=true (3rd arg)
            let allow = args.get(2).map(|v| v.is_truthy()).unwrap_or(false);
            if allow {
                if !class_known_or_autoload(vm, obj_class.as_bytes()) {
                    return Ok(Value::False);
                }
                (obj_class.as_bytes().to_vec(), s.as_bytes().to_vec())
            } else {
                return Ok(Value::False);
            }
        }
        _ => return Ok(Value::False),
    };
    let result = class_is_a(vm, &class_name, &check_name);
    Ok(if result { Value::True } else { Value::False })
}

fn is_subclass_of_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let (class_name, check_name) = match (args.first(), args.get(1)) {
        (Some(Value::Object(obj)), Some(Value::String(s))) => {
            (obj.borrow().class_name.clone(), s.as_bytes().to_vec())
        }
        (Some(Value::String(obj_class)), Some(Value::String(s))) => {
            // is_subclass_of with string class name + allow_string=true (3rd arg, default true)
            let allow = args.get(2).map(|v| v.is_truthy()).unwrap_or(true);
            if allow {
                if !class_known_or_autoload(vm, obj_class.as_bytes()) {
                    return Ok(Value::False);
                }
                (obj_class.as_bytes().to_vec(), s.as_bytes().to_vec())
            } else {
                return Ok(Value::False);
            }
        }
        _ => return Ok(Value::False),
    };
    // is_subclass_of returns false if class_name == check_name (unlike is_a)
    // (an alias names the same class entry)
    let canonical = |n: &[u8]| -> Vec<u8> {
        let l = n.to_ascii_lowercase();
        vm.classes
            .get(&l)
            .map(|c| c.name.to_ascii_lowercase())
            .unwrap_or(l)
    };
    if canonical(&class_name) == canonical(&check_name) {
        return Ok(Value::False);
    }
    let result = class_is_a(vm, &class_name, &check_name);
    Ok(if result { Value::True } else { Value::False })
}

/// zend_lookup_class(): true if the class exists, autoloading it if needed.
fn class_known_or_autoload(vm: &mut Vm, name: &[u8]) -> bool {
    let lower = lower_bytes(name);
    if vm.classes.contains_key(&lower) || vm.is_known_builtin_class(&lower) {
        return true;
    }
    vm.try_autoload_class(name);
    vm.classes.contains_key(&lower)
}

/// Check if class_name is the same as or inherits from check_name
fn class_is_a(vm: &Vm, class_name: &[u8], check_name: &[u8]) -> bool {
    let class_lower: Vec<u8> = class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
    let check_lower: Vec<u8> = check_name.iter().map(|b| b.to_ascii_lowercase()).collect();

    // Direct match
    if class_lower == check_lower {
        return true;
    }

    // Check alias match
    if let Some(alias_target) = vm.class_aliases.get(&check_lower) {
        if class_lower == *alias_target {
            return true;
        }
    }

    // Check built-in exception hierarchy
    if goro_core::vm::is_builtin_subclass(&class_lower, &check_lower) {
        return true;
    }

    // Walk the parent chain
    let mut current = class_lower;
    loop {
        let Some(class_entry) = vm.classes.get(&current) else {
            // An internal ancestor: its hierarchy comes from the stubs.
            return goro_core::vm::is_builtin_subclass(&current, &check_lower);
        };
        // Check interfaces
        for iface in &class_entry.interfaces {
            let iface_lower: Vec<u8> = iface.iter().map(|b| b.to_ascii_lowercase()).collect();
            if iface_lower == check_lower {
                return true;
            }
            // Check parent interfaces too
            if class_is_a_interface(vm, &iface_lower, &check_lower) {
                return true;
            }
        }

        // Move to parent
        match &class_entry.parent {
            Some(parent) => {
                let parent_lower: Vec<u8> = parent.iter().map(|b| b.to_ascii_lowercase()).collect();
                // (a parent named by an alias of the class checked for)
                let canonical = |n: &[u8]| {
                    vm.classes
                        .get(n)
                        .map(|c| c.name.to_ascii_lowercase())
                        .unwrap_or_else(|| n.to_vec())
                };
                if parent_lower == check_lower
                    || canonical(&parent_lower) == canonical(&check_lower)
                {
                    return true;
                }
                current = parent_lower;
            }
            None => break,
        }
    }

    false
}

fn class_is_a_interface(vm: &Vm, iface_name: &[u8], check_name: &[u8]) -> bool {
    if !vm.classes.contains_key(iface_name) {
        return goro_core::vm::is_builtin_subclass(iface_name, check_name);
    }
    if let Some(ce) = vm.classes.get(iface_name) {
        for parent_iface in &ce.interfaces {
            let pi_lower: Vec<u8> = parent_iface
                .iter()
                .map(|b| b.to_ascii_lowercase())
                .collect();
            if pi_lower == check_name {
                return true;
            }
            if class_is_a_interface(vm, &pi_lower, check_name) {
                return true;
            }
        }
    }
    false
}
fn get_parent_class_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let class_name = match args.first().map(|v| v.deref()) {
        Some(Value::Object(obj)) => obj.borrow().class_name.clone(),
        Some(Value::String(s)) => {
            let lower = lower_bytes(s.as_bytes());
            if !vm.classes.contains_key(&lower) && !vm.is_known_builtin_class(&lower) {
                vm.try_autoload_class(s.as_bytes());
                if vm.current_exception.is_some() {
                    return Err(VmError {
                        message: "Uncaught exception".into(),
                        line: vm.current_line,
                    });
                }
                if !vm.classes.contains_key(&lower) && !vm.is_known_builtin_class(&lower) {
                    return Err(throw_type_error_msg(vm, "get_parent_class(): Argument #1 ($object_or_class) must be an object or a valid class name, string given".to_string()));
                }
            }
            s.as_bytes().to_vec()
        }
        None => {
            vm.emit_deprecated("Calling get_parent_class() without arguments is deprecated");
            match vm.current_class_scope() {
                Some(c) => c.to_vec(),
                None => return Ok(Value::False),
            }
        }
        Some(other) => {
            return Err(throw_type_error_msg(
                vm,
                format!(
                    "get_parent_class(): Argument #1 ($object_or_class) must be an object or a valid class name, {} given",
                    Vm::value_type_name(&other)
                ),
            ));
        }
    };
    let class_lower: Vec<u8> = class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
    if let Some(class) = vm.classes.get(&class_lower)
        && let Some(ref parent) = class.parent
    {
        return Ok(Value::String(PhpString::from_vec(parent.clone())));
    }
    // An internal class's parent.
    if let Some(parent) = goro_core::vm::get_builtin_parent(&class_lower) {
        return Ok(Value::String(PhpString::from_bytes(parent)));
    }
    Ok(Value::False)
}
fn get_called_class_fn(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // Return the current called class from late static binding stack
    if let Some(class_name) = vm.called_class_stack.last() {
        Ok(Value::String(PhpString::from_vec(class_name.clone())))
    } else {
        let msg = "get_called_class() must be called from within a class".to_string();
        let line = vm.current_line;
        let exc = vm.create_exception(b"Error", &msg, line);
        vm.current_exception = Some(exc);
        Err(VmError { message: msg, line })
    }
}
fn get_defined_vars_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
}
fn get_defined_functions_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.len() == 1 {
        vm.emit_deprecated(
            "get_defined_functions(): The $exclude_disabled parameter has no effect since PHP 8.0",
        );
    }
    let mut internal = PhpArray::new();
    for name in vm.functions.keys() {
        if name.as_slice() == goro_core::gc::DESTRUCTOR_FIBER_FN {
            continue;
        }
        internal.push(Value::String(PhpString::from_vec(name.clone())));
    }
    let mut user = PhpArray::new();
    for name in vm.user_functions.keys() {
        // Skip class methods (contain ::) and internal closures
        if !name.contains(&b':')
            && !name.starts_with(b"__closure_")
            && !name.starts_with(b"__arrow_")
        {
            user.push(Value::String(PhpString::from_vec(name.clone())));
        }
    }
    let mut result = PhpArray::new();
    result.set(
        goro_core::array::ArrayKey::String(PhpString::from_bytes(b"internal")),
        Value::Array(Rc::new(RefCell::new(internal))),
    );
    result.set(
        goro_core::array::ArrayKey::String(PhpString::from_bytes(b"user")),
        Value::Array(Rc::new(RefCell::new(user))),
    );
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}
fn array_first_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let arr = arr.borrow();
        Ok(arr
            .iter()
            .next()
            .map(|(_, v)| v.clone())
            .unwrap_or(Value::Null))
    } else {
        Ok(Value::Null)
    }
}
fn array_last_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let arr = arr.borrow();
        Ok(arr
            .iter()
            .last()
            .map(|(_, v)| v.clone())
            .unwrap_or(Value::Null))
    } else {
        Ok(Value::Null)
    }
}

fn array_change_key_case_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        // Check type of case argument - must be int-like
        if let Some(case_val) = args.get(1) {
            if matches!(case_val, Value::Array(_) | Value::Object(_)) {
                let type_name = match case_val {
                    Value::Array(_) => "array",
                    Value::Object(_) => "object",
                    _ => "unknown",
                };
                let msg = format!(
                    "array_change_key_case(): Argument #2 ($case) must be of type int, {} given",
                    type_name
                );
                let exc = vm.throw_type_error(msg.clone());
                vm.current_exception = Some(exc);
                return Err(VmError {
                    message: msg,
                    line: vm.current_line,
                });
            }
            // For string values, throw TypeError too
            if let Value::String(s) = case_val {
                if s.to_string_lossy().parse::<i64>().is_err() {
                    let msg = "array_change_key_case(): Argument #2 ($case) must be of type int, string given".to_string();
                    let exc = vm.throw_type_error(msg.clone());
                    vm.current_exception = Some(exc);
                    return Err(VmError {
                        message: msg,
                        line: vm.current_line,
                    });
                }
            }
        }
        let case = args.get(1).map(|v| v.to_long()).unwrap_or(0); // 0=lower, 1=upper
        let arr = arr.borrow();
        let mut result = PhpArray::new();
        for (key, val) in arr.iter() {
            let new_key = match key {
                goro_core::array::ArrayKey::String(s) => {
                    let bytes = s.as_bytes();
                    let transformed: Vec<u8> = if case == 0 {
                        bytes.iter().map(|b| b.to_ascii_lowercase()).collect()
                    } else {
                        bytes.iter().map(|b| b.to_ascii_uppercase()).collect()
                    };
                    goro_core::array::ArrayKey::String(PhpString::from_vec(transformed))
                }
                other => other.clone(),
            };
            result.set(new_key, val.clone());
        }
        Ok(Value::Array(Rc::new(RefCell::new(result))))
    } else {
        Ok(Value::False)
    }
}

fn array_multisort_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        let msg = "array_multisort() expects at least 1 argument, 0 given";
        let exc = vm.create_exception(b"ArgumentCountError", msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Ok(Value::Null);
    }
    let throw = |vm: &mut Vm, class: &[u8], msg: String| -> Result<Value, VmError> {
        let exc = vm.create_exception(class, &msg, vm.current_line);
        vm.current_exception = Some(exc);
        Ok(Value::Null)
    };
    // (array, sort_type, descending)
    let mut arrays: Vec<(Rc<RefCell<PhpArray>>, i64, bool)> = Vec::new();
    let mut order_allowed = false;
    let mut type_allowed = false;
    let mut sort_order_desc = false;
    let mut sort_type: i64 = SORT_REGULAR;
    for (i, arg) in args.iter().enumerate() {
        let v = arg.deref();
        match &v {
            Value::Array(arr) => {
                if let Some(last) = arrays.last_mut() {
                    last.1 = sort_type;
                    last.2 = sort_order_desc;
                    sort_order_desc = false;
                    sort_type = SORT_REGULAR;
                }
                arrays.push((arr.clone(), SORT_REGULAR, false));
                order_allowed = true;
                type_allowed = true;
            }
            Value::Long(n) => {
                let fname = if i == 0 {
                    "Argument #1 ($array)".to_string()
                } else {
                    format!("Argument #{}", i + 1)
                };
                match n & !SORT_FLAG_CASE {
                    3 | 4 => {
                        if order_allowed {
                            sort_order_desc = *n == 3;
                            order_allowed = false;
                        } else {
                            return throw(
                                vm,
                                b"TypeError",
                                format!(
                                    "array_multisort(): {} must be an array or a sort flag that has not already been specified",
                                    fname
                                ),
                            );
                        }
                    }
                    0 | 1 | 2 | 5 | 6 => {
                        if type_allowed {
                            sort_type = *n;
                            type_allowed = false;
                        } else {
                            return throw(
                                vm,
                                b"TypeError",
                                format!(
                                    "array_multisort(): {} must be an array or a sort flag that has not already been specified",
                                    fname
                                ),
                            );
                        }
                    }
                    _ => {
                        return throw(
                            vm,
                            b"ValueError",
                            format!("array_multisort(): {} must be a valid sort flag", fname),
                        );
                    }
                }
            }
            _ => {
                let fname = if i == 0 {
                    "Argument #1 ($array)".to_string()
                } else {
                    format!("Argument #{}", i + 1)
                };
                return throw(
                    vm,
                    b"TypeError",
                    format!(
                        "array_multisort(): {} must be an array or a sort flag",
                        fname
                    ),
                );
            }
        }
    }
    let size = arrays[0].0.borrow().len();
    for a in &arrays[1..] {
        if a.0.borrow().len() != size {
            return throw(
                vm,
                b"ValueError",
                "Array sizes are inconsistent".to_string(),
            );
        }
    }
    if size < 1 {
        return Ok(Value::True);
    }
    if let Some(last) = arrays.last_mut() {
        last.1 = sort_type;
        last.2 = sort_order_desc;
    }
    // Rows: one (key, value) per array.
    let cols: Vec<Vec<(ArrayKey, Value)>> = arrays
        .iter()
        .map(|a| {
            a.0.borrow()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        })
        .collect();
    let rows: Vec<Vec<(ArrayKey, Value)>> = (0..size)
        .map(|k| cols.iter().map(|c| c[k].clone()).collect())
        .collect();
    let flags: Vec<(i64, bool)> = arrays.iter().map(|a| (a.1, a.2)).collect();
    let sorted = crate::zsort::zend_sort_stable(rows, |x, y| {
        for (r, (ty, desc)) in flags.iter().enumerate() {
            if vm.current_exception.is_some() {
                return 0;
            }
            let mut c = php_data_cmp(vm, &x[r].1, &y[r].1, *ty);
            if *desc {
                c = -c;
            }
            if c != 0 {
                return if c > 0 { 1 } else { -1 };
            }
        }
        0
    });
    if vm.current_exception.is_some() {
        return Ok(Value::Null);
    }
    for (i, a) in arrays.iter().enumerate() {
        let mut new_arr = PhpArray::new();
        for row in &sorted {
            let (k, v) = &row[i];
            match k {
                ArrayKey::String(_) => new_arr.set(k.clone(), v.clone()),
                ArrayKey::Int(_) => new_arr.push(v.clone()),
            }
        }
        *a.0.borrow_mut() = new_arr;
    }
    Ok(Value::True)
}

// ============= File I/O implementation using thread-local file handle storage =============

use std::collections::HashMap as StdHashMap;
use std::io::{Read as IoRead, Seek, SeekFrom, Write as IoWrite};

/// Userspace stream wrappers (stream_wrapper_register classes).
#[path = "userstream.rs"]
pub(crate) mod userstream;

/// The Directory class (ext/standard/dir.c).
#[path = "dir_class.rs"]
pub(crate) mod dir_class;

/// Socket transports (tcp/udp/unix/udg) and socket stream functions.
#[path = "sockets.rs"]
pub(crate) mod xp_socket;

/// The http:// and https:// wrapper.
#[path = "http_wrapper.rs"]
mod http_wrapper;

/// The ftp:// and ftps:// wrapper.
#[path = "ftp_wrapper.rs"]
mod ftp_wrapper;

/// proc_open() family.
#[path = "proc_open.rs"]
pub(crate) mod proc_open;

/// CLI: write pending unbuffered output before blocking or spawning.
pub(crate) fn flush_cli_output(vm: &mut Vm) {
    vm.flush_cli_output();
}

thread_local! {
    static FILE_HANDLES: RefCell<StdHashMap<i64, FileHandle>> = RefCell::new(StdHashMap::new());
    /// stream->wrapperdata of http streams (their response headers).
    static HTTP_WRAPPER_DATA: RefCell<StdHashMap<i64, Value>> = RefCell::new(StdHashMap::new());
    /// Directory handles keyed by resource id. Each entry stores the directory
    /// path (for rewind) and a remaining list of entry names that we pop from
    /// the back to feed readdir().
    static DIR_HANDLES: RefCell<StdHashMap<i64, DirHandle>> = RefCell::new(StdHashMap::new());
    /// "Current" directory stream pointer used as fallback when readdir/closedir/
    /// rewinddir are called with no argument or null. PHP keeps this state per
    /// request; opendir() updates it.
    static CURRENT_DIR_RESOURCE: std::cell::Cell<Option<i64>> = const { std::cell::Cell::new(None) };
    /// DIRG(default_dir): the reference that keeps the last opened
    /// directory alive.
    static CURRENT_DIR_REF: RefCell<Option<Value>> = const { RefCell::new(None) };
    /// Stream contexts keyed by resource id. Each entry stores the options
    /// (wrapper => option_name => value) and params (notification, options).
    static STREAM_CONTEXTS: RefCell<StdHashMap<i64, StreamContext>> = RefCell::new(StdHashMap::new());
    /// User-defined stream wrappers: scheme -> (class_name, flags).
    static USER_WRAPPERS: RefCell<StdHashMap<Vec<u8>, UserWrapper>> = RefCell::new(StdHashMap::new());
}

/// Context options: wrapper => option => value, in insertion order.
pub(crate) type CtxOptions =
    goro_core::fxhash::FxIndexMap<Vec<u8>, goro_core::fxhash::FxIndexMap<Vec<u8>, Value>>;

/// Option `wrapper`/`name` of the stream context resource `id`.
pub(crate) fn stream_context_option(id: i64, wrapper: &[u8], name: &[u8]) -> Option<Value> {
    STREAM_CONTEXTS.with(|c| {
        c.borrow()
            .get(&id)
            .and_then(|ctx| ctx.options.get(wrapper))
            .and_then(|o| o.get(name))
            .cloned()
    })
}

#[derive(Clone, Default)]
pub(crate) struct StreamContext {
    /// Map: wrapper_name -> map(option_name -> Value), in insertion order.
    pub options: CtxOptions,
    /// Notification callback (or null if unset).
    pub notification: Option<Value>,
}

#[derive(Clone)]
pub(crate) struct UserWrapper {
    #[allow(dead_code)]
    pub class_name: Vec<u8>,
    #[allow(dead_code)]
    pub flags: i64,
}

pub(crate) struct DirHandle {
    pub path: String,
    pub remaining: Vec<String>,
}

fn read_dir_entries(path: &str) -> std::io::Result<Vec<String>> {
    let entries = std::fs::read_dir(path)?;
    let mut names: Vec<String> = vec![".".to_string(), "..".to_string()];
    for e in entries.flatten() {
        names.push(e.file_name().to_string_lossy().to_string());
    }
    names.sort();
    // Reverse so we can pop from the end efficiently while still feeding entries
    // in ascending order.
    names.reverse();
    Ok(names)
}

/// The glob:// wrapper (php_glob_stream_opener): the base names of the
/// pattern's matches, those outside open_basedir left out.
fn glob_stream_entries(vm: &mut Vm, pattern: &str) -> Vec<String> {
    let mut glob_buf: libc::glob_t = unsafe { std::mem::zeroed() };
    let c_pattern = std::ffi::CString::new(pattern.as_bytes()).unwrap_or_default();
    let ret = unsafe { libc::glob(c_pattern.as_ptr(), 0, None, &mut glob_buf) };
    let mut names = Vec::new();
    if ret == 0 {
        for i in 0..glob_buf.gl_pathc {
            let path = unsafe {
                let p = *glob_buf.gl_pathv.add(i);
                std::ffi::CStr::from_ptr(p).to_string_lossy().to_string()
            };
            if !goro_core::basedir::check_open_basedir_ex(vm, "", &path, false) {
                continue;
            }
            let base = match path.trim_end_matches('/').rfind('/') {
                Some(p) => path[p + 1..].to_string(),
                None => path.clone(),
            };
            names.push(base);
        }
    }
    unsafe {
        libc::globfree(&mut glob_buf);
    }
    names
}

/// A directory handle over a fixed list of entries (glob://).
fn alloc_dir_handle_entries(path: String, mut names: Vec<String>) -> i64 {
    // readdir() pops from the end.
    names.reverse();
    let did = goro_core::value::alloc_resource_id();
    DIR_HANDLES.with(|dh| {
        dh.borrow_mut().insert(
            did,
            DirHandle {
                path,
                remaining: names,
            },
        );
    });
    goro_core::value::register_resource(did, b"stream");
    set_current_dir(Some(did));
    did
}

fn alloc_dir_handle(path: String) -> i64 {
    let names = read_dir_entries(&path).unwrap_or_default();
    {
        let did = goro_core::value::alloc_resource_id();
        DIR_HANDLES.with(|dh| {
            dh.borrow_mut().insert(
                did,
                DirHandle {
                    path,
                    remaining: names,
                },
            );
        });
        goro_core::value::register_resource(did, b"stream");
        set_current_dir(Some(did));
        did
    }
}

/// Return the resource id from a directory handle Value. Accepts a Resource
/// or Long id (back-compat), a Directory object whose `handle` property holds
/// the resource id, or a Reference wrapping either of those.
fn dir_handle_resource_id(val: &Value) -> Option<i64> {
    match val {
        Value::Resource(n) => Some(n.id()),
        Value::Long(n) => Some(*n),
        Value::Reference(r) => dir_handle_resource_id(&r.borrow()),
        Value::Object(obj) => {
            let ob = obj.borrow();
            if ob.class_name.eq_ignore_ascii_case(b"Directory") {
                let handle = ob.get_property(b"handle");
                match handle {
                    Value::Resource(n) => Some(n.id()),
                    Value::Long(n) => Some(n),
                    _ => None,
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

/// For streams that are not backed by a real OS file (php://stdout,
/// php://stderr, php://output), writes are routed through `vm.write_output`
/// instead of writing to `file`. The tempfile we store in `file` is just a
/// placeholder so the rest of the code can keep using a single struct.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum SpecialStream {
    None,
    Stdin,
    Stdout,
    Stderr,
    Output,
    Memory,
    Temp,
}

struct FileHandle {
    file: std::fs::File,
    #[allow(dead_code)]
    mode: String,
    eof: bool,
    /// For special streams (php://stdout etc.), routes writes through vm.
    special: SpecialStream,
    /// Buffer of bytes pending consumption by the read filter chain.
    #[allow(dead_code)]
    filter_read_pending: Vec<u8>,
    /// Buffer of post-filter bytes ready to be returned by fread/fgets.
    filter_read_ready: Vec<u8>,
    /// URI that opened this stream (file path or php:// URL). Reported via stream_get_meta_data().
    pub uri: String,
    /// Wrapper type reported via stream_get_meta_data. Defaults to "plainfile" for files.
    pub wrapper_type: String,
    /// Stream type reported via stream_get_meta_data. Defaults to "STDIO".
    pub stream_type: String,
    /// Whether this stream is seekable.
    pub seekable: bool,
    /// For user-stream wrappers: object instance of the wrapper class.
    pub user_wrapper: Option<Value>,
    /// For user-stream wrappers: PHP class name of the wrapper class.
    #[allow(dead_code)]
    pub user_wrapper_class: Option<Vec<u8>>,
    /// For user-stream wrappers: scheme name (e.g. "var" for "var://").
    #[allow(dead_code)]
    pub user_scheme: Option<String>,
    /// Socket streams: php_netstream_data_t.
    pub sock: Option<xp_socket::SockData>,
    /// stream->fatal_error: a read filter failed.
    pub fatal_error: bool,
    /// data: streams: the RFC 2397 meta data (stream_get_meta_data()).
    pub data_meta: Option<Vec<(Vec<u8>, Value)>>,
    /// stream->position of a stream that cannot seek (pipes, FIFOs, ttys):
    /// counted from the bytes read and written. It starts at -1 for one
    /// opened from a descriptor (_php_stream_fopen_from_fd()) and at 0 for
    /// popen(). None: ftell() asks the descriptor.
    pub pipe_pos: Option<i64>,
    /// Socket streams with the TLS layer set up (php_openssl_netstream_data_t).
    pub crypto: Option<Box<dyn goro_core::crypto::CryptoStream>>,
    /// PHP_STREAM_FLAG_DETECT_EOL / PHP_STREAM_FLAG_EOL_MAC (EOL_*).
    pub eol: u8,
    /// Plain files opened for appending: stream->position, which starts at
    /// 0 and advances by what is written (while the descriptor jumps to the
    /// end): (position, descriptor offset) at the last write or seek.
    pub append_pos: Option<(u64, u64)>,
    /// php://temp still held in memory: the size at which it moves to a
    /// temporary file (a new stream, which takes a resource id).
    pub temp_max: Option<u64>,
}

impl FileHandle {
    /// The descriptor offset minus what is buffered.
    fn logical_os_pos(&mut self) -> Option<u64> {
        let buffered = self.filter_read_ready.len() as u64;
        self.file
            .stream_position()
            .ok()
            .map(|p| p.saturating_sub(buffered))
    }

    /// stream->position of an append-mode file.
    fn append_position(&mut self) -> Option<u64> {
        let (base, os_base) = self.append_pos?;
        let os = self.logical_os_pos()?;
        Some(base + os.saturating_sub(os_base))
    }
}

/// FileHandle::eol: not decided yet (auto_detect_line_endings is looked
/// up on the first line read), "\n" lines, detection pending, "\r" lines.
const EOL_UNSET: u8 = 0;
const EOL_LF: u8 = 1;
const EOL_DETECT: u8 = 2;
const EOL_MAC: u8 = 3;

impl Drop for FileHandle {
    /// php_openssl_sockop_close(): an active TLS layer sends close_notify
    /// before the descriptor goes away.
    fn drop(&mut self) {
        if let Some(c) = self.crypto.as_mut() {
            c.close(std::os::unix::io::AsRawFd::as_raw_fd(&self.file));
        }
    }
}

impl FileHandle {
    /// A php://stdin, php://stdout, php://stderr or php://fd/N stream over
    /// a dup()ed descriptor.
    fn is_dup_descriptor(&self) -> bool {
        matches!(self.special, SpecialStream::None)
            && self.wrapper_type == "PHP"
            && self.stream_type == "STDIO"
            && self.sock.is_none()
    }

    /// ops->read() on the underlying descriptor. Socket streams follow
    /// php_sockop_read(): Err(WouldBlock) means "no data right now" (not
    /// EOF) and Err(TimedOut) a timed-out blocking read. `has_buffered`
    /// says data was already gathered by the current read call.
    fn read_raw(&mut self, buf: &mut [u8], has_buffered: bool) -> std::io::Result<usize> {
        // Data already read but not consumed (php_stream readbuf).
        if !self.filter_read_ready.is_empty() && !buf.is_empty() {
            let n = buf.len().min(self.filter_read_ready.len());
            buf[..n].copy_from_slice(&self.filter_read_ready[..n]);
            self.filter_read_ready.drain(..n);
            return Ok(n);
        }
        // php_stream_fill_read_buffer(): descriptor-backed streams read a
        // whole chunk and keep what the caller did not ask for.
        if self.is_buffered() && buf.len() < STREAM_CHUNK_SIZE {
            let mut chunk = vec![0u8; STREAM_CHUNK_SIZE];
            let n = self.read_direct(&mut chunk, has_buffered)?;
            let k = n.min(buf.len());
            buf[..k].copy_from_slice(&chunk[..k]);
            if n > k {
                self.filter_read_ready.extend_from_slice(&chunk[k..n]);
            }
            return Ok(k);
        }
        self.read_direct(buf, has_buffered)
    }

    /// ops->read() straight from the descriptor, bypassing the read buffer.
    fn read_direct(&mut self, buf: &mut [u8], has_buffered: bool) -> std::io::Result<usize> {
        if self.sock.is_some() {
            xp_socket::sock_read(self, buf, has_buffered)
        } else {
            let n = self.file.read(buf)?;
            if let Some(p) = self.pipe_pos.as_mut() {
                *p += n as i64;
            }
            Ok(n)
        }
    }

    /// Streams read through a read buffer (memory/temp streams and the
    /// output sinks are not: PHP_STREAM_FLAG_NO_BUFFER).
    fn is_buffered(&self) -> bool {
        self.special == SpecialStream::None && self.user_wrapper.is_none()
    }

    /// _php_stream_read() keeps reading until the request is satisfied
    /// only for plain files and memory/temp streams.
    fn is_greedy(&self) -> bool {
        self.sock.is_none()
            && (matches!(self.special, SpecialStream::Memory | SpecialStream::Temp)
                || (self.seekable && self.wrapper_type == "plainfile"))
    }
}

/// zend_close_rsrc_list(): streams are freed at the end of the request (in
/// reverse order); for streams with filters this flushes the write chain
/// and runs the filters' onClose().
fn shutdown_filtered_streams(vm: &mut Vm) {
    let mut ids = crate::stream_filters::streams_with_filters();
    ids.sort();
    for fid in ids.into_iter().rev() {
        if !has_file_handle(fid) || userstream::get_us(fid).is_some() {
            continue;
        }
        goro_core::value::close_resource(fid);
        let saved = vm.current_exception.take();
        flush_write_filters(vm, fid, true);
        crate::stream_filters::remove_stream_filters(vm, fid);
        let fh = FILE_HANDLES.with(|h| h.borrow_mut().remove(&fid));
        // STDIN/STDOUT/STDERR: PHP_STREAM_FLAG_NO_RSCR_DTOR_CLOSE, the
        // resource is released but descriptors 0-2 stay open.
        if (1..=3).contains(&fid) {
            std::mem::forget(fh);
        }
        if vm.current_exception.is_none() {
            vm.current_exception = saved;
        }
    }
}

/// PHP_FUNCTION(fnmatch)
fn fnmatch_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    const MAXPATHLEN: usize = 4096;
    let pattern = args
        .first()
        .map(|v| v.to_php_string())
        .unwrap_or_else(PhpString::empty);
    let filename = args
        .get(1)
        .map(|v| v.to_php_string())
        .unwrap_or_else(PhpString::empty);
    let flags = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    for (n, name, v) in [(1, "pattern", &pattern), (2, "filename", &filename)] {
        if v.as_bytes().contains(&0) {
            return Err(throw_value_error_msg(
                vm,
                &format!(
                    "fnmatch(): Argument #{} (${}) must not contain any null bytes",
                    n, name
                ),
            ));
        }
    }
    if filename.len() >= MAXPATHLEN {
        vm.emit_warning(&format!(
            "fnmatch(): Filename exceeds the maximum allowed length of {} characters",
            MAXPATHLEN
        ));
        return Ok(Value::False);
    }
    if pattern.len() >= MAXPATHLEN {
        vm.emit_warning(&format!(
            "fnmatch(): Pattern exceeds the maximum allowed length of {} characters",
            MAXPATHLEN
        ));
        return Ok(Value::False);
    }
    let p = std::ffi::CString::new(pattern.as_bytes()).unwrap_or_default();
    let f = std::ffi::CString::new(filename.as_bytes()).unwrap_or_default();
    let r = unsafe { libc::fnmatch(p.as_ptr(), f.as_ptr(), flags as libc::c_int) };
    Ok(if r == 0 { Value::True } else { Value::False })
}

/// Re-read the state of descriptors 0-2 after the embedder replaced them
/// (e.g. the PHPT runner installing a test's stdin).
pub fn refresh_std_streams() {
    FILE_HANDLES.with(|h| {
        let mut h = h.borrow_mut();
        for (fid, fd) in [(1i64, 0), (2, 1), (3, 2)] {
            if let Some(fh) = h.get_mut(&fid) {
                fh.seekable = unsafe { libc::lseek(fd, 0, libc::SEEK_CUR) } >= 0;
                fh.eof = false;
                fh.filter_read_ready.clear();
            }
        }
    });
}

/// The STDIN/STDOUT/STDERR constants (resources 1-3) wrap descriptors 0-2
/// (cli_register_file_handles()); output to the latter two still goes
/// through the VM output layer.
fn register_std_streams() {
    use std::os::unix::io::FromRawFd;
    FILE_HANDLES.with(|h| {
        let mut h = h.borrow_mut();
        for (fid, fd, mode, special, uri) in [
            (1i64, 0, "rb", SpecialStream::None, "php://stdin"),
            (2, 1, "wb", SpecialStream::Stdout, "php://stdout"),
            (3, 2, "wb", SpecialStream::Stderr, "php://stderr"),
        ] {
            if h.contains_key(&fid) {
                continue;
            }
            if unsafe { libc::fcntl(fd, libc::F_GETFD) } < 0 {
                continue;
            }
            let seekable = unsafe { libc::lseek(fd, 0, libc::SEEK_CUR) } >= 0;
            // The handle owns the real descriptor: fclose(STDIN) closes 0.
            let file = unsafe { std::fs::File::from_raw_fd(fd) };
            h.insert(
                fid,
                FileHandle {
                    file,
                    mode: mode.to_string(),
                    eof: false,
                    special,
                    filter_read_pending: Vec::new(),
                    filter_read_ready: Vec::new(),
                    uri: uri.to_string(),
                    wrapper_type: "PHP".to_string(),
                    stream_type: "STDIO".to_string(),
                    seekable,
                    user_wrapper: None,
                    user_wrapper_class: None,
                    user_scheme: None,
                    sock: None,
                    fatal_error: false,
                    data_meta: None,
                    crypto: None,
                    pipe_pos: if seekable { None } else { Some(-1) },
                    eol: EOL_UNSET,
                    append_pos: None,
                    temp_max: None,
                },
            );
        }
    });
}

/// Default stream chunk size (php_stream chunk_size).
const STREAM_CHUNK_SIZE: usize = 8192;

fn alloc_file_handle(file: std::fs::File, mode: &str) -> i64 {
    alloc_file_handle_special(file, mode, SpecialStream::None)
}

fn alloc_file_handle_uri(file: std::fs::File, mode: &str, uri: &str) -> i64 {
    let fid = alloc_file_handle_special(file, mode, SpecialStream::None);
    if mode.starts_with('a') {
        FILE_HANDLES.with(|h| {
            if let Some(fh) = h.borrow_mut().get_mut(&fid) {
                fh.append_pos = Some((0, 0));
            }
        });
    }
    FILE_HANDLES.with(|h| {
        if let Some(fh) = h.borrow_mut().get_mut(&fid) {
            fh.uri = uri.to_string();
        }
    });
    fid
}

fn alloc_file_handle_special_uri(
    file: std::fs::File,
    mode: &str,
    special: SpecialStream,
    uri: &str,
) -> i64 {
    let fid = alloc_file_handle_special(file, mode, special);
    FILE_HANDLES.with(|h| {
        if let Some(fh) = h.borrow_mut().get_mut(&fid) {
            fh.uri = uri.to_string();
            // For php:// streams, set wrapper_type and stream_type appropriately.
            match special {
                // Memory/temp streams are always read-write ("w+b"),
                // whatever mode they were opened with.
                // php_stream_mode_from_str() / _php_stream_mode_to_str():
                // read-only, append or read-write.
                SpecialStream::Memory => {
                    fh.wrapper_type = "PHP".to_string();
                    fh.stream_type = "MEMORY".to_string();
                    fh.mode = temp_stream_mode(mode).to_string();
                }
                SpecialStream::Temp => {
                    fh.wrapper_type = "PHP".to_string();
                    fh.stream_type = "TEMP".to_string();
                    fh.mode = temp_stream_mode(mode).to_string();
                }
                SpecialStream::Stdin
                | SpecialStream::Stdout
                | SpecialStream::Stderr
                | SpecialStream::Output => {
                    fh.wrapper_type = "PHP".to_string();
                    fh.stream_type = "STDIO".to_string();
                    fh.seekable = false;
                }
                _ => {}
            }
        }
    });
    fid
}

/// php_stream_url_wrap_rfc2397(): parse a data: URL into its meta data
/// (mediatype, parameters, base64) and decoded contents.
pub(crate) fn parse_rfc2397(url: &[u8]) -> Result<(Vec<(Vec<u8>, Value)>, Vec<u8>), &'static str> {
    let mut path = &url[5..];
    // C strings end at NUL.
    if let Some(p) = path.iter().position(|&c| c == 0) {
        path = &path[..p];
    }
    if path.starts_with(b"//") {
        path = &path[2..];
    }
    let comma = path
        .iter()
        .position(|&c| c == b',')
        .ok_or("rfc2397: no comma in URL")?;
    let mut meta: Vec<(Vec<u8>, Value)> = Vec::new();
    let set = |meta: &mut Vec<(Vec<u8>, Value)>, k: &[u8], v: Value| {
        if let Some(e) = meta.iter_mut().find(|(n, _)| n == k) {
            e.1 = v;
        } else {
            meta.push((k.to_vec(), v));
        }
    };
    let mut base64 = false;
    if comma != 0 {
        let mut m = &path[..comma];
        let semi = m.iter().position(|&c| c == b';');
        let sep = m.iter().position(|&c| c == b'/');
        if semi.is_none() && sep.is_none() {
            return Err("rfc2397: illegal media type");
        }
        match (semi, sep) {
            (None, _) => {
                set(
                    &mut meta,
                    b"mediatype",
                    Value::String(PhpString::from_bytes(m)),
                );
                m = &[];
            }
            (Some(sm), Some(sp)) if sp < sm => {
                set(
                    &mut meta,
                    b"mediatype",
                    Value::String(PhpString::from_bytes(&m[..sm])),
                );
                m = &m[sm..];
            }
            (Some(sm), _) => {
                if sm != 0 || m != b";base64" {
                    return Err("rfc2397: illegal media type");
                }
            }
        }
        // Parameters and ";base64".
        while m.first() == Some(&b';') {
            m = &m[1..];
            let eq = m.iter().position(|&c| c == b'=');
            let semi = m.iter().position(|&c| c == b';');
            match eq {
                Some(e) if semi.is_none_or(|s| s > e) => {
                    let name = &m[..e];
                    let vend = semi.unwrap_or(m.len());
                    let val = &m[e + 1..vend];
                    if name != b"mediatype" {
                        set(&mut meta, name, Value::String(PhpString::from_bytes(val)));
                    }
                    m = &m[vend..];
                }
                _ => {
                    if m != b"base64" {
                        return Err("rfc2397: illegal parameter");
                    }
                    base64 = true;
                    m = &[];
                    break;
                }
            }
        }
        if !m.is_empty() {
            return Err("rfc2397: illegal URL");
        }
    }
    set(
        &mut meta,
        b"base64",
        if base64 { Value::True } else { Value::False },
    );
    let data = &path[comma + 1..];
    let bytes = if base64 {
        // php_base64_decode_ex(..., strict = 1)
        match base64_decode_strict(data) {
            Some(b) => b,
            None => return Err("rfc2397: unable to decode"),
        }
    } else {
        data_uri_urldecode(data)
    };
    Ok((meta, bytes))
}

/// php_base64_decode_ex() in strict mode.
fn base64_decode_strict(input: &[u8]) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a' + 26) as u32),
            b'0'..=b'9' => Some((c - b'0' + 52) as u32),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    };
    let mut out = Vec::new();
    let mut acc: u32 = 0;
    let mut n = 0usize;
    let mut padding = 0usize;
    for &c in input {
        if c == b'=' {
            padding += 1;
            continue;
        }
        let v = match val(c) {
            Some(v) => v,
            None => {
                if matches!(c, b' ' | b'\t' | b'\r' | b'\n') {
                    continue;
                }
                return None;
            }
        };
        if padding > 0 {
            return None;
        }
        acc = (acc << 6) | v;
        n += 1;
        if n.is_multiple_of(4) {
            out.push((acc >> 16) as u8);
            out.push((acc >> 8) as u8);
            out.push(acc as u8);
            acc = 0;
        }
    }
    match n % 4 {
        0 => {}
        1 => return None,
        2 => out.push((acc >> 4) as u8),
        _ => {
            out.push((acc >> 10) as u8);
            out.push((acc >> 2) as u8);
        }
    }
    if padding > 0 && (padding > 2 || !(n + padding).is_multiple_of(4)) {
        return None;
    }
    Some(out)
}

/// Mode string of a php://memory / php://temp stream opened with `mode`.
fn temp_stream_mode(mode: &str) -> &'static str {
    if mode.contains('a') {
        "a+b"
    } else if mode.contains('w') || mode.contains('+') {
        "w+b"
    } else {
        "rb"
    }
}

/// Helper used by file functions to validate that the first argument is an
/// open stream resource. If the value is not a stream resource (or is a closed
/// stream resource), this throws a TypeError matching PHP's wording.
///
/// Returns Ok(fid) for valid streams (FILE_HANDLES contains the id).
/// PHP_Z_PARAM_STREAM: a non-resource is a parameter type error.
pub(crate) fn require_resource_arg(
    vm: &mut Vm,
    val: &Value,
    func_name: &str,
) -> Result<(), VmError> {
    let v = val.deref();
    if matches!(v, Value::Resource(_)) {
        return Ok(());
    }
    let msg = format!(
        "{}(): Argument #1 ($stream) must be of type resource, {} given",
        func_name,
        Vm::value_type_name(&v)
    );
    let exc = vm.throw_type_error(msg.clone());
    vm.current_exception = Some(exc);
    Err(VmError {
        message: msg,
        line: vm.current_line,
    })
}

fn require_open_stream(vm: &mut Vm, val: &Value, func_name: &str) -> Result<i64, VmError> {
    require_resource_arg(vm, val, func_name)?;
    let fid = val.to_long();
    let has_handle = FILE_HANDLES.with(|h| h.borrow().contains_key(&fid));
    if has_handle {
        // CLI: stream I/O may block (or be observed by another process);
        // pending unbuffered output goes out first.
        vm.flush_cli_output();
        return Ok(fid);
    }
    let msg = format!(
        "{}(): Argument #1 ($stream) must be an open stream resource",
        func_name
    );
    let exc = vm.throw_type_error(msg.clone());
    vm.current_exception = Some(exc);
    Err(VmError {
        message: msg,
        line: vm.current_line,
    })
}

fn alloc_file_handle_special(file: std::fs::File, mode: &str, special: SpecialStream) -> i64 {
    {
        let fid = goro_core::value::alloc_resource_id();
        // _php_stream_fopen_from_fd(): a descriptor that cannot seek starts
        // at position -1.
        let pipe_pos = {
            use std::os::unix::io::AsRawFd;
            if unsafe { libc::lseek(file.as_raw_fd(), 0, libc::SEEK_CUR) } < 0 {
                Some(-1)
            } else {
                None
            }
        };
        FILE_HANDLES.with(|handles| {
            handles.borrow_mut().insert(
                fid,
                FileHandle {
                    file,
                    mode: mode.to_string(),
                    eof: false,
                    special,
                    filter_read_pending: Vec::new(),
                    filter_read_ready: Vec::new(),
                    uri: String::new(),
                    wrapper_type: "plainfile".to_string(),
                    stream_type: "STDIO".to_string(),
                    seekable: true,
                    user_wrapper: None,
                    user_wrapper_class: None,
                    user_scheme: None,
                    sock: None,
                    fatal_error: false,
                    data_meta: None,
                    crypto: None,
                    pipe_pos,
                    eol: EOL_UNSET,
                    append_pos: None,
                    temp_max: None,
                },
            );
        });
        fid
    }
}

fn data_uri_urldecode(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        let b = input[i];
        if b == b'%' && i + 2 < input.len() {
            let h1 = hex_digit_misc(input[i + 1]);
            let h2 = hex_digit_misc(input[i + 2]);
            if let (Some(a), Some(b)) = (h1, h2) {
                out.push((a << 4) | b);
                i += 3;
                continue;
            }
        }
        if b == b'+' {
            out.push(b' ');
        } else {
            out.push(b);
        }
        i += 1;
    }
    out
}

fn hex_digit_misc(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Call a builtin through the function table (so wrappers installed over
/// it, e.g. userspace stream dispatch, apply), falling back to `f`.
fn call_registered(
    vm: &mut Vm,
    name: &[u8],
    f: fn(&mut Vm, &[Value]) -> Result<Value, VmError>,
    args: &[Value],
) -> Result<Value, VmError> {
    let g = vm.functions.get(name).copied().unwrap_or(f);
    g(vm, args)
}

pub(crate) fn fopen_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // php_error_docref() names the active function (file_get_contents(),
    // highlight_file(), ...) when fopen's implementation is reached from it.
    let fopen_caller = vm
        .current_builtin_name()
        .unwrap_or_else(|| "fopen".to_string());
    // Binary-safe copy of the name (data: payloads, php://filter targets).
    let raw_name: Vec<u8> = args
        .first()
        .map(|v| v.to_php_string().as_bytes().to_vec())
        .unwrap_or_default();
    let mut filename = args
        .first()
        .unwrap_or(&Value::Null)
        .to_php_string()
        .to_string_lossy();
    // Empty filename → ValueError, matching PHP's behavior.
    if filename.is_empty() {
        let msg = "fopen(): Argument #1 ($filename) cannot be empty";
        let exc = vm.create_exception(b"ValueError", msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg.to_string(),
            line: vm.current_line,
        });
    }
    // Preserve the original filename for the uri field of stream_get_meta_data.
    // PHP keeps the path as passed to fopen() (including any file:// prefix).
    let original_uri = filename.clone();
    let mode = args
        .get(1)
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_else(|| "r".to_string());

    // PHP fopen rejects null bytes embedded in the path.
    if filename.contains('\0') {
        vm.emit_warning(&format!(
            "{}(): Argument #1 ($filename) must not contain any null bytes",
            fopen_caller
        ));
        return Ok(Value::False);
    }

    // Handle file:// URIs. Per RFC, file://[host]/path. If host is empty or
    // "localhost", treat as local path. Otherwise emit warnings matching PHP.
    // Use case-insensitive match for the scheme (RFC 3986 schemes are
    // case-insensitive — PHP also accepts File://).
    if filename.to_ascii_lowercase().starts_with("file://") {
        let (rest, host, path): (String, String, String) = {
            let r = filename[7..].to_string();
            let (h, p) = match r.find('/') {
                Some(idx) => (r[..idx].to_string(), r[idx..].to_string()),
                None => (r.clone(), String::new()),
            };
            (r, h, p)
        };
        if !host.is_empty() && host != "localhost" {
            vm.emit_warning(&format!(
                "{}(): Remote host file access not supported, file://{}",
                fopen_caller, rest
            ));
            vm.emit_warning(&format!(
                "{}({}): Failed to open stream: no suitable wrapper could be found",
                fopen_caller, filename
            ));
            return Ok(Value::False);
        }
        // Strip prefix and continue with local path handling.
        filename = path;
    }

    // Handle http:// and https:// URLs via the built-in HTTP client. Fetch
    // the body upfront and back the resulting handle with a temp file so
    // subsequent fread/feof/fgets calls work as normal file ops.
    if filename.starts_with("http://") || filename.starts_with("https://") {
        let caller = fopen_caller.clone();
        let body = match http_wrapper::http_open(vm, &caller, &filename, args.get(3)) {
            Some(b) => b,
            None if vm.current_exception.is_some() => {
                return Err(VmError {
                    message: "Uncaught exception".into(),
                    line: vm.current_line,
                });
            }
            None => return Ok(Value::False),
        };
        // Write to a temp file
        let tmp_path = std::env::temp_dir().join(format!(
            "goro_http_{}_{}.tmp",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        if std::fs::write(&tmp_path, &body).is_err() {
            return Ok(Value::False);
        }
        if let Ok(f) = std::fs::File::open(&tmp_path) {
            // Best-effort cleanup; we leak the temp file otherwise.
            let _ = std::fs::remove_file(&tmp_path);
            let fid = alloc_file_handle(f, &mode);
            goro_core::value::register_resource(fid, b"stream");
            // The http wrapper's stream: its meta data, with the response
            // headers as stream->wrapperdata.
            FILE_HANDLES.with(|h| {
                if let Some(fh) = h.borrow_mut().get_mut(&fid) {
                    fh.wrapper_type = "http".to_string();
                    fh.stream_type = "tcp_socket/ssl".to_string();
                    fh.seekable = false;
                    fh.uri = filename.clone();
                }
            });
            let headers = http_wrapper::last_response_headers();
            if !matches!(headers, Value::Null) {
                HTTP_WRAPPER_DATA.with(|w| w.borrow_mut().insert(fid, headers));
            }
            return Ok(Value::resource(fid));
        }
        return Ok(Value::False);
    }

    // Normalize mode: strip 'b' (binary) and 't' (text) flags - they're all the same on Linux
    // php_stream_parse_fopen_modes(): the first character and '+' matter,
    // anything else ('b', 't', 'e', 'n', zlib's level digits...) is ignored.
    let mode_clean: String = {
        let mut m: String = mode.chars().take(1).collect();
        if mode.contains('+') {
            m.push('+');
        }
        m
    };

    // php://filter/<chain>/resource=<inner_url> (php_stream_url_wrap_php).
    if filename.len() > 13 && filename[..13].eq_ignore_ascii_case("php://filter/") {
        let rest = &filename[12..];
        let mut mode_rw = 0;
        if mode.contains('r') || mode.contains('+') {
            mode_rw |= crate::stream_filters::STREAM_FILTER_READ;
        }
        if mode.contains('w') || mode.contains('+') || mode.contains('a') {
            mode_rw |= crate::stream_filters::STREAM_FILTER_WRITE;
        }
        let Some(idx) = rest.find("/resource=") else {
            let exc = vm.create_exception(b"Error", "No URL resource specified", vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: "No URL resource specified".to_string(),
                line: vm.current_line,
            });
        };
        let chain_str = &rest[..idx];
        // The target, binary-safe.
        let raw_inner = raw_name
            .windows(10)
            .position(|w| w == b"/resource=")
            .map(|p| raw_name[p + 10..].to_vec())
            .unwrap_or_default();
        // Open the inner stream (through the registered fopen, so
        // userspace wrappers can serve it).
        let mut inner_args = vec![
            Value::String(PhpString::from_vec(raw_inner)),
            Value::String(PhpString::from_string(mode.clone())),
            Value::False,
        ];
        if let Some(ctx) = args.get(3) {
            inner_args.push(ctx.clone());
        }
        let inner = call_registered(vm, b"fopen", fopen_fn, &inner_args)?;
        let inner_fid = match inner {
            Value::Resource(n) => n.id(),
            _ => return Ok(Value::False),
        };
        for seg in chain_str.split('/').filter(|p| !p.is_empty()) {
            let lower = seg.to_ascii_lowercase();
            if lower.starts_with("read=") {
                apply_filter_list(vm, inner_fid, &seg[5..], true, false)?;
            } else if lower.starts_with("write=") {
                apply_filter_list(vm, inner_fid, &seg[6..], false, true)?;
            } else {
                apply_filter_list(
                    vm,
                    inner_fid,
                    seg,
                    mode_rw & crate::stream_filters::STREAM_FILTER_READ != 0,
                    mode_rw & crate::stream_filters::STREAM_FILTER_WRITE != 0,
                )?;
            }
        }
        if vm.current_exception.is_some() {
            let _ = call_registered(vm, b"fclose", fclose_fn, &[Value::resource(inner_fid)]);
            return Err(VmError {
                message: String::new(),
                line: vm.current_line,
            });
        }
        return Ok(Value::resource(inner_fid));
    }

    // data: URLs (php_stream_url_wrap_rfc2397).
    if raw_name.starts_with(b"data:") {
        let func = vm
            .current_builtin_name()
            .unwrap_or_else(|| "fopen".to_string());
        match parse_rfc2397(&raw_name) {
            Ok((meta, bytes)) => {
                let mut tmp = match tempfile::tempfile() {
                    Ok(f) => f,
                    Err(_) => return Ok(Value::False),
                };
                use std::io::{Seek, SeekFrom, Write};
                if tmp.write_all(&bytes).is_err() {
                    return Ok(Value::False);
                }
                let _ = tmp.seek(SeekFrom::Start(0));
                let fid = alloc_file_handle_special_uri(tmp, &mode, SpecialStream::Temp, &filename);
                // A php://temp stream: its inner memory stream is a resource too.
                goro_core::value::alloc_resource_id();
                FILE_HANDLES.with(|h| {
                    if let Some(fh) = h.borrow_mut().get_mut(&fid) {
                        fh.wrapper_type = "RFC2397".to_string();
                        fh.stream_type = "RFC2397".to_string();
                        // "enforce exact mode"
                        fh.mode = mode.chars().take(15).collect();
                        fh.data_meta = Some(meta);
                    }
                });
                goro_core::value::register_resource(fid, b"stream");
                return Ok(Value::resource(fid));
            }
            Err(e) => {
                vm.emit_warning(&format!(
                    "{}({}): Failed to open stream: {}",
                    func, filename, e
                ));
                return Ok(Value::False);
            }
        }
    }

    // Handle php:// wrapper paths. php://stdout, php://stderr, php://output
    // are write-only sinks routed through the VM output. php://memory and
    // php://temp are seekable in-memory streams backed by a tempfile.
    if let Some(rest) = filename.strip_prefix("php://") {
        let lower: String = rest.to_ascii_lowercase();
        // php_stream_url_wrap_php(): the CLI's php://stdin (and, from the
        // command line, php://stdout / php://stderr) is a dup() of the
        // descriptor -- a socket stream for a socket. A descriptor that was
        // closed (fclose(STDIN)) cannot be duplicated: the open fails.
        let std_fd = match lower.as_str() {
            "stdin" => Some(0),
            "stdout" if vm.cli_direct_output => Some(1),
            "stderr" if vm.cli_direct_output => Some(2),
            _ => None,
        };
        if let Some(std_fd) = std_fd {
            let fd = unsafe { libc::dup(std_fd) };
            if fd < 0 {
                vm.emit_warning(&format!(
                    "{}({}): Failed to open stream: operation failed",
                    fopen_caller, filename
                ));
                return Ok(Value::False);
            }
            let mut st: libc::stat = unsafe { std::mem::zeroed() };
            if unsafe { libc::fstat(fd, &mut st) } == 0
                && (st.st_mode & libc::S_IFMT) == libc::S_IFSOCK
            {
                return Ok(Value::resource(xp_socket::stream_from_socket(vm, fd)));
            }
            use std::os::unix::io::FromRawFd;
            let file = unsafe { std::fs::File::from_raw_fd(fd) };
            let fid = alloc_file_handle_special_uri(file, &mode, SpecialStream::Stdin, &filename);
            FILE_HANDLES.with(|h| {
                if let Some(fh) = h.borrow_mut().get_mut(&fid) {
                    fh.special = SpecialStream::None;
                    fh.seekable = unsafe { libc::lseek(fd, 0, libc::SEEK_CUR) } >= 0;
                }
            });
            goro_core::value::register_resource(fid, b"stream");
            return Ok(Value::resource(fid));
        }
        let special = match lower.as_str() {
            "stdout" => Some(SpecialStream::Stdout),
            "stderr" => Some(SpecialStream::Stderr),
            "stdin" => Some(SpecialStream::Stdin),
            "output" => Some(SpecialStream::Output),
            "memory" => Some(SpecialStream::Memory),
            "input" => Some(SpecialStream::Memory), // tests don't rely on stdin
            s if s.starts_with("temp") => Some(SpecialStream::Temp),
            _ => None,
        };
        if let Some(sp) = special {
            match tempfile::tempfile() {
                Ok(mut f) => {
                    if lower == "input" {
                        use std::io::{Seek, SeekFrom, Write};
                        let body = get_request_body();
                        let _ = f.write_all(&body);
                        let _ = f.seek(SeekFrom::Start(0));
                    }
                    let is_temp = matches!(sp, SpecialStream::Temp);
                    let fid = alloc_file_handle_special_uri(f, &mode, sp, &filename);
                    if is_temp {
                        // php_stream_temp_create(): its inner memory stream.
                        goro_core::value::alloc_resource_id();
                        let max = lower
                            .strip_prefix("temp/maxmemory:")
                            .and_then(|m| m.parse::<i64>().ok())
                            .filter(|m| *m >= 0)
                            .map(|m| m as u64)
                            .unwrap_or(2 * 1024 * 1024);
                        FILE_HANDLES.with(|h| {
                            if let Some(fh) = h.borrow_mut().get_mut(&fid) {
                                fh.temp_max = Some(max);
                            }
                        });
                    }
                    goro_core::value::register_resource(fid, b"stream");
                    return Ok(Value::resource(fid));
                }
                Err(_) => return Ok(Value::False),
            }
        }

        // php://fd/<n> -- duplicate the file descriptor <n>.
        // php://fd is invalid (must include a number).
        if lower == "fd" {
            vm.emit_warning(&format!("{}(): Invalid php:// URL specified", fopen_caller));
            vm.emit_warning(&format!(
                "{}({}): Failed to open stream: operation failed",
                fopen_caller, filename
            ));
            return Ok(Value::False);
        }
        if let Some(fd_tail) = lower.strip_prefix("fd/") {
            // Reject if trailing slash present or empty
            if fd_tail.is_empty() || fd_tail.contains('/') {
                vm.emit_warning(&format!(
                    "{}({}): Failed to open stream: php://fd/ stream must be specified in the form php://fd/<orig fd>",
                    fopen_caller,
                    filename
                ));
                return Ok(Value::False);
            }
            // Must be a non-negative integer < max FDs.
            let parsed: Option<i64> = fd_tail.parse::<i64>().ok();
            let max_fd: i64 = {
                #[cfg(target_os = "linux")]
                unsafe {
                    let lim = libc::sysconf(libc::_SC_OPEN_MAX);
                    if lim > 0 { lim as i64 } else { 1024 }
                }
                #[cfg(not(target_os = "linux"))]
                {
                    1024
                }
            };
            match parsed {
                Some(n) if n >= 0 && n < max_fd => {
                    // Try to dup the fd. If dup fails, emit error.
                    let dup_fd = unsafe { libc::dup(n as i32) };
                    if dup_fd < 0 {
                        let err = std::io::Error::last_os_error();
                        let errno = err.raw_os_error().unwrap_or(0);
                        vm.emit_warning(&format!(
                            "{}({}): Failed to open stream: Error duping file descriptor {}; possibly it doesn't exist: [{}]: {}",
                            fopen_caller,
                            filename, n, errno, strip_os_error(&err.to_string())
                        ));
                        return Ok(Value::False);
                    }
                    // Convert raw fd into File. We pick a SpecialStream variant
                    // so writes route correctly: fd 1 -> Stdout, fd 2 -> Stderr,
                    // others use None (writes go to the file itself).
                    use std::os::unix::io::FromRawFd;
                    let f = unsafe { std::fs::File::from_raw_fd(dup_fd) };
                    let sp = match n {
                        _ if vm.cli_direct_output => SpecialStream::None,
                        1 => SpecialStream::Stdout,
                        2 => SpecialStream::Stderr,
                        _ => SpecialStream::None,
                    };
                    let fid = alloc_file_handle_special(f, &mode, sp);
                    FILE_HANDLES.with(|h| {
                        if let Some(fh) = h.borrow_mut().get_mut(&fid) {
                            fh.wrapper_type = "PHP".to_string();
                            fh.stream_type = "STDIO".to_string();
                            fh.uri = filename.to_string();
                        }
                    });
                    goro_core::value::register_resource(fid, b"stream");
                    return Ok(Value::resource(fid));
                }
                _ => {
                    vm.emit_warning(&format!(
                        "{}({}): Failed to open stream: The file descriptors must be non-negative numbers smaller than {}",
                        fopen_caller,
                        filename, max_fd
                    ));
                    return Ok(Value::False);
                }
            }
        }
    }

    // Validate mode: the first character must be one of r/w/a/x/c
    // (php_stream_parse_fopen_modes()).
    let valid_mode = matches!(mode.chars().next(), Some('r' | 'w' | 'a' | 'x' | 'c'));
    if !valid_mode {
        vm.emit_warning(&format!(
            "{}({}): Failed to open stream: `{}' is not a valid mode for fopen",
            fopen_caller, filename, mode
        ));
        return Ok(Value::False);
    }

    // Resolve include_path if requested (3rd arg). Only for read-mode and
    // non-absolute non-scheme paths.
    let use_include_path = args.get(2).map(|v| v.is_truthy()).unwrap_or(false);
    let filename: String = if use_include_path
        && (mode_clean == "r" || mode_clean == "r+")
        && !filename.contains("://")
        && !filename.starts_with('/')
        && !filename.starts_with("./")
        && !filename.starts_with("../")
    {
        let inc = vm
            .constants
            .get(b"include_path".as_ref())
            .map(|v| v.to_php_string().to_string_lossy())
            .unwrap_or_else(|| ".".to_string());
        let mut found: Option<String> = None;
        for entry in inc.split(':') {
            // Skip empty entries (PHP does not treat empty as ".").
            if entry.is_empty() {
                continue;
            }
            let candidate = if entry == "." {
                filename.to_string()
            } else {
                format!("{}/{}", entry.trim_end_matches('/'), filename)
            };
            if std::fs::metadata(&candidate).is_ok() {
                found = Some(candidate);
                break;
            }
        }
        // Fall back to the script's directory.
        if found.is_none() {
            let script = vm.current_file.clone();
            if let Some(parent) = std::path::Path::new(&script).parent() {
                let candidate = parent.join(&filename);
                if candidate.exists() {
                    found = Some(candidate.to_string_lossy().to_string());
                }
            }
        }
        found.unwrap_or_else(|| filename.to_string())
    } else {
        filename.to_string()
    };

    // For plain (non-scheme) paths, lexically normalize so that
    // foo/bar/../baz resolves to foo/baz even if bar doesn't exist.
    // Matches PHP's expand_filepath behavior.
    let open_path = if !filename.contains("://") {
        normalize_path_lexical(&filename)
    } else {
        filename.clone()
    };

    let file = match mode_clean.as_str() {
        "r" => std::fs::File::open(&open_path),
        "r+" => std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&open_path),
        "w" => std::fs::File::create(&open_path),
        "w+" => std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&open_path),
        "a" => std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&open_path),
        "a+" => std::fs::OpenOptions::new()
            .read(true)
            .append(true)
            .create(true)
            .open(&open_path),
        "x" => std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&open_path),
        "x+" => std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&open_path),
        "c" => std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&open_path),
        "c+" => std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&open_path),
        _ => std::fs::File::open(&open_path), // default to read
    };

    match file {
        Ok(f) => {
            // PHP's `uri` field in stream_get_meta_data preserves the path passed
            // to fopen() (e.g. relative paths stay relative, file:// prefix is kept).
            let fid = alloc_file_handle_uri(f, &mode, &original_uri);
            goro_core::value::register_resource(fid, b"stream");
            Ok(Value::resource(fid))
        }
        Err(e) => {
            let msg = strip_os_error(&e.to_string());
            vm.emit_warning(&format!(
                "{}({}): Failed to open stream: {}",
                fopen_caller, filename, msg
            ));
            Ok(Value::False)
        }
    }
}

/// The name a stream builtin was called by, for its diagnostics: ext/zlib
/// registers gzread() & co. as aliases of fread() & co., and
/// zend_argument_*_error() name the active function.
pub(crate) fn called_as(vm: &Vm, name: &str) -> String {
    if let Some(cur) = vm.current_builtin_name() {
        let alias_of = match cur.as_str() {
            "gzread" => "fread",
            "gzwrite" | "gzputs" => "fwrite",
            "gzgets" => "fgets",
            "gzgetc" => "fgetc",
            "gzeof" => "feof",
            "gzrewind" => "rewind",
            "gzseek" => "fseek",
            "gztell" => "ftell",
            "gzpassthru" => "fpassthru",
            "gzclose" => "fclose",
            "fputs" => "fwrite",
            _ => "",
        };
        if alias_of == name {
            return cur;
        }
    }
    name.to_string()
}

/// Set the directory readdir()/closedir()/rewinddir() use by default
/// (php_set_default_dir()); it holds a reference to it.
pub(crate) fn set_current_dir(id: Option<i64>) {
    CURRENT_DIR_RESOURCE.with(|c| c.set(id));
    let old = CURRENT_DIR_REF.with(|r| r.replace(id.map(Value::resource)));
    drop(old);
}

/// The stream list destructor (_php_stream_free(PHP_STREAM_FREE_RSRC_DTOR)):
/// a stream handed to PHP code is closed when its last reference goes away,
/// through the registered fclose()/closedir() (so wrappers see it).
fn stream_resource_dtor(vm: &mut Vm, id: i64) {
    goro_core::value::set_resource_no_fclose(id, false);
    if !goro_core::value::is_open_resource(id) || userstream::free_user_stream(vm, id) {
        return;
    }
    let is_dir = DIR_HANDLES.with(|d| d.borrow().contains_key(&id));
    let name: &[u8] = if is_dir { b"closedir" } else { b"fclose" };
    if let Some(f) = vm.functions.get(name).copied() {
        let _ = f(vm, &[Value::resource(id)]);
    }
}

pub(crate) fn fclose_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    require_resource_arg(
        vm,
        args.first().unwrap_or(&Value::Null),
        &called_as(vm, "fclose"),
    )?;
    let fid = args.first().unwrap_or(&Value::Null).to_long();
    // PHP_STREAM_FLAG_NO_FCLOSE: a filter callback is running on it, or
    // an object owns it.
    if crate::stream_filters::in_filter_callback(fid)
        || goro_core::value::is_resource_no_fclose(fid)
    {
        vm.emit_warning(
            "fclose(): cannot close the provided stream, as it must not be manually closed",
        );
        return Ok(Value::False);
    }
    // CLI: output written so far reaches descriptor 1 before a stream
    // (possibly STDOUT itself) is closed.
    vm.flush_cli_output();
    // _php_stream_free(): flush the write chain, then remove the filters.
    flush_write_filters(vm, fid, true);
    crate::stream_filters::remove_stream_filters(vm, fid);
    FILE_HANDLES.with(|handles| {
        handles.borrow_mut().remove(&fid);
    });
    // Mark the resource as closed rather than removing it, so gettype()
    // still reports "resource (closed)" while is_resource() returns false.
    goro_core::value::close_resource(fid);
    xp_socket::release_stream_ctx(fid);
    Ok(Value::True)
}

/// CLI: STDERR writes go straight to descriptor 2 (after pending stdout).
fn write_cli_stderr(vm: &mut Vm, bytes: &[u8]) -> usize {
    vm.flush_cli_output();
    let mut done = 0;
    while done < bytes.len() {
        let n = unsafe {
            libc::write(
                2,
                bytes[done..].as_ptr() as *const libc::c_void,
                bytes.len() - done,
            )
        };
        if n <= 0 {
            break;
        }
        done += n as usize;
    }
    done
}

/// Write `bytes` to the physical backing of the stream (file or vm
/// output for special streams). Used after the write filter chain has
/// been applied.
fn write_to_stream(vm: &mut Vm, fid: i64, bytes: &[u8]) -> usize {
    let n = write_to_stream_inner(vm, fid, bytes);
    FILE_HANDLES.with(|h| {
        if let Some(p) = h
            .borrow_mut()
            .get_mut(&fid)
            .and_then(|fh| fh.pipe_pos.as_mut())
        {
            *p += n as i64;
        }
    });
    n
}

fn write_to_stream_inner(vm: &mut Vm, fid: i64, bytes: &[u8]) -> usize {
    // The PHP constants STDIN/STDOUT/STDERR are pre-registered at resource
    // ids 1/2/3 without a FILE_HANDLES entry. Route those directly.
    if fid == 3 && vm.cli_direct_output {
        return write_cli_stderr(vm, bytes);
    }
    if fid == 2 {
        // CLI: STDOUT is unbuffered and bypasses output buffering.
        vm.write_stdout_stream(bytes);
        return bytes.len();
    }
    if fid == 3 {
        vm.write_output(bytes);
        vm.flush_cli_output();
        return bytes.len();
    }
    // Check special routing for explicitly opened php://stdout etc.
    let special = FILE_HANDLES.with(|h| {
        h.borrow()
            .get(&fid)
            .map(|fh| fh.special)
            .unwrap_or(SpecialStream::None)
    });
    match special {
        SpecialStream::Stderr if vm.cli_direct_output => write_cli_stderr(vm, bytes),
        SpecialStream::Stdout => {
            vm.write_stdout_stream(bytes);
            bytes.len()
        }
        SpecialStream::Output | SpecialStream::Stderr => {
            vm.write_output(bytes);
            bytes.len()
        }
        SpecialStream::Stdin => {
            // Writing to stdin is a no-op; return 0
            0
        }
        SpecialStream::None | SpecialStream::Memory | SpecialStream::Temp => {
            // CLI: the descriptor may be (a dup of) descriptor 1; what was
            // printed before goes out first.
            vm.flush_cli_output();
            FILE_HANDLES.with(|h| {
                let mut h = h.borrow_mut();
                if let Some(fh) = h.get_mut(&fid) {
                    // For append mode, writes always go to the end of the file.
                    // PHP/POSIX 'a' / 'a+' mode requires this behavior. std::fs::OpenOptions
                    // .append(true) on Linux already enforces it via O_APPEND for real
                    // files, but tempfile-backed php://temp / php://memory streams need
                    // an explicit seek since they are opened with .read+.write.
                    let mode_chars: String = fh
                        .mode
                        .chars()
                        .filter(|&c| c != 'b' && c != 't' && c != 'e')
                        .collect();
                    if mode_chars.starts_with('a') {
                        let _ = fh.file.seek(SeekFrom::End(0));
                    }
                    // php_stream_temp_write(): reaching the limit moves the
                    // data to a temporary file stream.
                    if let Some(max) = fh.temp_max {
                        let len = fh.file.metadata().map(|m| m.len()).unwrap_or(0);
                        if len + bytes.len() as u64 >= max {
                            fh.temp_max = None;
                            goro_core::value::alloc_resource_id();
                        }
                    }
                    fh.file.write(bytes).unwrap_or_default()
                } else {
                    0
                }
            })
        }
    }
}

fn fread_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let r = fread_fn_inner(vm, args);
    xp_socket::flush_crypto_warnings(vm, "fread");
    r
}

fn fread_fn_inner(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let arg = args.first().unwrap_or(&Value::Null);
    let fid = require_open_stream(vm, arg, &called_as(vm, "fread"))?;
    let length_signed = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    if length_signed <= 0 {
        let msg = format!(
            "{}(): Argument #2 ($length) must be greater than 0",
            called_as(vm, "fread")
        );
        let exc = vm.create_exception(b"ValueError", &msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    let length = length_signed as usize;
    // If read filters are attached, route through filter chain.
    let has_read_filters =
        crate::stream_filters::chain_has_direction(fid, crate::stream_filters::STREAM_FILTER_READ);
    if has_read_filters {
        return Ok(match read_filtered_ex(vm, fid, length) {
            Some(d) => Value::String(PhpString::from_vec(d)),
            None => Value::False,
        });
    }
    // Detect write-only mode (w/a/x/c without '+')
    let is_write_only = FILE_HANDLES.with(|h| {
        h.borrow()
            .get(&fid)
            .map(|fh| {
                let mode = &fh.mode;
                // Strip 'b'/'t'/'e' suffixes
                let core: String = mode
                    .chars()
                    .filter(|c| matches!(*c, 'r' | 'w' | 'a' | 'x' | 'c' | '+'))
                    .collect();
                // Write-only if mode does not contain 'r' or '+'
                !core.contains('r') && !core.contains('+')
            })
            .unwrap_or(false)
    });
    let result = FILE_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(fh) = handles.get_mut(&fid) {
            // _php_stream_read(): plain files and memory streams read until
            // the request is satisfied; other streams return after one read.
            let greedy = fh.is_greedy();
            let mut buf = vec![0u8; length.min(1 << 20)];
            let mut got = 0usize;
            loop {
                if got == buf.len() {
                    if got >= length {
                        break;
                    }
                    buf.resize((got * 2).min(length), 0);
                }
                match fh.read_raw(&mut buf[got..], got > 0) {
                    Ok(0) => {
                        fh.eof = true;
                        break;
                    }
                    Ok(n) => {
                        got += n;
                        if got >= length || !greedy {
                            break;
                        }
                    }
                    // Non-blocking stream without data: not an error.
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(_) if got > 0 => break,
                    Err(_) if fh.sock.is_some() => return Ok(Value::False),
                    Err(e) => return Err(e.raw_os_error().unwrap_or(0)),
                }
            }
            buf.truncate(got);
            Ok(Value::String(PhpString::from_vec(buf)))
        } else {
            Ok(Value::False)
        }
    });
    match result {
        Ok(v) => Ok(v),
        Err(errno) => {
            // php_stdiop_read(): a failed read() is reported (with the
            // 8192-byte buffer size PHP reads in, not the requested length);
            // anything but EBADF also marks the stream at EOF (a directory
            // opened as a file: EISDIR).
            if is_write_only || (errno != 9 && errno > 0) {
                if errno != 9 {
                    FILE_HANDLES.with(|h| {
                        if let Some(fh) = h.borrow_mut().get_mut(&fid) {
                            fh.eof = true;
                        }
                    });
                }
                let fname = vm
                    .current_builtin_name()
                    .unwrap_or_else(|| "fread".to_string());
                let line = vm.current_line;
                vm.emit_notice_at(
                    &format!(
                        "{}(): Read of 8192 bytes failed with errno={} {}",
                        fname,
                        errno,
                        match errno {
                            9 => "Bad file descriptor".to_string(),
                            e => std::io::Error::from_raw_os_error(e)
                                .to_string()
                                .split(" (os error")
                                .next()
                                .unwrap_or("Unknown error")
                                .to_string(),
                        }
                    ),
                    line,
                );
            }
            Ok(Value::False)
        }
    }
}

/// _php_stream_fill_read_buffer() for a stream with read filters: read
/// chunks and run them through the chain until `size` bytes are buffered
/// (or EOF). false == FAILURE.
fn fill_read_buffer_filtered(vm: &mut Vm, fid: i64, size: usize) -> bool {
    let to_read_now = size.min(STREAM_CHUNK_SIZE);
    loop {
        let (eof, buffered) = FILE_HANDLES.with(|h| {
            h.borrow()
                .get(&fid)
                .map(|fh| (fh.eof, fh.filter_read_ready.len()))
                .unwrap_or((true, 0))
        });
        if eof || buffered >= to_read_now {
            return true;
        }
        // ops->read() of one chunk
        let mut raw = vec![0u8; STREAM_CHUNK_SIZE];
        let (justread, eof_now) = FILE_HANDLES.with(|h| {
            let mut h = h.borrow_mut();
            let Some(fh) = h.get_mut(&fid) else {
                return (-1isize, true);
            };
            let r = match fh.read_direct(&mut raw, false) {
                Ok(0) => {
                    fh.eof = true;
                    0
                }
                Ok(n) => {
                    // php://temp reads its inner memory stream greedily, so
                    // a short read reaches its end.
                    if fh.special == SpecialStream::Temp && n < STREAM_CHUNK_SIZE {
                        fh.eof = true;
                    }
                    n as isize
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => 0,
                Err(_) => -1,
            };
            (r, fh.eof)
        });
        if justread < 0 && buffered == 0 {
            return false;
        }
        let flags = if justread > 0 {
            if eof_now {
                crate::stream_filters::PSFS_FLAG_FLUSH_CLOSE
            } else {
                crate::stream_filters::PSFS_FLAG_NORMAL
            }
        } else if eof_now {
            crate::stream_filters::PSFS_FLAG_FLUSH_CLOSE
        } else {
            crate::stream_filters::PSFS_FLAG_FLUSH_INC
        };
        let data = if justread > 0 {
            Some(&raw[..justread as usize])
        } else {
            None
        };
        let r = crate::stream_filters::run_read_chain(vm, fid, data, flags);
        match r.status {
            crate::stream_filters::PSFS_PASS_ON => FILE_HANDLES.with(|h| {
                if let Some(fh) = h.borrow_mut().get_mut(&fid) {
                    fh.filter_read_ready.extend_from_slice(&r.out);
                }
            }),
            crate::stream_filters::PSFS_FEED_ME => {}
            _ => {
                FILE_HANDLES.with(|h| {
                    if let Some(fh) = h.borrow_mut().get_mut(&fid) {
                        fh.eof = true;
                        fh.fatal_error = true;
                    }
                });
                return false;
            }
        }
        if justread <= 0 {
            return true;
        }
    }
}

/// _php_stream_read() on a stream with read filters. None when the read
/// failed before anything was read.
fn read_filtered_ex(vm: &mut Vm, fid: i64, length: usize) -> Option<Vec<u8>> {
    let take = |fid: i64, out: &mut Vec<u8>, want: usize| -> usize {
        FILE_HANDLES.with(|h| {
            let mut h = h.borrow_mut();
            let Some(fh) = h.get_mut(&fid) else {
                return 0;
            };
            let n = want.min(fh.filter_read_ready.len());
            out.extend(fh.filter_read_ready.drain(..n));
            n
        })
    };
    let greedy = FILE_HANDLES.with(|h| {
        h.borrow()
            .get(&fid)
            .map(|fh| fh.is_greedy())
            .unwrap_or(false)
    });
    let mut out = Vec::new();
    take(fid, &mut out, length);
    while out.len() < length {
        if !fill_read_buffer_filtered(vm, fid, length - out.len()) {
            if out.is_empty() {
                return None;
            }
            break;
        }
        let want = length - out.len();
        if take(fid, &mut out, want) == 0 {
            break;
        }
        if !greedy {
            break;
        }
    }
    Some(out)
}

fn read_filtered(vm: &mut Vm, fid: i64, length: usize) -> Vec<u8> {
    read_filtered_ex(vm, fid, length).unwrap_or_default()
}

/// _php_stream_flush(): push the write chain (FLUSH_INC or FLUSH_CLOSE) to
/// the stream.
fn flush_write_filters(vm: &mut Vm, fid: i64, closing: bool) {
    if !crate::stream_filters::chain_has_direction(fid, crate::stream_filters::STREAM_FILTER_WRITE)
    {
        return;
    }
    let flags = if closing {
        crate::stream_filters::PSFS_FLAG_FLUSH_CLOSE
    } else {
        crate::stream_filters::PSFS_FLAG_FLUSH_INC
    };
    let (_, out) = crate::stream_filters::run_write_chain(vm, fid, None, flags);
    if !out.is_empty() {
        write_unfiltered(vm, fid, &out);
    }
}

/// _php_stream_write_buffer(): write bytes past the filters.
fn write_unfiltered(vm: &mut Vm, fid: i64, data: &[u8]) -> isize {
    if xp_socket::is_socket(fid) {
        xp_socket::sock_write(vm, "fwrite", fid, data)
    } else {
        write_to_stream(vm, fid, data) as isize
    }
}

/// Stream position helpers for the "consumed" filter.
pub(crate) fn filter_stream_tell(fid: i64) -> Option<i64> {
    FILE_HANDLES.with(|h| {
        let mut h = h.borrow_mut();
        let fh = h.get_mut(&fid)?;
        let pos = fh.file.stream_position().ok()?;
        Some(pos as i64 - fh.filter_read_ready.len() as i64)
    })
}

pub(crate) fn filter_stream_seek(fid: i64, pos: i64) {
    FILE_HANDLES.with(|h| {
        if let Some(fh) = h.borrow_mut().get_mut(&fid) {
            if fh.seekable && pos >= 0 {
                fh.filter_read_ready.clear();
                let _ = fh.file.seek(SeekFrom::Start(pos as u64));
                fh.eof = false;
            }
        }
    });
}

/// Allocate a resource id from the shared stream/resource counter.
pub(crate) fn alloc_resource_id() -> i64 {
    goro_core::value::alloc_resource_id()
}

fn fwrite_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let fid = args.first().map(|v| v.to_long()).unwrap_or(0);
    let before = FILE_HANDLES.with(|h| {
        h.borrow_mut()
            .get_mut(&fid)
            .and_then(|fh| fh.append_position())
    });
    let r = fwrite_fn_impl(vm, args)?;
    if let (Value::Long(n), Some(before)) = (&r, before) {
        FILE_HANDLES.with(|h| {
            if let Some(fh) = h.borrow_mut().get_mut(&fid) {
                if let Some(os) = fh.logical_os_pos() {
                    fh.append_pos = Some((before + (*n).max(0) as u64, os));
                }
            }
        });
    }
    Ok(r)
}

fn fwrite_fn_impl(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let arg = args.first().unwrap_or(&Value::Null);
    // For STDOUT/STDERR (1/2/3) without FILE_HANDLES entry, accept long IDs.
    let raw_fid = arg.to_long();
    let is_special_writer =
        matches!(arg.deref(), Value::Resource(_)) && (raw_fid == 1 || raw_fid == 2 || raw_fid == 3);
    let fid = if is_special_writer {
        raw_fid
    } else {
        require_open_stream(vm, arg, &called_as(vm, "fwrite"))?
    };
    let data = match args.get(1) {
        Some(v @ Value::Object(_)) => vm.value_to_string_checked(v)?,
        Some(v) => v.to_php_string(),
        None => PhpString::from_bytes(b""),
    };
    let bytes = data.as_bytes();
    // PHP: if $length is provided and >= 0, write min(length, strlen(data)) bytes.
    // If $length is negative, write 0 bytes (treat as 0).
    let length: usize = match args.get(2) {
        Some(v) if !matches!(v, Value::Null | Value::Undef) => {
            let n = v.to_long();
            if n <= 0 {
                0
            } else {
                (n as usize).min(bytes.len())
            }
        }
        _ => bytes.len(),
    };
    let to_write = &bytes[..length];
    let original_len = to_write.len();
    // PHP_FUNCTION(fwrite): nothing to write is not a write.
    if original_len == 0 {
        return Ok(Value::Long(0));
    }

    // Check if the underlying stream allows writing.
    let (stream_mode, is_special) = FILE_HANDLES.with(|h| {
        let b = h.borrow();
        match b.get(&fid) {
            // A dup()ed descriptor (php://stdout opened "r") writes to
            // whatever the descriptor allows.
            Some(fh) if fh.is_dup_descriptor() => (None, false),
            Some(fh) => (
                Some(fh.mode.clone()),
                !matches!(fh.special, SpecialStream::None),
            ),
            None => (None, false),
        }
    });
    if let Some(ref m) = stream_mode {
        let clean: String = m.chars().filter(|&c| c != 'b' && c != 't').collect();
        // 'r' (read-only) without '+' is not writable.
        if clean == "r" {
            if !is_special {
                let line = vm.current_line;
                vm.emit_notice_at(
                    &format!(
                        "fwrite(): Write of {} bytes failed with errno=9 Bad file descriptor",
                        original_len
                    ),
                    line,
                );
            }
            return Ok(Value::False);
        }
    }

    // Streams without a write op (data:): "Stream is not writable".
    if FILE_HANDLES.with(|h| {
        h.borrow()
            .get(&fid)
            .map(|fh| fh.data_meta.is_some())
            .unwrap_or(false)
    }) {
        let line = vm.current_line;
        vm.emit_notice_at("fwrite(): Stream is not writable", line);
        return Ok(Value::False);
    }
    // _php_stream_write(): invalidate the read buffer of a seekable stream
    // so the data lands at the logical position.
    FILE_HANDLES.with(|h| {
        if let Some(fh) = h.borrow_mut().get_mut(&fid) {
            if fh.seekable && !fh.filter_read_ready.is_empty() {
                let n = std::mem::take(&mut fh.filter_read_ready).len() as i64;
                let _ = fh.file.seek(SeekFrom::Current(-n));
            }
        }
    });
    // Socket streams: php_sockop_write() reports partial / failed writes.
    if xp_socket::is_socket(fid)
        && !crate::stream_filters::chain_has_direction(
            fid,
            crate::stream_filters::STREAM_FILTER_WRITE,
        )
    {
        if to_write.is_empty() {
            return Ok(Value::Long(0));
        }
        let n = xp_socket::sock_write(vm, "fwrite", fid, to_write);
        return Ok(if n < 0 {
            Value::False
        } else {
            Value::Long(n as i64)
        });
    }
    // _php_stream_write_filtered(): fwrite() reports what the first filter
    // consumed, false on PSFS_ERR_FATAL.
    if crate::stream_filters::chain_has_direction(fid, crate::stream_filters::STREAM_FILTER_WRITE) {
        if to_write.is_empty() {
            return Ok(Value::Long(0));
        }
        let (consumed, out) = crate::stream_filters::run_write_chain(
            vm,
            fid,
            Some(to_write),
            crate::stream_filters::PSFS_FLAG_NORMAL,
        );
        let mut ok = consumed >= 0;
        if !out.is_empty() && write_unfiltered(vm, fid, &out) < 0 {
            ok = false;
        }
        if vm.current_exception.is_some() {
            return Err(VmError {
                message: String::new(),
                line: vm.current_line,
            });
        }
        return Ok(if ok {
            Value::Long(consumed as i64)
        } else {
            Value::False
        });
    }
    let filtered = to_write.to_vec();
    // STDOUT/STDERR are resource ids 1/2/3 without a FILE_HANDLES entry.
    let exists =
        fid == 1 || fid == 2 || fid == 3 || FILE_HANDLES.with(|h| h.borrow().contains_key(&fid));
    if !exists {
        return Ok(Value::False);
    }
    write_to_stream(vm, fid, &filtered);
    // PHP returns the number of bytes from the user's input that were
    // accepted, regardless of how many post-filter bytes were emitted.
    Ok(Value::Long(original_len as i64))
}

/// Whether `fid` names an open stream handle.
pub(crate) fn has_file_handle(fid: i64) -> bool {
    FILE_HANDLES.with(|h| h.borrow().contains_key(&fid))
}

/// php_stream->mode of an open stream.
pub(crate) fn stream_raw_mode(fid: i64) -> Option<String> {
    FILE_HANDLES.with(|h| h.borrow().get(&fid).map(|fh| fh.mode.clone()))
}

fn stream_mode_clean(fid: i64) -> Option<String> {
    FILE_HANDLES.with(|h| {
        h.borrow()
            .get(&fid)
            .map(|fh| fh.mode.chars().filter(|&c| c != 'b' && c != 't').collect())
    })
}

/// `php_stream_get_line(stream, NULL/buf, maxlen + 1, &len)` for the
/// standard functions built on it (fgetcsv, fscanf): reads one line
/// (through the regular fgets() path), or at most `maxlen` bytes.
/// Read errors on write-only streams are reported under `func`.
pub(crate) fn stream_get_line(
    vm: &mut Vm,
    func: &str,
    handle: &Value,
    maxlen: Option<usize>,
) -> Option<Vec<u8>> {
    let fid = handle.to_long();
    if let Some(m) = stream_mode_clean(fid) {
        if matches!(m.as_str(), "w" | "a" | "x" | "c") {
            let line = vm.current_line;
            vm.emit_notice_at(
                &format!(
                    "{}(): Read of 8192 bytes failed with errno=9 Bad file descriptor",
                    func
                ),
                line,
            );
            return None;
        }
    }
    let mut a = vec![handle.clone()];
    if let Some(n) = maxlen {
        a.push(Value::Long((n as i64).saturating_add(1)));
    }
    match fgets_fn(vm, &a) {
        Ok(Value::String(s)) => Some(s.as_bytes().to_vec()),
        _ => None,
    }
}

/// `php_stream_write()` for functions built on it (fputcsv). Returns the
/// number of bytes written, or None on failure (write errors on read-only
/// streams are reported under `func`).
pub(crate) fn stream_write(vm: &mut Vm, func: &str, handle: &Value, data: &[u8]) -> Option<i64> {
    let fid = handle.to_long();
    let dup_fd = FILE_HANDLES.with(|h| {
        h.borrow()
            .get(&fid)
            .is_some_and(|fh| fh.is_dup_descriptor())
    });
    if let Some(m) = stream_mode_clean(fid).filter(|_| !dup_fd) {
        let is_special = FILE_HANDLES.with(|h| {
            h.borrow()
                .get(&fid)
                .map(|fh| !matches!(fh.special, SpecialStream::None))
                .unwrap_or(false)
        });
        if m == "r" {
            if !is_special {
                let line = vm.current_line;
                vm.emit_notice_at(
                    &format!(
                        "{}(): Write of {} bytes failed with errno=9 Bad file descriptor",
                        func,
                        data.len()
                    ),
                    line,
                );
            }
            return None;
        }
    }
    match fwrite_fn(
        vm,
        &[handle.clone(), Value::String(PhpString::from_bytes(data))],
    ) {
        Ok(Value::Long(n)) => Some(n),
        _ => None,
    }
}

fn fgets_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let r = fgets_fn_inner(vm, args);
    xp_socket::flush_crypto_warnings(vm, "fgets");
    r
}

fn fgets_fn_inner(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let arg = args.first().unwrap_or(&Value::Null);
    let fid = require_open_stream(vm, arg, &called_as(vm, "fgets"))?;
    // PHP semantics: $length (if provided) must be > 0. fgets reads up to
    // length-1 bytes (the remaining byte is a virtual null terminator in C).
    // When omitted, reads until newline (no byte cap — we use a large default).
    let explicit_length = match args.get(1) {
        Some(v) if !matches!(v, Value::Null | Value::Undef) => Some(v.to_long()),
        _ => None,
    };
    let limit = if let Some(n) = explicit_length {
        if n <= 0 {
            let msg = format!(
                "{}(): Argument #2 ($length) must be greater than 0",
                called_as(vm, "fgets")
            );
            let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
        (n as usize).saturating_sub(1)
    } else {
        usize::MAX
    };
    // Special: $length == 1 means read 0 bytes - PHP returns "" or false.
    if limit == 0 {
        // For length=1 (limit=0), PHP returns false (after reading something it would be empty).
        return Ok(Value::False);
    }
    // Emit notice and return false if underlying stream is write-only.
    let stream_mode = FILE_HANDLES.with(|h| h.borrow().get(&fid).map(|fh| fh.mode.clone()));
    if let Some(m) = stream_mode {
        let clean: String = m.chars().filter(|&c| c != 'b' && c != 't').collect();
        if matches!(clean.as_str(), "w" | "a" | "x" | "c") {
            let line = vm.current_line;
            vm.emit_notice_at(
                "fgets(): Read of 8192 bytes failed with errno=9 Bad file descriptor",
                line,
            );
            return Ok(Value::False);
        }
    }

    let has_read_filters =
        crate::stream_filters::chain_has_direction(fid, crate::stream_filters::STREAM_FILTER_READ);
    if has_read_filters {
        let mut out: Vec<u8> = Vec::new();
        let mut hit_newline = false;
        while out.len() < limit {
            let chunk = match read_filtered_ex(vm, fid, 1) {
                Some(c) => c,
                // A filter failed: _php_stream_get_line() returns NULL.
                None if stream_fatal_error(fid) => return Ok(Value::False),
                None => Vec::new(),
            };
            if chunk.is_empty() {
                if stream_fatal_error(fid) {
                    return Ok(Value::False);
                }
                break;
            }
            out.extend_from_slice(&chunk);
            if *chunk.last().unwrap() == b'\n' {
                hit_newline = true;
                break;
            }
        }
        let is_eof_now = FILE_HANDLES.with(|h| {
            h.borrow()
                .get(&fid)
                .map(|fh| fh.eof && fh.filter_read_ready.is_empty())
                .unwrap_or(true)
        });
        if out.is_empty() && is_eof_now {
            return Ok(Value::False);
        }
        let _ = hit_newline;
        return Ok(Value::String(PhpString::from_vec(out)));
    }

    let detect_eol = vm
        .constants
        .get(b"auto_detect_line_endings".as_slice())
        .map(ini_parse_bool)
        .unwrap_or(false);
    let mut read_errno = 0;
    let r = FILE_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(fh) = handles.get_mut(&fid) {
            if fh.eol == EOL_UNSET {
                fh.eol = if detect_eol { EOL_DETECT } else { EOL_LF };
            }
            let mut buf = Vec::new();
            let mut byte = [0u8; 1];
            let mut failed = false;
            if fh.eol == EOL_DETECT {
                // php_stream_locate_eol(): decide from the buffered data.
                if let Ok(1) = fh.read_raw(&mut byte, false) {
                    fh.filter_read_ready.insert(0, byte[0]);
                }
                let data = &fh.filter_read_ready;
                let cr = data.iter().position(|&c| c == b'\r');
                let lf = data.iter().position(|&c| c == b'\n');
                match (cr, lf) {
                    (Some(c), l) if l != Some(c + 1) && !l.is_some_and(|l| l < c) => {
                        fh.eol = EOL_MAC;
                    }
                    (_, Some(_)) => fh.eol = EOL_LF,
                    _ => {}
                }
            }
            let terminator = if fh.eol == EOL_MAC { b'\r' } else { b'\n' };
            // _php_stream_get_line(): a stream already at EOF with nothing
            // buffered is not read again (fread()/fgetc() would read).
            if fh.eof && fh.filter_read_ready.is_empty() && fh.sock.is_none() {
                return Ok(Value::False);
            }
            for _ in 0..limit {
                match fh.read_raw(&mut byte, false) {
                    Ok(0) => {
                        fh.eof = true;
                        break;
                    }
                    Ok(_) => {
                        buf.push(byte[0]);
                        if byte[0] == terminator {
                            break;
                        }
                    }
                    Err(e) => {
                        failed = e.kind() != std::io::ErrorKind::WouldBlock;
                        if fh.sock.is_none() {
                            if let Some(n) =
                                e.raw_os_error().filter(|n| *n != 9 && *n != libc::EAGAIN)
                            {
                                read_errno = n;
                                fh.eof = true;
                            }
                        }
                        break;
                    }
                }
            }
            // A socket read that timed out (or failed) with nothing read
            // makes fgets() fail.
            if buf.is_empty() && fh.sock.is_some() && (failed || fh.eof) {
                return Ok(Value::False);
            }
            // php://temp reads greedily from its inner memory stream, so the
            // eof flag is raised as soon as all data has been consumed.
            if !fh.eof && matches!(fh.special, SpecialStream::Temp) {
                if let (Ok(pos), Ok(meta)) = (fh.file.stream_position(), fh.file.metadata()) {
                    if pos >= meta.len() {
                        fh.eof = true;
                    }
                }
            }
            if buf.is_empty() && fh.eof {
                Ok(Value::False)
            } else {
                Ok(Value::String(PhpString::from_vec(buf)))
            }
        } else {
            Ok(Value::False)
        }
    });
    if read_errno != 0 {
        read_failed_notice(vm, "fgets", read_errno);
    }
    r
}

fn feof_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let arg = args.first().unwrap_or(&Value::Null);
    require_resource_arg(vm, arg, &called_as(vm, "feof"))?;
    let fid = arg.to_long();
    let has_handle = FILE_HANDLES.with(|handles| handles.borrow().contains_key(&fid));
    if !has_handle {
        // If it was a resource (possibly closed) or any other type, throw TypeError
        let is_closed_resource = matches!(arg, Value::Resource(_))
            && goro_core::value::is_registered_resource(fid)
            && !goro_core::value::is_open_resource(fid);
        let is_resource =
            matches!(arg, Value::Resource(_)) && goro_core::value::is_open_resource(fid);
        if is_closed_resource || !is_resource {
            let msg = format!(
                "{}(): Argument #1 ($stream) must be an open stream resource",
                called_as(vm, "feof")
            );
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
        return Ok(Value::True);
    }
    FILE_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(fh) = handles.get_mut(&fid) {
            // _php_stream_eof(): buffered data means not at EOF; sockets
            // check whether the peer is still alive.
            if !fh.filter_read_ready.is_empty() {
                return Ok(Value::False);
            }
            if !fh.eof && fh.sock.is_some() {
                xp_socket::check_liveness(fh);
            }
            if fh.eof {
                Ok(Value::True)
            } else {
                Ok(Value::False)
            }
        } else {
            Ok(Value::True)
        }
    })
}

fn rewind_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let r = rewind_fn_impl(vm, args)?;
    resync_append_pos(args.first().map(|v| v.to_long()).unwrap_or(0));
    Ok(r)
}

/// A seek moved an append-mode file: its position is the offset again.
fn resync_append_pos(fid: i64) {
    FILE_HANDLES.with(|h| {
        if let Some(fh) = h.borrow_mut().get_mut(&fid) {
            if fh.append_pos.is_some() {
                fh.append_pos = fh.logical_os_pos().map(|p| (p, p));
            }
        }
    });
}

fn rewind_fn_impl(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let arg = args.first().unwrap_or(&Value::Null);
    let fid = require_open_stream(vm, arg, &called_as(vm, "rewind"))?;
    flush_write_filters(vm, fid, false);
    FILE_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(fh) = handles.get_mut(&fid) {
            fh.eof = false;
            fh.filter_read_ready.clear();
            match fh.file.seek(SeekFrom::Start(0)) {
                Ok(_) => Ok(Value::True),
                Err(_) => Ok(Value::False),
            }
        } else {
            Ok(Value::False)
        }
    })
}

fn is_dir_handle(v: &Value) -> bool {
    matches!(v.deref(), Value::Resource(id) if DIR_HANDLES.with(|dh| dh.borrow().contains_key(&id.id())))
}

fn fseek_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let arg = args.first().unwrap_or(&Value::Null);
    // A directory stream's seek rewinds it (php_plain_files_dirstream_rewind).
    if is_dir_handle(arg) {
        rewinddir_fn(vm, std::slice::from_ref(arg))?;
        return Ok(Value::Long(0));
    }
    let r = fseek_fn_impl(vm, args)?;
    if matches!(r, Value::Long(0)) {
        resync_append_pos(arg.to_long());
    }
    Ok(r)
}

fn fseek_fn_impl(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let arg = args.first().unwrap_or(&Value::Null);
    let fid = require_open_stream(vm, arg, &called_as(vm, "fseek"))?;
    let offset = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let whence = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    // Invalid whence values return -1 without seeking.
    if !matches!(whence, 0..=2) {
        return Ok(Value::Long(-1));
    }
    flush_write_filters(vm, fid, false);

    let result = FILE_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(fh) = handles.get_mut(&fid) {
            // Track whether this is a special stream that doesn't support seek.
            // PHP_STREAM_FLAG_NO_SEEK: also a descriptor stream over a pipe.
            let is_unseekable_special = matches!(
                fh.special,
                SpecialStream::Stdout | SpecialStream::Stderr | SpecialStream::Output
            ) || (fh.is_dup_descriptor() && !fh.seekable);
            fh.eof = false;
            // The read buffer is discarded; SEEK_CUR is relative to the
            // logical position (before the buffered bytes).
            // (_php_stream_seek(): SEEK_CUR is relative to stream->position)
            let position = fh.append_pos.and_then(|_| fh.append_position());
            let buffered = std::mem::take(&mut fh.filter_read_ready).len() as i64;
            let seek = match whence {
                0 => SeekFrom::Start(offset as u64), // SEEK_SET
                1 if position.is_some() => {
                    let to = position.unwrap_or(0) as i64 + offset;
                    if to < 0 {
                        return Ok(-1);
                    }
                    SeekFrom::Start(to as u64)
                }
                1 => SeekFrom::Current(offset - buffered), // SEEK_CUR
                2 => SeekFrom::End(offset),                // SEEK_END
                _ => SeekFrom::Start(offset as u64),
            };
            if is_unseekable_special {
                return Err(true); // emit warning
            }
            match fh.file.seek(seek) {
                Ok(_) => Ok(0i64),
                Err(_) => Ok(-1),
            }
        } else {
            Err(false)
        }
    });
    match result {
        Ok(v) => Ok(Value::Long(v)),
        Err(true) => {
            vm.emit_warning("fseek(): Stream does not support seeking");
            Ok(Value::Long(-1))
        }
        Err(false) => Ok(Value::Long(-1)),
    }
}

fn ftell_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let arg = args.first().unwrap_or(&Value::Null);
    require_resource_arg(vm, arg, &called_as(vm, "ftell"))?;
    let fid = arg.to_long();
    let has_handle = FILE_HANDLES.with(|handles| handles.borrow().contains_key(&fid));
    if !has_handle {
        let is_closed_resource = matches!(arg, Value::Resource(_))
            && goro_core::value::is_registered_resource(fid)
            && !goro_core::value::is_open_resource(fid);
        let is_open_resource =
            matches!(arg, Value::Resource(_)) && goro_core::value::is_open_resource(fid);
        if is_closed_resource || !is_open_resource {
            let msg = format!(
                "{}(): Argument #1 ($stream) must be an open stream resource",
                called_as(vm, "ftell")
            );
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
        return Ok(Value::False);
    }
    FILE_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(fh) = handles.get_mut(&fid) {
            if fh.append_pos.is_some() {
                if let Some(p) = fh.append_position() {
                    return Ok(Value::Long(p as i64));
                }
            }
            let buffered = fh.filter_read_ready.len() as u64;
            if let Some(p) = fh.pipe_pos {
                // PHP_FUNCTION(ftell): a position of -1 is reported as false.
                let pos = p - buffered as i64;
                return Ok(if pos == -1 {
                    Value::False
                } else {
                    Value::Long(pos)
                });
            }
            match fh.file.stream_position() {
                Ok(pos) => Ok(Value::Long(pos.saturating_sub(buffered) as i64)),
                Err(_) => Ok(Value::False),
            }
        } else {
            Ok(Value::False)
        }
    })
}

fn fflush_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    require_resource_arg(
        vm,
        args.first().unwrap_or(&Value::Null),
        &called_as(vm, "fflush"),
    )?;
    let fid = args.first().unwrap_or(&Value::Null).to_long();
    flush_write_filters(vm, fid, false);
    FILE_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(fh) = handles.get_mut(&fid) {
            match fh.file.flush() {
                Ok(_) => Ok(Value::True),
                Err(_) => Ok(Value::False),
            }
        } else {
            Ok(Value::False)
        }
    })
}

fn fsync_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let arg = args.first().unwrap_or(&Value::Null);
    let fid = require_open_stream(vm, arg, "fsync")?;
    enum Result3 {
        Ok,
        Err,
        NoFsync,
    }
    let result = FILE_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(fh) = handles.get_mut(&fid) {
            // Memory/Temp/Output have no real backing fd; cannot fsync.
            if matches!(
                fh.special,
                SpecialStream::Memory | SpecialStream::Temp | SpecialStream::Output
            ) {
                return Result3::NoFsync;
            }
            // Stdin can't be fsynced (real fd 0 returns EINVAL).
            if matches!(fh.special, SpecialStream::Stdin) {
                return Result3::Err;
            }
            // Stdout/Stderr are real streams but typically not fsync-able.
            match fh.file.sync_all() {
                Ok(_) => Result3::Ok,
                Err(_) => Result3::Err,
            }
        } else {
            Result3::NoFsync
        }
    });
    match result {
        Result3::Ok => Ok(Value::True),
        Result3::Err => Ok(Value::False),
        Result3::NoFsync => {
            vm.emit_warning("fsync(): Can't fsync this stream!");
            Ok(Value::False)
        }
    }
}

fn fdatasync_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let arg = args.first().unwrap_or(&Value::Null);
    let fid = require_open_stream(vm, arg, "fdatasync")?;
    enum Result3 {
        Ok,
        Err,
        NoFsync,
    }
    let result = FILE_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(fh) = handles.get_mut(&fid) {
            if matches!(
                fh.special,
                SpecialStream::Memory | SpecialStream::Temp | SpecialStream::Output
            ) {
                return Result3::NoFsync;
            }
            if matches!(fh.special, SpecialStream::Stdin) {
                return Result3::Err;
            }
            match fh.file.sync_data() {
                Ok(_) => Result3::Ok,
                Err(_) => Result3::Err,
            }
        } else {
            Result3::NoFsync
        }
    });
    match result {
        Result3::Ok => Ok(Value::True),
        Result3::Err => Ok(Value::False),
        Result3::NoFsync => {
            vm.emit_warning("fdatasync(): Can't fsync this stream!");
            Ok(Value::False)
        }
    }
}

fn mkdir_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path_v = args.first().unwrap_or(&Value::Null).to_php_string();
    // Null byte check (PHP raises ValueError as TypeError message)
    if path_v.as_bytes().contains(&0) {
        let msg = "mkdir(): Argument #1 ($directory) must not contain any null bytes".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    let path = path_v.to_string_lossy().to_string();
    let path = strip_file_scheme(&path).unwrap_or(path);
    let recursive = args.get(2).map(|v| v.is_truthy()).unwrap_or(false);
    // php_plain_files_mkdir(): a recursive mkdir of an existing path fails
    // like a plain one ("File exists").
    // mkdir(2) with the requested mode (the umask applies), for each missing
    // component when recursive.
    let mode = args
        .get(1)
        .filter(|v| !matches!(v, Value::Null | Value::Undef))
        .map(|v| v.to_long())
        .unwrap_or(0o777) as u32;
    let result = {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(recursive && !std::path::Path::new(&path).exists())
            .mode(mode & 0o7777)
            .create(&path)
    };
    match result {
        Ok(_) => Ok(Value::True),
        Err(e) => {
            let msg = strip_os_error(&e.to_string());
            vm.emit_warning(&format!("mkdir(): {}", msg));
            Ok(Value::False)
        }
    }
}

fn rmdir_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // php_clear_stat_cache(1, ...): the realpath cache is dropped.
    REALPATH_CACHE.with(|c| c.borrow_mut().clear());
    let path_v = args.first().unwrap_or(&Value::Null).to_php_string();
    if path_v.as_bytes().contains(&0) {
        let msg = "rmdir(): Argument #1 ($directory) must not contain any null bytes".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    let path = path_v.to_string_lossy().to_string();
    // Support file:// scheme for tests.
    let path = if let Some(rest) = path.strip_prefix("file://") {
        rest.to_string()
    } else {
        path
    };
    match std::fs::remove_dir(&path) {
        Ok(_) => Ok(Value::True),
        Err(e) => {
            let msg = strip_os_error(&e.to_string());
            vm.emit_warning(&format!("rmdir({}): {}", path, msg));
            Ok(Value::False)
        }
    }
}
fn glob_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let pattern = args.first().unwrap_or(&Value::Null).to_php_string();
    // Z_PARAM_PATH
    if pattern.as_bytes().contains(&0) {
        let msg = "glob(): Argument #1 ($pattern) must not contain any null bytes";
        let exc = _vm.create_exception(b"ValueError", msg, _vm.current_line);
        _vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg.to_string(),
            line: _vm.current_line,
        });
    }
    let flags = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    // php_glob.h's flags (PHP's own glob implementation).
    const G_ERR: i64 = 0x0004;
    const G_MARK: i64 = 0x0008;
    const G_NOCHECK: i64 = 0x0010;
    const G_NOSORT: i64 = 0x0020;
    const G_BRACE: i64 = 0x0080;
    const G_NOESCAPE: i64 = 0x1000;
    const G_ONLYDIR: i64 = 1 << 30;
    const AVAILABLE: i64 = G_BRACE | G_MARK | G_NOSORT | G_NOCHECK | G_NOESCAPE | G_ERR | G_ONLYDIR;
    if pattern.len() >= 4096 {
        _vm.emit_warning(&format!(
            "glob(): Pattern exceeds the maximum allowed length of {} characters",
            4096
        ));
        return Ok(Value::False);
    }
    if AVAILABLE & flags != flags {
        _vm.emit_warning(
            "glob(): At least one of the passed flags is invalid or not supported on this platform",
        );
        return Ok(Value::False);
    }
    let pattern_str = pattern.to_string_lossy();

    // Use libc glob
    let mut glob_buf: libc::glob_t = unsafe { std::mem::zeroed() };
    let c_pattern = std::ffi::CString::new(pattern_str.as_bytes()).unwrap_or_default();

    let mut libc_flags = 0i32;
    for (php, c) in [
        (G_ERR, libc::GLOB_ERR),
        (G_MARK, libc::GLOB_MARK),
        (G_NOSORT, libc::GLOB_NOSORT),
        (G_NOESCAPE, libc::GLOB_NOESCAPE),
    ] {
        if flags & php != 0 {
            libc_flags |= c;
        }
    }
    #[cfg(target_os = "linux")]
    if flags & G_BRACE != 0 {
        libc_flags |= libc::GLOB_BRACE;
    }
    #[cfg(target_os = "linux")]
    if flags & G_ONLYDIR != 0 {
        libc_flags |= libc::GLOB_ONLYDIR;
    }

    let ret = unsafe { libc::glob(c_pattern.as_ptr(), libc_flags, None, &mut glob_buf) };

    // GLOB_ONLYDIR is a hint to the OS; on Linux it can still return
    // non-directories. PHP enforces it explicitly.
    let only_dir = flags & G_ONLYDIR != 0;

    let mut arr = PhpArray::new();
    if ret == 0 {
        for i in 0..glob_buf.gl_pathc {
            let path = unsafe {
                let p = *glob_buf.gl_pathv.add(i);
                std::ffi::CStr::from_ptr(p).to_string_lossy().to_string()
            };
            if only_dir {
                // Strip trailing slash before testing is_dir (so GLOB_MARK
                // entries still work).
                let test_path = path.trim_end_matches('/');
                if !std::fs::metadata(test_path)
                    .map(|m| m.is_dir())
                    .unwrap_or(false)
                {
                    continue;
                }
            }
            // (php_glob(): GLOB_MARK does not add a second slash to a
            // match that already ends in one, as glibc's glob() does)
            let path = if libc_flags & libc::GLOB_MARK != 0 && path.ends_with("//") {
                path[..path.len() - 1].to_string()
            } else {
                path
            };
            arr.push(Value::String(PhpString::from_string(path)));
        }
    }
    unsafe {
        libc::globfree(&mut glob_buf);
    }

    // GLOB_NOCHECK: if no matches, return the pattern itself
    if ret != 0 && flags & G_NOCHECK != 0 {
        arr.push(Value::String(PhpString::from_string(
            pattern_str.to_string(),
        )));
    }

    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}
fn scandir_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let dir = args.first().unwrap_or(&Value::Null).to_php_string();
    let dir_str = dir.to_string_lossy();
    let sort_order = args.get(1).map(|v| v.to_long()).unwrap_or(0); // 0 = ascending, 1 = descending

    if dir.is_empty() {
        let msg = "scandir(): Argument #1 ($directory) must not be empty";
        let exc = vm.create_exception(b"ValueError", msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg.into(),
            line: vm.current_line,
        });
    }
    if let Some(pattern) = dir_str.strip_prefix("glob://") {
        let mut names = glob_stream_entries(vm, pattern);
        if sort_order != 0 && sort_order != 2 {
            names.sort_by(|a, b| b.cmp(a));
        } else if sort_order == 0 {
            names.sort();
        }
        let mut arr = PhpArray::new();
        for name in names {
            arr.push(Value::String(PhpString::from_string(name)));
        }
        goro_core::value::alloc_resource_id();
        return Ok(Value::Array(Rc::new(RefCell::new(arr))));
    }
    match std::fs::read_dir(&*dir_str) {
        Ok(entries) => {
            let mut names: Vec<String> = vec![".".to_string(), "..".to_string()];
            for e in entries.flatten() {
                names.push(e.file_name().to_string_lossy().to_string());
            }
            // PHP_SCANDIR_SORT_ASCENDING (0), _NONE (2), anything else descends.
            if sort_order == 0 {
                names.sort();
            } else if sort_order != 2 {
                names.sort_by(|a, b| b.cmp(a));
            }
            let mut arr = PhpArray::new();
            for name in names {
                arr.push(Value::String(PhpString::from_string(name)));
            }
            // php_stream_scandir() opened a directory stream.
            goro_core::value::alloc_resource_id();
            Ok(Value::Array(Rc::new(RefCell::new(arr))))
        }
        Err(e) => {
            let msg = strip_os_error(&e.to_string());
            vm.emit_warning(&format!(
                "scandir({}): Failed to open directory: {}",
                dir_str, msg
            ));
            vm.emit_warning(&format!(
                "scandir(): (errno {}): {}",
                e.raw_os_error().unwrap_or(0),
                msg
            ));
            Ok(Value::False)
        }
    }
}
// Thread-local storage for HTTP response headers and status code. The CLI
// server reads these after script execution.
thread_local! {
    pub(crate) static SENT_HEADERS: std::cell::RefCell<Vec<(Vec<u8>, Vec<u8>)>> = const { std::cell::RefCell::new(Vec::new()) };
    pub(crate) static RESPONSE_CODE: std::cell::Cell<i64> = const { std::cell::Cell::new(200) };
    // Request body for php://input
    pub(crate) static REQUEST_BODY: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
    static SAPI_BODY: std::cell::RefCell<(Vec<u8>, usize)> = const { std::cell::RefCell::new((Vec::new(), 0)) };
    // SG(sapi_headers).http_status_line: set by header("HTTP/...")
    pub(crate) static STATUS_LINE: std::cell::RefCell<Option<Vec<u8>>> = const { std::cell::RefCell::new(None) };
}
/// sapi_update_response_code(): a new code drops the custom status line.
fn sapi_update_response_code(code: i64) {
    if RESPONSE_CODE.with(|c| c.get()) == code {
        return;
    }
    STATUS_LINE.with(|s| *s.borrow_mut() = None);
    RESPONSE_CODE.with(|c| c.set(code));
}
/// SG(sapi_headers).http_response_code
pub fn peek_response_code() -> i64 {
    RESPONSE_CODE.with(|c| c.get())
}
/// sapi_header_op() with an "HTTP/x.y NNN ..." line (for SAPIs).
pub fn set_status_line(line: &[u8]) {
    let code = line
        .iter()
        .position(|&c| c == b' ')
        .map(|p| {
            let d: Vec<u8> = line[p + 1..]
                .iter()
                .copied()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            String::from_utf8_lossy(&d).parse::<i64>().unwrap_or(0)
        })
        .unwrap_or(200);
    sapi_update_response_code(code);
    STATUS_LINE.with(|s| *s.borrow_mut() = Some(line.to_vec()));
}
/// The status line set by header("HTTP/..."), if any (cleared).
pub fn take_status_line() -> Option<Vec<u8>> {
    STATUS_LINE.with(|s| s.borrow_mut().take())
}
/// sapi_send_headers(): the default Content-type joins the header list
/// (when the script set none).
pub fn sapi_add_default_content_type(vm: &mut Vm) {
    if !goro_core::sapi_headers::send_default_content_type() {
        return;
    }
    let get = |k: &[u8]| {
        vm.constants
            .get(k)
            .map(|v| v.to_php_string().as_bytes().to_vec())
    };
    let mimetype = get(b"default_mimetype")
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| b"text/html".to_vec());
    let charset = get(b"default_charset").unwrap_or_else(|| b"UTF-8".to_vec());
    let mut line = b"Content-type: ".to_vec();
    line.extend_from_slice(&mimetype);
    if !charset.is_empty() && mimetype.len() >= 5 && mimetype[..5].eq_ignore_ascii_case(b"text/") {
        line.extend_from_slice(b"; charset=");
        line.extend_from_slice(&charset);
    }
    sapi_add_header(&line, false);
    goro_core::sapi_headers::content_type_set(&line[14..]);
}
/// SG(request_info).request_body: the buffered request body.
pub fn set_request_body(body: Vec<u8>) {
    REQUEST_BODY.with(|b| *b.borrow_mut() = body);
}
/// SG(request_info).request_body as buffered so far.
pub fn buffered_request_body() -> Vec<u8> {
    REQUEST_BODY.with(|b| b.borrow().clone())
}
/// The SAPI's request body and how much of it was read
/// (SG(read_post_bytes)).
pub fn set_sapi_body(body: Vec<u8>, read: usize) {
    SAPI_BODY.with(|b| *b.borrow_mut() = (body, read));
}
/// sapi_read_post_block() of everything left unread.
pub fn read_sapi_body() -> Vec<u8> {
    SAPI_BODY.with(|b| {
        let mut b = b.borrow_mut();
        let pos = b.1.min(b.0.len());
        let rest = b.0[pos..].to_vec();
        b.1 = b.0.len();
        rest
    })
}
/// Records that `n` more bytes of the SAPI body were read.
pub fn advance_sapi_body(n: usize) {
    SAPI_BODY.with(|b| {
        let mut b = b.borrow_mut();
        b.1 = (b.1 + n).min(b.0.len());
    });
}
/// The unread part of the SAPI body, without consuming it.
pub fn peek_sapi_body() -> Vec<u8> {
    SAPI_BODY.with(|b| {
        let b = b.borrow();
        b.0[b.1.min(b.0.len())..].to_vec()
    })
}
/// php://input: the buffered body, plus whatever the SAPI has not
/// delivered yet (php_stream_input_read() reads it on demand).
pub fn get_request_body() -> Vec<u8> {
    let rest = read_sapi_body();
    REQUEST_BODY.with(|b| {
        let mut b = b.borrow_mut();
        b.extend_from_slice(&rest);
        b.clone()
    })
}
/// SG(sapi_headers).headers, as (name, value) pairs.
pub fn peek_sent_headers() -> Vec<(Vec<u8>, Vec<u8>)> {
    SENT_HEADERS.with(|h| h.borrow().clone())
}
/// SG(sapi_headers).http_status_line
pub fn peek_status_line() -> Option<Vec<u8>> {
    STATUS_LINE.with(|s| s.borrow().clone())
}
pub fn take_sent_headers() -> Vec<(Vec<u8>, Vec<u8>)> {
    SENT_HEADERS.with(|h| {
        let mut g = h.borrow_mut();
        std::mem::take(&mut *g)
    })
}
pub fn take_response_code() -> i64 {
    RESPONSE_CODE.with(|c| {
        let v = c.get();
        c.set(200);
        v
    })
}

/// sapi_header_op(): headers cannot be modified once output started.
/// Emits PHP's warning and returns true when the headers were already sent.
pub fn headers_already_sent_error(vm: &mut Vm) -> bool {
    if !vm.headers_sent {
        return false;
    }
    let msg = match vm.output_start.clone() {
        Some((f, l)) => format!(
            "Cannot modify header information - headers already sent by (output started at {}:{})",
            f, l
        ),
        None => "Cannot modify header information - headers already sent".to_string(),
    };
    vm.emit_warning(&msg);
    true
}

/// sapi_add_header_ex() for extensions: add a raw "Name: value" header line.
pub fn sapi_add_header(line: &[u8], replace: bool) {
    let colon = match line.iter().position(|&c| c == b':') {
        Some(c) => c,
        None => return,
    };
    let name = String::from_utf8_lossy(&line[..colon])
        .trim()
        .as_bytes()
        .to_vec();
    let mut v = &line[colon + 1..];
    while let Some((&b' ', rest)) = v.split_first() {
        v = rest;
    }
    let value = v.to_vec();
    SENT_HEADERS.with(|h| {
        let mut hh = h.borrow_mut();
        if replace {
            hh.retain(|(k, _)| !k.eq_ignore_ascii_case(&name));
        }
        hh.push((name, value));
    });
}

/// SAPI_HEADER_DELETE_PREFIX: remove headers `name` whose value starts with `value_prefix`.
pub fn sapi_delete_header_prefix(name: &[u8], value_prefix: &[u8]) {
    SENT_HEADERS.with(|h| {
        h.borrow_mut()
            .retain(|(k, v)| !(k.eq_ignore_ascii_case(name) && v.starts_with(value_prefix)));
    });
}

fn header_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if headers_already_sent_error(_vm) {
        return Ok(Value::Null);
    }
    let header = args.first().unwrap_or(&Value::Null).to_php_string();
    let replace = args.get(1).map(|v| v.is_truthy()).unwrap_or(true);
    let response_code = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    // sapi_header_op(): trailing whitespace is cut off, then the line must
    // not contain CR/LF or NUL.
    {
        let b = header.as_bytes();
        let mut len = b.len();
        while len > 0 && matches!(b[len - 1], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
            len -= 1;
        }
        for &c in &b[..len] {
            let msg = match c {
                b'\n' | b'\r' => {
                    "Header may not contain more than a single header, new line detected"
                }
                0 => "Header may not contain NUL bytes",
                _ => continue,
            };
            let line = _vm.current_line;
            _vm.emit_warning_at(msg, line);
            return Ok(Value::Null);
        }
    }
    let header_str = header.to_string_lossy();
    // Special case: "HTTP/1.x NNN ..." sets the status code and becomes the
    // response status line.
    if header.len() >= 5 && header.as_bytes()[..5].eq_ignore_ascii_case(b"HTTP/") {
        let mut line = header.as_bytes().to_vec();
        while line.last().is_some_and(|c| c.is_ascii_whitespace()) {
            line.pop();
        }
        // sapi_extract_response_code()
        let mut code = 200;
        for i in 0..line.len() {
            if line[i] == b' ' && line.get(i + 1) != Some(&b' ') {
                let digits: Vec<u8> = line[i + 1..]
                    .iter()
                    .copied()
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                code = String::from_utf8_lossy(&digits).parse::<i64>().unwrap_or(0);
                break;
            }
        }
        sapi_update_response_code(code);
        STATUS_LINE.with(|s| *s.borrow_mut() = Some(line));
        return Ok(Value::Null);
    }
    // Parse "Name: Value" or "Name:Value"
    if let Some(colon) = header_str.find(':') {
        let name = header_str[..colon].trim().as_bytes().to_vec();
        let value = header_str[colon + 1..].trim().as_bytes().to_vec();
        // PHP auto-sets 401 when WWW-Authenticate is sent without an explicit
        // status code (and the current code is still 200).
        if name.eq_ignore_ascii_case(b"WWW-Authenticate") && response_code == 0 {
            RESPONSE_CODE.with(|c| {
                if c.get() == 200 || c.get() == 0 {
                    c.set(401);
                }
            });
        }
        // PHP auto-sets 302 for Location: headers (unless code is already 3xx).
        if name.eq_ignore_ascii_case(b"Location") && response_code == 0 {
            RESPONSE_CODE.with(|c| {
                let cur = c.get();
                if cur == 200 || cur < 300 {
                    c.set(302);
                }
            });
        }
        // sapi_header_op(): SG(sapi_headers).mimetype, with the default
        // charset applied (sapi_apply_default_charset()).
        if name.eq_ignore_ascii_case(b"Content-Type") {
            let mut mimetype = value.clone();
            let charset = _vm
                .constants
                .get(b"default_charset".as_slice())
                .map(|v| v.to_php_string().as_bytes().to_vec())
                .unwrap_or_else(|| b"UTF-8".to_vec());
            if !charset.is_empty()
                && mimetype.starts_with(b"text/")
                && !mimetype.windows(8).any(|w| w == b"charset=")
            {
                mimetype.extend_from_slice(b";charset=");
                mimetype.extend_from_slice(&charset);
            }
            goro_core::sapi_headers::content_type_set(&mimetype);
        }
        // sapi_header_op(): a script setting Content-Length cannot know the
        // compressed size, so zlib output compression is switched off
        // (req #44164).
        if name.eq_ignore_ascii_case(b"Content-Length") {
            let key = b"zlib.output_compression".to_vec();
            if let Some(hook) = _vm.ini_set_hooks.get(&key).copied() {
                let zero = Value::String(PhpString::from_bytes(b"0"));
                if hook(_vm, &key, &zero) {
                    _vm.constants.insert(key, zero);
                }
            }
        }
        if replace {
            // Remove existing headers with the same name (case-insensitive)
            SENT_HEADERS.with(|h| {
                let mut hh = h.borrow_mut();
                hh.retain(|(k, _)| !k.eq_ignore_ascii_case(&name));
                hh.push((name, value));
            });
        } else {
            SENT_HEADERS.with(|h| {
                h.borrow_mut().push((name, value));
            });
        }
    }
    if response_code > 0 {
        RESPONSE_CODE.with(|c| c.set(response_code));
    }
    Ok(Value::Null)
}
/// Format a unix timestamp as "D, d M Y H:i:s GMT" (UTC).
fn cookie_gmt_date(t: i64) -> String {
    let days = t.div_euclid(86400);
    let secs = t.rem_euclid(86400);
    let wd = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"][days.rem_euclid(7) as usize];
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    let months = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!(
        "{}, {:02} {} {:04} {:02}:{:02}:{:02} GMT",
        wd,
        d,
        months[(m - 1) as usize],
        y,
        secs / 3600,
        (secs / 60) % 60,
        secs % 60
    )
}

/// php_setcookie_common() + php_setcookie() (ext/standard/head.c)
fn setcookie_common(
    vm: &mut Vm,
    args: &[Value],
    fname: &str,
    is_raw: bool,
) -> Result<Value, VmError> {
    let throw = |vm: &mut Vm, class: &[u8], msg: String| -> Result<Value, VmError> {
        let exc = vm.create_exception(class, &msg, vm.current_line);
        vm.current_exception = Some(exc);
        Err(VmError {
            message: msg,
            line: vm.current_line,
        })
    };
    let illegal = "\",\", \";\", \" \", \"\\t\", \"\\r\", \"\\n\", \"\\013\", or \"\\014\"";
    let bad = |b: &[u8], with_eq: bool| {
        b.iter().any(|&c| {
            matches!(c, b',' | b';' | b' ' | b'\t' | b'\r' | b'\n' | 0x0b | 0x0c)
                || (with_eq && c == b'=')
        })
    };
    let name = vm
        .value_to_string(args.first().unwrap_or(&Value::Null))
        .as_bytes()
        .to_vec();
    let value: Option<Vec<u8>> = args
        .get(1)
        .map(|v| vm.value_to_string(v).as_bytes().to_vec());
    let mut expires: i64 = 0;
    let mut path: Option<Vec<u8>> = None;
    let mut domain: Option<Vec<u8>> = None;
    let mut secure = false;
    let mut httponly = false;
    let mut samesite: Option<Vec<u8>> = None;
    let mut partitioned = false;
    match args.get(2).map(|v| v.deref()) {
        Some(Value::Array(opts)) => {
            if args.len() > 3 {
                return throw(
                    vm,
                    b"ArgumentCountError",
                    format!(
                        "{}(): Expects exactly 3 arguments when argument #3 ($expires_or_options) is an array",
                        fname
                    ),
                );
            }
            let entries: Vec<(ArrayKey, Value)> = opts
                .borrow()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            for (k, v) in entries {
                let key = match k {
                    ArrayKey::String(s) => s.as_bytes().to_vec(),
                    ArrayKey::Int(_) => {
                        return throw(
                            vm,
                            b"ValueError",
                            format!("{}(): option array cannot have numeric keys", fname),
                        );
                    }
                };
                match key.to_ascii_lowercase().as_slice() {
                    b"expires" => expires = v.to_long(),
                    b"path" => path = Some(vm.value_to_string(&v).as_bytes().to_vec()),
                    b"domain" => domain = Some(vm.value_to_string(&v).as_bytes().to_vec()),
                    b"secure" => secure = v.is_truthy(),
                    b"httponly" => httponly = v.is_truthy(),
                    b"samesite" => samesite = Some(vm.value_to_string(&v).as_bytes().to_vec()),
                    b"partitioned" => partitioned = v.is_truthy(),
                    _ => {
                        return throw(
                            vm,
                            b"ValueError",
                            format!(
                                "{}(): option \"{}\" is invalid",
                                fname,
                                goro_core::string::msg_str(&key)
                            ),
                        );
                    }
                }
            }
        }
        Some(v) => expires = v.to_long(),
        None => {}
    }
    if let Some(v) = args.get(3) {
        path = Some(vm.value_to_string(v).as_bytes().to_vec());
    }
    if let Some(v) = args.get(4) {
        domain = Some(vm.value_to_string(v).as_bytes().to_vec());
    }
    if let Some(v) = args.get(5) {
        secure = v.is_truthy();
    }
    if let Some(v) = args.get(6) {
        httponly = v.is_truthy();
    }

    if name.is_empty() {
        return throw(
            vm,
            b"ValueError",
            format!("{}(): Argument #1 ($name) must not be empty", fname),
        );
    }
    if bad(&name, true) {
        return throw(
            vm,
            b"ValueError",
            format!(
                "{}(): Argument #1 ($name) cannot contain \"=\", {}",
                fname, illegal
            ),
        );
    }
    if is_raw {
        if let Some(v) = &value {
            if bad(v, false) {
                return throw(
                    vm,
                    b"ValueError",
                    format!(
                        "{}(): Argument #2 ($value) cannot contain {}",
                        fname, illegal
                    ),
                );
            }
        }
    }
    if let Some(p) = &path {
        if bad(p, false) {
            return throw(
                vm,
                b"ValueError",
                format!("{}(): \"path\" option cannot contain {}", fname, illegal),
            );
        }
    }
    if let Some(d) = &domain {
        if bad(d, false) {
            return throw(
                vm,
                b"ValueError",
                format!("{}(): \"domain\" option cannot contain {}", fname, illegal),
            );
        }
    }
    if expires >= 253402300800 {
        return throw(
            vm,
            b"ValueError",
            format!(
                "{}(): \"expires\" option cannot have a year greater than 9999",
                fname
            ),
        );
    }
    if partitioned && !secure {
        return throw(
            vm,
            b"ValueError",
            format!(
                "{}(): \"partitioned\" option cannot be used without \"secure\" option",
                fname
            ),
        );
    }
    let mut buf: Vec<u8> = name.clone();
    match &value {
        None => {
            buf.extend_from_slice(b"=deleted; expires=Thu, 01 Jan 1970 00:00:01 GMT; Max-Age=0")
        }
        Some(v) if v.is_empty() => {
            buf.extend_from_slice(b"=deleted; expires=Thu, 01 Jan 1970 00:00:01 GMT; Max-Age=0")
        }
        Some(v) => {
            buf.push(b'=');
            if is_raw {
                buf.extend_from_slice(v);
            } else {
                for &b in v {
                    match b {
                        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                            buf.push(b)
                        }
                        _ => buf.extend_from_slice(format!("%{:02X}", b).as_bytes()),
                    }
                }
            }
            if expires > 0 {
                buf.extend_from_slice(b"; expires=");
                buf.extend_from_slice(cookie_gmt_date(expires).as_bytes());
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                let diff = (expires - now).max(0);
                buf.extend_from_slice(format!("; Max-Age={}", diff).as_bytes());
            }
        }
    }
    if let Some(p) = &path {
        if !p.is_empty() {
            buf.extend_from_slice(b"; path=");
            buf.extend_from_slice(p);
        }
    }
    if let Some(d) = &domain {
        if !d.is_empty() {
            buf.extend_from_slice(b"; domain=");
            buf.extend_from_slice(d);
        }
    }
    if secure {
        buf.extend_from_slice(b"; secure");
    }
    if httponly {
        buf.extend_from_slice(b"; HttpOnly");
    }
    if let Some(ss) = &samesite {
        if !ss.is_empty() {
            buf.extend_from_slice(b"; SameSite=");
            buf.extend_from_slice(ss);
        }
    }
    if partitioned {
        buf.extend_from_slice(b"; Partitioned");
    }
    if headers_already_sent_error(vm) {
        return Ok(Value::False);
    }
    SENT_HEADERS.with(|h| {
        h.borrow_mut().push((b"Set-Cookie".to_vec(), buf));
    });
    Ok(Value::True)
}

fn setcookie_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    setcookie_common(vm, args, "setcookie", false)
}

fn setrawcookie_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    setcookie_common(vm, args, "setrawcookie", true)
}
fn headers_sent_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let (file, line) = vm.output_start.clone().unwrap_or_default();
    // (the line first, like php-src)
    if let Some(Value::Reference(r)) = args.get(1) {
        let _ = vm.try_assign_ref(r, Value::Long(line as i64));
    }
    if let Some(Value::Reference(r)) = args.first() {
        let _ = vm.try_assign_ref(r, Value::String(PhpString::from_string(file)));
    }
    Ok(if vm.headers_sent {
        Value::True
    } else {
        Value::False
    })
}

fn header_remove_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // sapi_header_op(SAPI_HEADER_DELETE[_ALL])
    if headers_already_sent_error(_vm) {
        return Ok(Value::Null);
    }
    if let Some(name_val) = args.first() {
        if matches!(name_val, Value::Null) {
            // Remove all
            SENT_HEADERS.with(|h| h.borrow_mut().clear());
        } else {
            let name = name_val.to_php_string();
            let mut name_bytes = name.as_bytes();
            if name_bytes.is_empty() {
                return Ok(Value::Null);
            }
            while let Some((&c, rest)) = name_bytes.split_last() {
                if !c.is_ascii_whitespace() && c != 0x0b {
                    break;
                }
                name_bytes = rest;
            }
            if name_bytes.contains(&b':') {
                _vm.emit_warning("Header to delete may not contain colon.");
                return Ok(Value::Null);
            }
            SENT_HEADERS.with(|h| {
                let mut hh = h.borrow_mut();
                hh.retain(|(k, _)| !k.eq_ignore_ascii_case(name_bytes));
            });
        }
    } else {
        SENT_HEADERS.with(|h| h.borrow_mut().clear());
    }
    Ok(Value::Null)
}

fn getallheaders_fn(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // Read HTTP_* keys from $_SERVER, converting back to original header names.
    let server = vm.get_global(b"_SERVER").unwrap_or(Value::Null);
    let mut out = PhpArray::new();
    if let Value::Array(arr) = server {
        for (k, v) in arr.borrow().iter() {
            let key = match k {
                ArrayKey::String(s) => s.to_string_lossy(),
                _ => continue,
            };
            if let Some(rest) = key.strip_prefix("HTTP_") {
                // HTTP_USER_AGENT -> User-Agent
                let parts: Vec<&str> = rest.split('_').collect();
                let header_name: String = parts
                    .iter()
                    .map(|p| {
                        let mut chars = p.chars();
                        match chars.next() {
                            Some(c) => c
                                .to_uppercase()
                                .chain(chars.flat_map(|c| c.to_lowercase()))
                                .collect::<String>(),
                            None => String::new(),
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("-");
                out.set(
                    ArrayKey::String(PhpString::from_string(header_name)),
                    v.clone(),
                );
            } else if key == "CONTENT_TYPE" {
                out.set(
                    ArrayKey::String(PhpString::from_bytes(b"Content-Type")),
                    v.clone(),
                );
            } else if key == "CONTENT_LENGTH" {
                out.set(
                    ArrayKey::String(PhpString::from_bytes(b"Content-Length")),
                    v.clone(),
                );
            }
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(out))))
}

fn apache_response_headers_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut arr = PhpArray::new();
    // PHP's apache_response_headers (in cli-server) prepends a default
    // X-Powered-By and appends a default Content-type when those aren't
    // explicitly set; mirror that.
    let mut has_xpb = false;
    let mut has_ct = false;
    SENT_HEADERS.with(|h| {
        for (k, _v) in h.borrow().iter() {
            if k.eq_ignore_ascii_case(b"X-Powered-By") {
                has_xpb = true;
            }
            if k.eq_ignore_ascii_case(b"Content-Type") || k.eq_ignore_ascii_case(b"Content-type") {
                has_ct = true;
            }
        }
    });
    if !has_xpb {
        arr.set(
            ArrayKey::String(PhpString::from_bytes(b"X-Powered-By")),
            Value::String(PhpString::from_bytes(b"PHP/8.5.4")),
        );
    }
    SENT_HEADERS.with(|h| {
        // (add_response_header(): a header without a name is left out)
        for (k, v) in h.borrow().iter().filter(|(k, _)| !k.is_empty()) {
            arr.set(
                ArrayKey::String(PhpString::from_vec(k.clone())),
                Value::String(PhpString::from_vec(v.clone())),
            );
        }
    });
    if !has_ct {
        arr.set(
            ArrayKey::String(PhpString::from_bytes(b"Content-type")),
            Value::String(PhpString::from_bytes(b"text/html; charset=UTF-8")),
        );
    }
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}
fn http_response_code_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let as_result = |code: i64| {
        if code == 0 {
            Value::False
        } else {
            Value::Long(code)
        }
    };
    if let Some(v) = args.first() {
        if !matches!(v, Value::Null) {
            if vm.headers_sent {
                let msg = match vm.output_start.clone() {
                    Some((f, l)) => format!(
                        "http_response_code(): Cannot set response code - headers already sent (output started at {}:{})",
                        f, l
                    ),
                    None => "http_response_code(): Cannot set response code - headers already sent"
                        .to_string(),
                };
                vm.emit_warning(&msg);
                return Ok(Value::False);
            }
            let code = v.to_long();
            if code != 0 && STATUS_LINE.with(|s| s.borrow().is_some()) {
                vm.emit_warning(
                    "http_response_code(): Calling http_response_code() after header('HTTP/...') has no effect",
                );
            }
            let prev = RESPONSE_CODE.with(|c| c.replace(code));
            return Ok(if prev == 0 {
                Value::True
            } else {
                Value::Long(prev)
            });
        }
    }
    Ok(as_result(RESPONSE_CODE.with(|c| c.get())))
}

fn array_all_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let arr_data: Vec<(ArrayKey, Value)> = arr
            .borrow()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let callback = args.get(1);

        if let Some(cb) = callback {
            for (key, val) in &arr_data {
                let key_val = match key {
                    ArrayKey::Int(n) => Value::Long(*n),
                    ArrayKey::String(s) => Value::String(s.clone()),
                };
                let result = call_user_func(vm, &[cb.clone(), val.clone(), key_val])?;
                if !result.is_truthy() {
                    return Ok(Value::False);
                }
            }
            Ok(Value::True)
        } else {
            Ok(if arr_data.iter().all(|(_, v)| v.is_truthy()) {
                Value::True
            } else {
                Value::False
            })
        }
    } else {
        Ok(Value::False)
    }
}

fn array_any_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let arr_data: Vec<(ArrayKey, Value)> = arr
            .borrow()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let callback = args.get(1);

        if let Some(cb) = callback {
            for (key, val) in &arr_data {
                let key_val = match key {
                    ArrayKey::Int(n) => Value::Long(*n),
                    ArrayKey::String(s) => Value::String(s.clone()),
                };
                let result = call_user_func(vm, &[cb.clone(), val.clone(), key_val])?;
                if result.is_truthy() {
                    return Ok(Value::True);
                }
            }
            Ok(Value::False)
        } else {
            Ok(if arr_data.iter().any(|(_, v)| v.is_truthy()) {
                Value::True
            } else {
                Value::False
            })
        }
    } else {
        Ok(Value::False)
    }
}

// spl_object_hash and spl_object_id moved to goro-ext-spl
#[allow(dead_code)]
fn str_contains_builtin(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let h = args.first().unwrap_or(&Value::Null).to_php_string();
    let n = args.get(1).unwrap_or(&Value::Null).to_php_string();
    if n.is_empty() {
        return Ok(Value::True);
    }
    let hb = h.as_bytes();
    let nb = n.as_bytes();
    for i in 0..=hb.len().saturating_sub(nb.len()) {
        if hb.len() >= i + nb.len() && &hb[i..i + nb.len()] == nb {
            return Ok(Value::True);
        }
    }
    Ok(Value::False)
}

// Simple stable merge sort that tolerates inconsistent comparators without
// panicking, unlike the stdlib sort on recent Rust versions. Used by usort
// and friends, where user callbacks may not implement a total order.

/// php_array_user_compare_unstable(): call the user comparator, handling the
/// bool-return deprecation (with swapped retry for false) like PHP.
fn user_sort_compare(
    vm: &mut Vm,
    cb: &Value,
    a: Value,
    b: Value,
    fname: &str,
    warned: &mut bool,
) -> i32 {
    if vm.current_exception.is_some() {
        return 0;
    }
    let result = match call_user_func(vm, &[cb.clone(), a.clone(), b.clone()]) {
        Ok(v) => v,
        Err(_) => return 0,
    };
    if matches!(result, Value::True | Value::False) {
        if !*warned {
            *warned = true;
            vm.emit_deprecated(&format!(
                "{}(): Returning bool from comparison function is deprecated, return an integer less than, equal to, or greater than zero",
                fname
            ));
        }
        if matches!(result, Value::False) {
            if vm.current_exception.is_some() {
                return 0;
            }
            let r2 = match call_user_func(vm, &[cb.clone(), b, a]) {
                Ok(v) => v,
                Err(_) => return 0,
            };
            let n = r2.to_long();
            return if n > 0 {
                -1
            } else if n < 0 {
                1
            } else {
                0
            };
        }
    }
    let n = result.to_long();
    if n > 0 {
        1
    } else if n < 0 {
        -1
    } else {
        0
    }
}

fn php_user_sort(
    vm: &mut Vm,
    args: &[Value],
    fname: &str,
    by_key: bool,
    renumber: bool,
) -> Result<Value, VmError> {
    let first = args.first().unwrap_or(&Value::Null);
    // (by reference: the variable gets the sorted copy, php_usort())
    let holder = match first {
        Value::Reference(r) => Some(r.clone()),
        _ => None,
    };
    let first = first.deref();
    require_array_arg(vm, &first, fname, "array", 1)?;
    if let (Value::Array(arr), Some(callback)) = (&first, args.get(1)) {
        let cb = callback.clone();
        let next_free = arr.borrow().next_free_key();
        let entries: Vec<(ArrayKey, Value)> = arr
            .borrow()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        if entries.is_empty() {
            return Ok(Value::True);
        }
        // The callbacks see the array as it was (they sort a copy).
        let arr = match holder {
            Some(_) => None,
            None => Some(arr.clone()),
        };
        drop(first);
        let mut warned = false;
        let sorted = if entries.len() > 1 {
            crate::zsort::zend_sort_stable(entries, |x, y| {
                if by_key {
                    user_sort_compare(
                        vm,
                        &cb,
                        key_to_value(&x.0),
                        key_to_value(&y.0),
                        fname,
                        &mut warned,
                    )
                } else {
                    user_sort_compare(vm, &cb, x.1.clone(), y.1.clone(), fname, &mut warned)
                }
            })
        } else {
            entries
        };
        let mut new_arr = PhpArray::new();
        for (k, v) in sorted {
            if renumber {
                new_arr.push(v);
            } else {
                new_arr.set(k, v);
            }
        }
        if !renumber {
            new_arr.set_next_free_key(next_free);
        }
        match (holder, arr) {
            (Some(r), _) => {
                let old = std::mem::replace(
                    &mut *r.borrow_mut(),
                    Value::Array(Rc::new(RefCell::new(new_arr))),
                );
                drop(old);
            }
            (None, Some(arr)) => *arr.borrow_mut() = new_arr,
            (None, None) => {}
        }
    }
    Ok(Value::True)
}

fn usort_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    php_user_sort(vm, args, "usort", false, true)
}

fn uasort_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    php_user_sort(vm, args, "uasort", false, false)
}
fn uksort_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    php_user_sort(vm, args, "uksort", true, false)
}

fn array_reduce_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let arr = match args.first() {
        Some(Value::Array(a)) => a.borrow(),
        _ => return Ok(Value::Null),
    };
    let callback = args.get(1).cloned().unwrap_or(Value::Null);
    let initial = args.get(2).cloned().unwrap_or(Value::Null);

    let entries: Vec<Value> = arr.values().cloned().collect();
    drop(arr);

    // Deep-clone the initial value so that modifications inside the callback
    // do not affect the caller's $initial (PHP value semantics for arrays).
    // Also deep-clone the carry between iterations to ensure the user's
    // returned value semantics are preserved.
    let mut carry = deep_clone_value(&initial);
    for val in entries {
        let result = vm.call_callback(&callback, &[deep_clone_value(&carry), val])?;
        carry = result;
    }

    Ok(carry)
}

fn array_key_first_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let arr = arr.borrow();
        Ok(arr
            .iter()
            .next()
            .map(|(k, _)| match k {
                goro_core::array::ArrayKey::Int(n) => Value::Long(*n),
                goro_core::array::ArrayKey::String(s) => Value::String(s.clone()),
            })
            .unwrap_or(Value::Null))
    } else {
        Ok(Value::Null)
    }
}
fn array_key_last_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let arr = arr.borrow();
        Ok(arr
            .iter()
            .last()
            .map(|(k, _)| match k {
                goro_core::array::ArrayKey::Int(n) => Value::Long(*n),
                goro_core::array::ArrayKey::String(s) => Value::String(s.clone()),
            })
            .unwrap_or(Value::Null))
    } else {
        Ok(Value::Null)
    }
}
fn array_is_list_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    match args.first() {
        Some(Value::Array(arr)) => {
            let arr = arr.borrow();
            let is_list = arr.iter().enumerate().all(
                |(i, (k, _))| matches!(k, goro_core::array::ArrayKey::Int(n) if *n == i as i64),
            );
            Ok(if is_list { Value::True } else { Value::False })
        }
        _ => {
            let type_name = args
                .first()
                .map(Vm::value_type_name)
                .unwrap_or("null".to_string());
            let msg = format!(
                "array_is_list(): Argument #1 ($array) must be of type array, {} given",
                type_name
            );
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            Err(VmError {
                message: msg,
                line: vm.current_line,
            })
        }
    }
}
#[allow(dead_code)]
fn compact_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // compact() needs access to the current scope's variables which we don't have from builtins
    Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
}

/// Body of the fiber the collector runs destructors in when it is invoked
/// inside a fiber (zend_gc.c's gc_destructor_fiber).
fn gc_destructor_fiber_fn(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    while goro_core::gc::dtor_step(vm) {}
    Ok(Value::Null)
}

/// gc_mem_caches(): zend_mm_gc()
fn gc_mem_caches_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(goro_core::value::memory_gc() as i64))
}

fn gc_status_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let st = goro_core::gc::gc_status();
    let mut result = PhpArray::new();
    let mut put = |k: &[u8], v: Value| {
        result.set(
            goro_core::array::ArrayKey::String(PhpString::from_bytes(k)),
            v,
        );
    };
    let b = |x: bool| if x { Value::True } else { Value::False };
    put(b"running", b(st.running));
    put(b"protected", b(st.protected));
    put(b"full", b(st.full));
    put(b"runs", Value::Long(st.runs));
    put(b"collected", Value::Long(st.collected));
    put(b"threshold", Value::Long(st.threshold));
    put(b"buffer_size", Value::Long(st.buffer_size));
    put(b"roots", Value::Long(st.roots));
    put(b"application_time", Value::Double(st.application_time));
    put(b"collector_time", Value::Double(st.collector_time));
    put(b"destructor_time", Value::Double(st.destructor_time));
    put(b"free_time", Value::Double(st.free_time));
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn debug_zval_dump_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    crate::output::debug_zval_dump(vm, args);
    Ok(Value::Null)
}

#[allow(dead_code)]
fn var_dump_direct(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // This overrides the one in output.rs - but output.rs registers first
    // So this won't actually be called. Let's skip.
    Ok(Value::Null)
}

/// Throw a TypeError from a builtin (sets the pending exception).
fn throw_type_error_msg(vm: &mut Vm, msg: String) -> VmError {
    let exc = vm.throw_type_error(msg.clone());
    vm.current_exception = Some(exc);
    VmError {
        message: format!("Uncaught TypeError: {}", msg),
        line: vm.current_line,
    }
}

fn get_class_methods_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().map(|v| v.deref()).unwrap_or(Value::Null);
    let class_name = match &val {
        Value::String(s) => {
            let name = s.as_bytes().to_vec();
            let lower: Vec<u8> = name.iter().map(|b| b.to_ascii_lowercase()).collect();
            if !vm.classes.contains_key(&lower) {
                vm.try_autoload_class(&name);
                if vm.current_exception.is_some() {
                    return Err(VmError {
                        message: "Uncaught exception".into(),
                        line: vm.current_line,
                    });
                }
            }
            if !vm.classes.contains_key(&lower)
                && goro_core::reflection::class_method_list(vm, &lower).is_none()
            {
                return Err(throw_type_error_msg(
                    vm,
                    format!(
                        "get_class_methods(): Argument #1 ($object_or_class) must be an object or a valid class name, {} given",
                        Vm::value_type_name(&val)
                    ),
                ));
            }
            name
        }
        Value::Object(obj) => obj.borrow().class_name.clone(),
        _ => {
            return Err(throw_type_error_msg(
                vm,
                format!(
                    "get_class_methods(): Argument #1 ($object_or_class) must be an object or a valid class name, {} given",
                    Vm::value_type_name(&val)
                ),
            ));
        }
    };
    let class_lower: Vec<u8> = class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
    let scope: Option<Vec<u8>> = vm
        .current_class_scope()
        .map(|name| name.iter().map(|b| b.to_ascii_lowercase()).collect());
    let mut result = PhpArray::new();
    if let Some(methods) = goro_core::reflection::class_method_list(vm, &class_lower) {
        for (name, vis, declaring) in methods {
            let visible = match vis {
                goro_core::object::Visibility::Public => true,
                goro_core::object::Visibility::Protected => match scope {
                    Some(ref scope) => {
                        scope.as_slice() == declaring.as_slice()
                            || vm.class_extends(scope, &declaring)
                            || vm.class_extends(&declaring, scope)
                    }
                    None => false,
                },
                goro_core::object::Visibility::Private => match scope {
                    Some(ref scope) => scope.as_slice() == declaring.as_slice(),
                    None => false,
                },
            };
            if visible {
                result.push(Value::String(PhpString::from_vec(name)));
            }
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn lower_bytes(b: &[u8]) -> Vec<u8> {
    b.iter().map(|c| c.to_ascii_lowercase()).collect()
}

#[allow(dead_code)]
fn get_parent_class_real(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let class_name = match args.first() {
        Some(Value::Object(obj)) => obj.borrow().class_name.clone(),
        Some(Value::String(s)) => s.as_bytes().to_vec(),
        _ => return Ok(Value::False),
    };
    let class_lower: Vec<u8> = class_name.iter().map(|b| b.to_ascii_lowercase()).collect();

    if let Some(class) = vm.classes.get(&class_lower)
        && let Some(ref parent) = class.parent
    {
        return Ok(Value::String(PhpString::from_vec(parent.clone())));
    }
    Ok(Value::False)
}

// iterator_to_array and iterator_count moved to goro-ext-spl

/// INI settings share the constant table; their names ("mysqli.max_links")
/// are lowercase and dotted, which no extension constant is.
fn is_ini_entry_name(name: &[u8]) -> bool {
    goro_core::vm::is_ini_entry_key(name)
}

fn get_defined_constants_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let categorize = args.first().map(|v| v.is_truthy()).unwrap_or(false);
    let mut result = PhpArray::new();
    if categorize {
        // Return categorized array, by module in module order. Constants do
        // not record the module that registered them: the extensions that
        // list theirs, the stubs' and a few prefixes tell.
        use goro_core::array::ArrayKey;
        let key = |n: &[u8]| ArrayKey::String(PhpString::from_vec(n.to_vec()));
        let mut buckets: Vec<(String, PhpArray)> = Vec::new();
        fn bucket<'a>(b: &'a mut Vec<(String, PhpArray)>, m: &str) -> &'a mut PhpArray {
            let i = match b.iter().position(|(n, _)| n.eq_ignore_ascii_case(m)) {
                Some(i) => i,
                None => {
                    b.push((m.to_string(), PhpArray::new()));
                    b.len() - 1
                }
            };
            &mut b[i].1
        }
        // Core's own, in registration order.
        {
            let core = bucket(&mut buckets, "Core");
            for n in goro_core::reflection::CORE_CONSTANT_ORDER {
                if let Some(v) = vm.constants.get(n.as_bytes()) {
                    core.set(key(n.as_bytes()), v.clone());
                }
            }
        }
        for (module, names) in &vm.module_constants {
            let m = String::from_utf8_lossy(module).into_owned();
            let arr = bucket(&mut buckets, &m);
            for n in names {
                if let Some(v) = vm.constants.get(n) {
                    arr.set(key(n), v.clone());
                }
            }
        }
        for (name, val) in &vm.constants {
            if is_ini_entry_name(name) {
                continue;
            }
            if vm.module_constants.iter().any(|(_, n)| n.contains(name)) {
                continue;
            }
            if vm.constant_files.contains_key(name) {
                continue;
            }
            let stub_ext = goro_core::stub_sigs::CONSTANTS
                .binary_search_by(|c| c.name.as_bytes().cmp(name.as_slice()))
                .ok()
                .map(|i| goro_core::stub_sigs::CONSTANTS[i].ext);
            let target = if name.starts_with(b"LIBXML_") {
                "libxml"
            } else if name.starts_with(b"MYSQLI_") {
                "mysqli"
            } else if let Some(e) = stub_ext {
                e
            } else if name.starts_with(b"STREAM_")
                || name.starts_with(b"IMAGETYPE_")
                || name == b"YESEXPR"
                || name == b"YESSTR"
            {
                "standard"
            } else if name.starts_with(b"ZLIB_") {
                "zlib"
            } else if name.starts_with(b"Dom\\") || name.starts_with(b"dom\\") {
                "dom"
            } else if name.starts_with(b"MHASH_") {
                "hash"
            } else if name.starts_with(b"MB_") {
                "mbstring"
            } else {
                "Core"
            };
            bucket(&mut buckets, target).set(key(name), val.clone());
        }
        // Core first, then the modules in module order.
        let rank = |m: &str| -> usize {
            if m.eq_ignore_ascii_case("Core") {
                return 0;
            }
            MODULE_ORDER
                .iter()
                .position(|o| o.eq_ignore_ascii_case(m))
                .map(|p| p + 1)
                .unwrap_or(usize::MAX)
        };
        buckets.sort_by_key(|(m, _)| rank(m));
        for (module, arr) in buckets {
            if !arr.is_empty() || module == "Core" {
                let name = MODULE_ORDER
                    .iter()
                    .find(|o| o.eq_ignore_ascii_case(&module))
                    .map(|o| o.to_string())
                    .unwrap_or(module);
                result.set(
                    ArrayKey::String(PhpString::from_string(name)),
                    Value::Array(Rc::new(RefCell::new(arr))),
                );
            }
        }
        let mut user = PhpArray::new();
        for (key, name) in &vm.user_constant_names {
            if let Some(v) = vm.constants.get(key) {
                user.set(
                    goro_core::array::ArrayKey::String(PhpString::from_vec(name.clone())),
                    v.clone(),
                );
            }
        }
        // (only when there are any)
        if !user.is_empty() {
            result.set(
                goro_core::array::ArrayKey::String(PhpString::from_bytes(b"user")),
                Value::Array(Rc::new(RefCell::new(user))),
            );
        }
    } else {
        for (name, val) in &vm.constants {
            if is_ini_entry_name(name) || vm.constant_files.contains_key(name) {
                continue;
            }
            result.set(
                goro_core::array::ArrayKey::String(PhpString::from_vec(name.clone())),
                val.clone(),
            );
        }
        // User constants last, in definition order, named as defined.
        for (key, name) in &vm.user_constant_names {
            if let Some(v) = vm.constants.get(key) {
                result.set(
                    goro_core::array::ArrayKey::String(PhpString::from_vec(name.clone())),
                    v.clone(),
                );
            }
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

/// The properties of `class` declared by its ancestor (or itself) `level`,
/// in that class's declaration order (Zend's properties_info order).
fn class_level_props<'a>(
    vm: &Vm,
    class: &'a goro_core::object::ClassEntry,
    level: &[u8],
) -> Vec<&'a goro_core::object::PropertyDef> {
    let order: Vec<&[u8]> = match vm.classes.get(level) {
        Some(c) => goro_core::reflection::own_declared_props(c, level)
            .into_iter()
            .map(|p| p.name.as_slice())
            .collect(),
        None => Vec::new(),
    };
    let mut v: Vec<&goro_core::object::PropertyDef> = class.properties.iter().collect();
    v.sort_by_key(|p| {
        order
            .iter()
            .position(|n| *n == p.name.as_slice())
            .unwrap_or(usize::MAX)
    });
    v
}

fn get_class_vars_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let class_name = args.first().unwrap_or(&Value::Null).to_php_string();
    let bare = class_name
        .as_bytes()
        .strip_prefix(b"\\")
        .unwrap_or(class_name.as_bytes());
    let mut name_lower: Vec<u8> = bare.to_ascii_lowercase();
    // Z_PARAM_CLASS: zend_lookup_class() autoloads.
    for attempt in 0..2 {
        if let Some(canon) = vm.class_aliases.get(&name_lower) {
            name_lower = canon.to_ascii_lowercase();
        }
        if attempt == 1
            || vm.classes.contains_key(&name_lower)
            || goro_core::stubs::class_entry(vm, &name_lower).is_some()
        {
            break;
        }
        let bare = bare.to_vec();
        vm.try_autoload_class(&bare);
        if vm.current_exception.is_some() {
            return Ok(Value::Null);
        }
    }
    // Internal classes known by name only: their declared shape (stubs)
    let internal = if vm.classes.contains_key(&name_lower) {
        None
    } else {
        goro_core::stubs::class_entry(vm, &name_lower)
    };
    // Z_PARAM_CLASS throws a TypeError when the class is not declared;
    // the message uses the original-case spelling supplied by the caller.
    if !vm.classes.contains_key(&name_lower) && internal.is_none() {
        let class_str = class_name.to_string_lossy().to_string();
        let msg = format!(
            "get_class_vars(): Argument #1 ($class) must be a valid class name, {} given",
            class_str
        );
        let exc = vm.create_exception(b"TypeError", &msg, 0);
        vm.current_exception = Some(exc);
        return Ok(Value::Null);
    }
    // Constant-expression defaults are evaluated first (zend_update_class_constants).
    if !vm.update_class_constants_ok(&name_lower) {
        return Ok(Value::Null);
    }
    // Caller-scope visibility filter: only return properties the calling
    // class would be allowed to see. Mirrors what PHP's get_class_vars
    // does at the engine level. Compare scopes in lowercase form because
    // PHP class names are case-insensitive and declaring_class is stored
    // lowercase.
    let caller_scope: Option<Vec<u8>> = vm
        .current_class_scope()
        .map(|n| n.iter().map(|b| b.to_ascii_lowercase()).collect());
    let can_see = |prop: &goro_core::object::PropertyDef| -> bool {
        match prop.visibility {
            goro_core::object::Visibility::Public => true,
            goro_core::object::Visibility::Protected => {
                // Visible when the caller's scope and the property's
                // declaring class share an inheritance chain — i.e. the
                // caller IS the declaring class, or descends from it, or
                // the declaring class descends from the caller. This
                // matches the check Zend performs at object access.
                if let Some(scope) = &caller_scope {
                    let decl_lower: Vec<u8> = prop
                        .declaring_class
                        .iter()
                        .map(|b| b.to_ascii_lowercase())
                        .collect();
                    *scope == decl_lower
                        || vm.class_extends(scope, &decl_lower)
                        || vm.class_extends(&decl_lower, scope)
                } else {
                    false
                }
            }
            goro_core::object::Visibility::Private => {
                // Visible only when the caller IS the declaring class.
                caller_scope.as_deref() == Some(prop.declaring_class.as_slice())
            }
        }
    };
    let mut result = PhpArray::new();
    for p in enum_declared_props(vm, &name_lower) {
        result.set(
            goro_core::array::ArrayKey::String(PhpString::from_bytes(p)),
            Value::Null,
        );
    }
    if let Some(class) = internal {
        for statics in [false, true] {
            for prop in &class.properties {
                if prop.is_static != statics || prop.is_virtual || !can_see(prop) {
                    continue;
                }
                let val = match &prop.default {
                    Value::Undef => Value::Null,
                    other => other.clone(),
                };
                result.set(
                    goro_core::array::ArrayKey::String(PhpString::from_vec(prop.name.clone())),
                    val,
                );
            }
        }
    } else if vm.classes.contains_key(&name_lower) {
        // Build the inheritance chain from most-derived to base; PHP's
        // get_class_vars emits each level's declared properties in that
        // order (non-static across all classes first, then static).
        let mut chain: Vec<Vec<u8>> = Vec::new();
        let mut cur = name_lower.clone();
        for _ in 0..50 {
            chain.push(cur.clone());
            let parent = vm.classes.get(&cur).and_then(|c| c.parent.clone());
            match parent {
                Some(p) => {
                    cur = p.iter().map(|b| b.to_ascii_lowercase()).collect();
                }
                None => break,
            }
        }
        let class = vm.classes.get(&name_lower).unwrap();
        // The class's property table holds one entry per name: a name a
        // descendant redeclares hides the ancestor's (private) one.
        let declared_at = |lvl: &[u8]| -> Vec<Vec<u8>> {
            class
                .properties
                .iter()
                .filter(|p| p.declaring_class.eq_ignore_ascii_case(lvl))
                .map(|p| p.name.clone())
                .collect()
        };
        let mut hidden: Vec<Vec<Vec<u8>>> = Vec::with_capacity(chain.len());
        let mut seen: Vec<Vec<u8>> = Vec::new();
        for cls_lower in &chain {
            hidden.push(seen.clone());
            seen.extend(declared_at(cls_lower));
        }
        // Non-static, walking child → parent
        for (lvl, cls_lower) in chain.iter().enumerate() {
            for prop in class_level_props(vm, class, cls_lower) {
                if prop.is_static || prop.is_virtual {
                    continue;
                }
                let decl_lower: Vec<u8> = prop
                    .declaring_class
                    .iter()
                    .map(|b| b.to_ascii_lowercase())
                    .collect();
                if decl_lower != *cls_lower || hidden[lvl].contains(&prop.name) {
                    continue;
                }
                if !can_see(prop) {
                    continue;
                }
                let val = match &prop.default {
                    Value::Undef => Value::Null,
                    other => other.clone(),
                };
                result.set(
                    goro_core::array::ArrayKey::String(PhpString::from_vec(prop.name.clone())),
                    val,
                );
            }
            // An internal ancestor: its (stub-declared) properties.
            if !vm.classes.contains_key(cls_lower) {
                if let Some(se) = goro_core::stubs::class_entry(vm, cls_lower) {
                    for prop in &se.properties {
                        if prop.is_static || prop.is_virtual || !can_see(prop) {
                            continue;
                        }
                        let key = goro_core::array::ArrayKey::String(PhpString::from_vec(
                            prop.name.clone(),
                        ));
                        if result.get(&key).is_none() {
                            let val = match &prop.default {
                                Value::Undef => Value::Null,
                                other => other.clone(),
                            };
                            result.set(key, val);
                        }
                    }
                }
            }
        }
        // Static, walking child → parent
        for (lvl, cls_lower) in chain.iter().enumerate() {
            for prop in class_level_props(vm, class, cls_lower) {
                if !prop.is_static {
                    continue;
                }
                let decl_lower: Vec<u8> = prop
                    .declaring_class
                    .iter()
                    .map(|b| b.to_ascii_lowercase())
                    .collect();
                if decl_lower != *cls_lower || hidden[lvl].contains(&prop.name) {
                    continue;
                }
                if !can_see(prop) {
                    continue;
                }
                let val = vm
                    .static_prop_raw(&name_lower, &prop.name)
                    .unwrap_or_else(|| prop.default.clone());
                let val = match val {
                    Value::Undef => Value::Null,
                    other => other,
                };
                result.set(
                    goro_core::array::ArrayKey::String(PhpString::from_vec(prop.name.clone())),
                    val,
                );
            }
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn opendir_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let path_str = path.to_string_lossy();
    if let Some(pattern) = path_str.strip_prefix("glob://") {
        let names = glob_stream_entries(vm, pattern);
        return Ok(Value::resource(alloc_dir_handle_entries(
            path_str.to_string(),
            names,
        )));
    }
    let dir_path = std::path::Path::new(&*path_str);
    if dir_path.is_dir() {
        match std::fs::read_dir(dir_path) {
            Ok(_) => {
                let did = alloc_dir_handle(path_str.to_string());
                Ok(Value::resource(did))
            }
            Err(e) => {
                vm.emit_warning(&format!(
                    "opendir({}): Failed to open directory: {}",
                    path_str,
                    strip_os_error(&e.to_string())
                ));
                Ok(Value::False)
            }
        }
    } else {
        let err_msg = match std::fs::metadata(dir_path) {
            Err(e) => strip_os_error(&e.to_string()),
            Ok(_) => "Not a directory".to_string(),
        };
        vm.emit_warning(&format!(
            "opendir({}): Failed to open directory: {}",
            path_str, err_msg
        ));
        Ok(Value::False)
    }
}

/// Resolve the resource id for a dir-handle argument and validate it.
/// On null/missing argument: emits the deprecation notice and falls back to
/// the current-dir-resource. On wrong-type argument: throws a TypeError.
/// Returns Ok(Some(id)) when we should proceed, Ok(None) when we should
/// short-circuit returning the supplied default value (e.g. when null is
/// passed and there is no current dir resource), or Err if a TypeError was
/// thrown.
fn resolve_dir_handle(
    vm: &mut Vm,
    args: &[Value],
    func_name: &str,
) -> Result<Option<i64>, VmError> {
    // Determine the supplied arg, treating "no argument supplied" identically to
    // "null supplied" — PHP's signature has a default of null.
    let raw = args.first().cloned().unwrap_or(Value::Null);
    // Dereference once to inspect the underlying value.
    let val = match &raw {
        Value::Reference(r) => r.borrow().clone(),
        v => v.clone(),
    };
    match val {
        Value::Null | Value::Undef => {
            vm.emit_deprecated_at(
                &format!(
                    "{}(): Passing null is deprecated, instead the last opened directory stream should be provided",
                    func_name
                ),
                vm.current_line,
            );
            Ok(CURRENT_DIR_RESOURCE.with(|c| c.get()))
        }
        Value::Long(_) => {
            // PHP TypeError: arg is int, not resource.
            let exc = vm.throw_type_error(format!(
                "{}(): Argument #1 ($dir_handle) must be of type resource, int given",
                func_name
            ));
            vm.current_exception = Some(exc);
            Err(VmError {
                message: format!(
                    "{}(): Argument #1 ($dir_handle) must be of type resource, int given",
                    func_name
                ),
                line: vm.current_line,
            })
        }
        Value::Resource(n) => {
            let is_dir = DIR_HANDLES.with(|dh| dh.borrow().contains_key(&n.id()));
            if is_dir {
                Ok(Some(n.id()))
            } else if !goro_core::value::is_open_resource(n.id()) {
                let exc = vm.throw_type_error(format!(
                    "{}(): Argument #1 ($dir_handle) must be an open stream resource",
                    func_name
                ));
                vm.current_exception = Some(exc);
                Err(VmError {
                    message: format!(
                        "{}(): Argument #1 ($dir_handle) must be an open stream resource",
                        func_name
                    ),
                    line: vm.current_line,
                })
            } else {
                let exc = vm.throw_type_error(format!(
                    "{}(): Argument #1 ($dir_handle) must be a valid Directory resource",
                    func_name
                ));
                vm.current_exception = Some(exc);
                Err(VmError {
                    message: format!(
                        "{}(): Argument #1 ($dir_handle) must be a valid Directory resource",
                        func_name
                    ),
                    line: vm.current_line,
                })
            }
        }
        Value::Object(_) => {
            if let Some(id) = dir_handle_resource_id(&val) {
                let is_dir = DIR_HANDLES.with(|dh| dh.borrow().contains_key(&id));
                if is_dir {
                    Ok(Some(id))
                } else if !goro_core::value::is_open_resource(id) {
                    let exc = vm.throw_type_error(format!(
                        "{}(): Argument #1 ($dir_handle) must be an open stream resource",
                        func_name
                    ));
                    vm.current_exception = Some(exc);
                    Err(VmError {
                        message: format!(
                            "{}(): Argument #1 ($dir_handle) must be an open stream resource",
                            func_name
                        ),
                        line: vm.current_line,
                    })
                } else {
                    let exc = vm.throw_type_error(format!(
                        "{}(): Argument #1 ($dir_handle) must be a valid Directory resource",
                        func_name
                    ));
                    vm.current_exception = Some(exc);
                    Err(VmError {
                        message: format!(
                            "{}(): Argument #1 ($dir_handle) must be a valid Directory resource",
                            func_name
                        ),
                        line: vm.current_line,
                    })
                }
            } else {
                let type_name = val.type_name();
                let exc = vm.throw_type_error(format!(
                    "{}(): Argument #1 ($dir_handle) must be of type resource, {} given",
                    func_name, type_name
                ));
                vm.current_exception = Some(exc);
                Err(VmError {
                    message: format!(
                        "{}(): Argument #1 ($dir_handle) must be of type resource, {} given",
                        func_name, type_name
                    ),
                    line: vm.current_line,
                })
            }
        }
        other => {
            let type_name = other.type_name();
            let exc = vm.throw_type_error(format!(
                "{}(): Argument #1 ($dir_handle) must be of type resource, {} given",
                func_name, type_name
            ));
            vm.current_exception = Some(exc);
            Err(VmError {
                message: format!(
                    "{}(): Argument #1 ($dir_handle) must be of type resource, {} given",
                    func_name, type_name
                ),
                line: vm.current_line,
            })
        }
    }
}

fn closedir_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let id = match resolve_dir_handle(vm, args, "closedir")? {
        Some(id) => id,
        None => {
            // Null with no current dir handle. PHP throws "No resource supplied"
            // as a TypeError after the deprecation notice.
            let exc = vm.throw_type_error("No resource supplied".to_string());
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: "No resource supplied".to_string(),
                line: vm.current_line,
            });
        }
    };
    DIR_HANDLES.with(|dh| {
        dh.borrow_mut().remove(&id);
    });
    goro_core::value::close_resource(id);
    if CURRENT_DIR_RESOURCE.with(|c| c.get()) == Some(id) {
        set_current_dir(None);
    }
    Ok(Value::Null)
}

fn readdir_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let id = match resolve_dir_handle(vm, args, "readdir")? {
        Some(id) => id,
        None => return Ok(Value::False),
    };
    DIR_HANDLES.with(|dh| {
        let mut handles = dh.borrow_mut();
        if let Some(entries) = handles.get_mut(&id) {
            if let Some(name) = entries.remaining.pop() {
                Ok(Value::String(PhpString::from_string(name)))
            } else {
                Ok(Value::False)
            }
        } else {
            Ok(Value::False)
        }
    })
}

fn chmod_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let filename = args.first().unwrap_or(&Value::Null).to_php_string();
    let mode_val = args.get(1).cloned().unwrap_or(Value::Null);
    // chmod() requires permissions to be an int. PHP throws TypeError
    // on string/array/object/null.
    let mode = match &mode_val {
        Value::Long(n) => *n,
        Value::Double(d) => *d as i64,
        Value::True => 1,
        Value::False => 0,
        _ => {
            let actual_type = match &mode_val {
                Value::String(_) => "string",
                Value::Array(_) => "array",
                Value::Object(_) => "object",
                Value::Null => "null",
                _ => "unknown",
            };
            let msg = format!(
                "chmod(): Argument #2 ($permissions) must be of type int, {} given",
                actual_type
            );
            let exc = vm.create_exception(b"TypeError", &msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = filename.to_string_lossy().to_string();
        let perms = std::fs::Permissions::from_mode(mode as u32 & 0o7777);
        match std::fs::set_permissions(&path, perms) {
            Ok(_) => Ok(Value::True),
            Err(e) => {
                let msg = strip_os_error(&e.to_string());
                vm.emit_warning(&format!("chmod(): {}", msg));
                Ok(Value::False)
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (filename, mode);
        Ok(Value::True)
    }
}

fn symlink_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let target = args.first().unwrap_or(&Value::Null).to_php_string();
    let link = args.get(1).unwrap_or(&Value::Null).to_php_string();
    if let Some(e) = path_arg_nul_error(vm, target.as_bytes(), "symlink", 1, "target")
        .or_else(|| path_arg_nul_error(vm, link.as_bytes(), "symlink", 2, "link"))
    {
        return Err(e);
    }
    let target_s = target.to_string_lossy();
    let link_s = link.to_string_lossy();
    // expand_filepath_ex() of the target, relative to the link's directory
    // (CWD_FILEPATH): a leading component that exists but is not a
    // directory makes it fail.
    {
        let base = std::path::Path::new(&*link_s)
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default();
        let full = base.join(&*target_s);
        let comps: Vec<_> = full.components().collect();
        let mut cur = std::path::PathBuf::new();
        for (i, c) in comps.iter().enumerate() {
            cur.push(c);
            if i + 1 == comps.len() {
                break;
            }
            match std::fs::metadata(if cur.as_os_str().is_empty() {
                std::path::Path::new(".")
            } else {
                &cur
            }) {
                Ok(m) if !m.is_dir() => {
                    vm.emit_warning("symlink(): No such file or directory");
                    return Ok(Value::False);
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
    }
    #[cfg(unix)]
    {
        match std::os::unix::fs::symlink(&*target_s, &*link_s) {
            Ok(_) => Ok(Value::True),
            Err(e) => {
                let msg = strip_os_error(&e.to_string());
                vm.emit_warning(&format!("symlink(): {}", msg));
                Ok(Value::False)
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (target_s, link_s);
        vm.emit_warning("symlink(): Not supported on this platform");
        Ok(Value::False)
    }
}

fn readlink_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let path_str = path.to_string_lossy();
    match std::fs::read_link(&*path_str) {
        Ok(p) => Ok(Value::String(PhpString::from_string(
            p.to_string_lossy().to_string(),
        ))),
        Err(e) => {
            // Strip "(os error N)" trailer and other Rust-isms.
            let msg = e.to_string();
            let stripped = strip_os_error(&msg);
            vm.emit_warning(&format!("readlink(): {}", stripped));
            Ok(Value::False)
        }
    }
}

/// Strip Rust's "(os error N)" trailer to produce a clean message like PHP's.
pub(crate) fn strip_os_error(msg: &str) -> String {
    if let Some(idx) = msg.rfind(" (os error") {
        msg[..idx].to_string()
    } else {
        msg.to_string()
    }
}

/// OnSetExceptionStringParamMaxLen: 0 to 1000000 (ZEND_ATOL of the value).
fn on_set_exception_string_param_max_len(vm: &mut Vm, value: &[u8], _startup: bool) -> bool {
    let s = String::from_utf8_lossy(value);
    let t = s.trim_start();
    let end = t
        .char_indices()
        .find(|&(i, c)| !(c.is_ascii_digit() || (i == 0 && (c == '-' || c == '+'))))
        .map_or(t.len(), |(i, _)| i);
    let n = t[..end].parse::<i64>().unwrap_or(0);
    if (0..=1_000_000).contains(&n) {
        vm.exception_string_param_max_len = n as usize;
        true
    } else {
        false
    }
}

fn debug_backtrace_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // DEBUG_BACKTRACE_PROVIDE_OBJECT (1) is the default
    let options = args.first().map(|v| v.to_long()).unwrap_or(1);
    let limit = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let trace = vm.debug_backtrace_array(options, limit, None);
    Ok(Value::Array(Rc::new(RefCell::new(trace))))
}

fn debug_print_backtrace_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let options = args.first().map(|v| v.to_long()).unwrap_or(0);
    let limit = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    // zend_trace_to_string() of the backtrace, without objects or {main}
    let trace = vm.debug_backtrace_array(options & !1, limit, None);
    let trace = Value::Array(Rc::new(RefCell::new(trace)));
    let max_len = vm.exception_string_param_max_len;
    let output = vm.trace_to_string(&trace, false, max_len);
    vm.write_output(output.as_bytes());
    Ok(Value::Null)
}

fn array_key_exists_fn2(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let key = args.first().unwrap_or(&Value::Null);
    let arr = args.get(1).unwrap_or(&Value::Null);

    // Check for null key deprecation
    let key_deref = key.deref();
    if matches!(key_deref, Value::Null | Value::Undef) {
        vm.emit_deprecated("Using null as the key parameter for array_key_exists() is deprecated, use an empty string instead");
    }

    // Float keys go through zend_dval_to_lval_safe: out-of-range values warn
    // and wrap, fractional ones are deprecated.
    let float_key = if let Value::Double(f) = key_deref {
        let line = vm.current_line;
        let holders = match arr {
            Value::Array(a) => std::rc::Rc::strong_count(a),
            _ => 0,
        };
        let n = vm.double_offset_to_long(f, line);
        // ZEND_ARRAY_KEY_EXISTS: an array released by the warning's error
        // handler (only this call still holds it) reports false.
        if let Value::Array(a) = arr {
            let now = std::rc::Rc::strong_count(a);
            if now < holders && now == 1 {
                return Ok(Value::False);
            }
        }
        Some(n)
    } else {
        None
    };

    if let Value::Resource(id) = &key_deref {
        vm.emit_warning(&format!(
            "Resource ID#{} used as offset, casting to integer ({})",
            id, id
        ));
    }

    // Check for invalid key types (array, object)
    match &key_deref {
        Value::Array(_) => {
            let msg = "Cannot access offset of type array on array".to_string();
            let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
        Value::Object(obj) => {
            let class_name = goro_core::value::display_class_name(&obj.borrow().class_name);
            let msg = format!("Cannot access offset of type {} on array", class_name);
            let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
        _ => {}
    }

    if let Value::Array(a) = arr {
        let arr_key = match float_key {
            Some(n) => goro_core::array::ArrayKey::Int(n),
            None => goro_core::vm::Vm::value_to_array_key(key.clone()),
        };
        Ok(if a.borrow().contains_key(&arr_key) {
            Value::True
        } else {
            Value::False
        })
    } else {
        let type_name = arg_type_name(arr);
        let msg = format!(
            "array_key_exists(): Argument #2 ($array) must be of type array, {} given",
            type_name
        );
        let exc = vm.create_exception(b"TypeError", &msg, 0);
        vm.current_exception = Some(exc);
        Err(VmError {
            message: msg,
            line: vm.current_line,
        })
    }
}

fn clearstatcache_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Stat results are not cached; the realpath cache is.
    let clear_realpath = args.first().map(|v| v.is_truthy()).unwrap_or(false);
    if clear_realpath {
        let filename = args
            .get(1)
            .map(|v| v.to_php_string())
            .unwrap_or_else(PhpString::empty);
        REALPATH_CACHE.with(|c| {
            let mut c = c.borrow_mut();
            if filename.is_empty() {
                c.clear();
            } else {
                c.retain(|e| e.path != filename.as_bytes());
            }
        });
    }
    Ok(Value::Null)
}

fn array_walk_recursive_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.len() > 3 {
        let msg = format!(
            "array_walk_recursive() expects at most 3 arguments, {} given",
            args.len()
        );
        let exc = vm.create_exception(b"ArgumentCountError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    let arr_val = args.first().unwrap_or(&Value::Null);
    let callback_v = args
        .get(1)
        .map(|c| c.unwrap_closure())
        .unwrap_or(Value::Null);
    let callback = &callback_v;
    let extra_data = args.get(2).cloned();
    if matches!(arr_val.deref(), Value::Object(_)) {
        // Objects are walked over their properties like array_walk().
        return array_walk(vm, args);
    }
    if !matches!(arr_val.deref(), Value::Array(_)) {
        require_array_arg(vm, arr_val, "array_walk_recursive", "array", 1)?;
        return Ok(Value::True);
    }

    // Extract callback function name and captured vars
    let (func_name, captured) = match callback {
        Value::String(s) => (s.as_bytes().to_vec(), vec![]),
        Value::Array(cb) => {
            let cb = cb.borrow();
            let vals: Vec<Value> = cb.values().cloned().collect();
            if vals.is_empty() {
                return Ok(Value::True);
            }
            (
                vals[0].to_php_string().as_bytes().to_vec(),
                vals[1..].to_vec(),
            )
        }
        _ => return Ok(Value::True),
    };

    let _ = (&func_name, &captured);
    if let Value::Array(a) = arr_val.deref() {
        if let Some(reason) = callable_error(vm, callback) {
            let msg = format!(
                "array_walk_recursive(): Argument #2 ($callback) must be a valid callback, {}",
                reason
            );
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
        let holder = match arr_val {
            Value::Reference(r) => Some(r),
            _ => None,
        };
        php_array_walk_arr(vm, a, holder, callback, extra_data.as_ref(), true, 0)?;
    }
    Ok(Value::True)
}

fn fileperms_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let path_str = path.to_string_lossy();
    if path_str.is_empty() {
        return Ok(Value::False);
    }
    if path.as_bytes().contains(&0) {
        let line = vm.current_line;
        vm.emit_warning_at("fileperms(): Filename contains null byte", line);
        return Ok(Value::False);
    }
    if !vm.check_open_basedir("fileperms", &path_str) {
        return Ok(Value::False);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match std::fs::metadata(&*path_str) {
            Ok(m) => Ok(Value::Long(m.mode() as i64)),
            Err(_) => {
                vm.emit_warning(&format!("fileperms(): stat failed for {}", path_str));
                Ok(Value::False)
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path_str;
        Ok(Value::Long(0o100644))
    }
}

fn filetype_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let path_str = path.to_string_lossy();
    let p = std::path::Path::new(&*path_str);
    // Use symlink_metadata so that symlinks are reported as "link" (not whatever they point to)
    let md = match std::fs::symlink_metadata(p) {
        Ok(m) => m,
        Err(_) => {
            if !path_str.is_empty() {
                vm.emit_warning(&format!("filetype(): Lstat failed for {}", path_str));
            }
            return Ok(Value::False);
        }
    };
    let ft_type = md.file_type();
    let ft = if ft_type.is_symlink() {
        "link"
    } else if ft_type.is_file() {
        "file"
    } else if ft_type.is_dir() {
        "dir"
    } else {
        // Unix-specific: detect character/block/fifo/socket devices
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileTypeExt;
            if ft_type.is_char_device() {
                "char"
            } else if ft_type.is_block_device() {
                "block"
            } else if ft_type.is_fifo() {
                "fifo"
            } else if ft_type.is_socket() {
                "socket"
            } else {
                "unknown"
            }
        }
        #[cfg(not(unix))]
        {
            "unknown"
        }
    };
    Ok(Value::String(PhpString::from_bytes(ft.as_bytes())))
}

/// php_natsort(): zend_hash_sort with php_array_natural_(case_)compare, keys kept.
fn php_natsort(
    vm: &mut Vm,
    args: &[Value],
    fname: &str,
    fold_case: bool,
) -> Result<Value, VmError> {
    let first = args.first().unwrap_or(&Value::Null);
    require_array_arg(vm, first, fname, "array", 1)?;
    if let Value::Array(arr) = first.deref() {
        let entries: Vec<(ArrayKey, Value)> = arr
            .borrow()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        if entries.len() <= 1 {
            return Ok(Value::True);
        }
        let flags = if fold_case {
            SORT_NATURAL | SORT_FLAG_CASE
        } else {
            SORT_NATURAL
        };
        let next_free = arr.borrow().next_free_key();
        let sorted =
            crate::zsort::zend_sort_stable(entries, |x, y| php_data_cmp(vm, &x.1, &y.1, flags));
        let mut new_arr = PhpArray::new();
        for (k, v) in sorted {
            new_arr.set(k, v);
        }
        new_arr.set_next_free_key(next_free);
        *arr.borrow_mut() = new_arr;
    }
    Ok(Value::True)
}

fn natsort_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    php_natsort(vm, args, "natsort", false)
}

fn natcasesort_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    php_natsort(vm, args, "natcasesort", true)
}

fn set_include_path_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    // OnUpdateStringUnempty
    if path.as_bytes().is_empty() {
        return Ok(Value::False);
    }
    ini_remember(vm, b"include_path");
    let old = vm
        .constants
        .get(b"include_path".as_ref())
        .cloned()
        .unwrap_or_else(|| Value::String(PhpString::from_bytes(b".")));
    vm.constants.insert(
        b"include_path".to_vec(),
        Value::String(PhpString::from_vec(path.as_bytes().to_vec())),
    );
    Ok(old)
}

fn get_include_path_fn(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(vm
        .constants
        .get(b"include_path".as_ref())
        .cloned()
        .unwrap_or_else(|| Value::String(PhpString::from_bytes(b"."))))
}

fn restore_include_path_fn(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    ini_restore(vm, &[Value::String(PhpString::from_bytes(b"include_path"))])
}

fn get_resource_type_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    resource_param(vm, val, "get_resource_type")?;
    let val = &val.deref();
    // For closed resources, return "Unknown" (matches PHP behavior).
    if let Value::Resource(id) = val {
        if goro_core::value::is_registered_resource(id.id())
            && !goro_core::value::is_open_resource(id.id())
        {
            return Ok(Value::String(PhpString::from_bytes(b"Unknown")));
        }
    }
    if let Some(t) = val.resource_type_name() {
        return Ok(Value::String(PhpString::from_vec(t)));
    }
    Ok(Value::String(PhpString::from_bytes(b"Unknown")))
}

fn link_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let target = args.first().unwrap_or(&Value::Null).to_php_string();
    let link = args.get(1).unwrap_or(&Value::Null).to_php_string();
    if let Some(e) = path_arg_nul_error(vm, target.as_bytes(), "link", 1, "target")
        .or_else(|| path_arg_nul_error(vm, link.as_bytes(), "link", 2, "link"))
    {
        return Err(e);
    }
    match std::fs::hard_link(&*target.to_string_lossy(), &*link.to_string_lossy()) {
        Ok(_) => Ok(Value::True),
        Err(e) => {
            let msg = strip_os_error(&e.to_string());
            vm.emit_warning(&format!("link(): {}", msg));
            Ok(Value::False)
        }
    }
}

fn unlink_fn2(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // php_clear_stat_cache(1, ...): the realpath cache is dropped.
    REALPATH_CACHE.with(|c| c.borrow_mut().clear());
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    if path.as_bytes().contains(&0) {
        let msg = "unlink(): Argument #1 ($filename) must not contain any null bytes".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    let path_str = path.to_string_lossy();
    match std::fs::remove_file(&*path_str) {
        Ok(_) => Ok(Value::True),
        Err(e) => {
            let msg = strip_os_error(&e.to_string());
            vm.emit_warning(&format!("unlink({}): {}", path_str, msg));
            Ok(Value::False)
        }
    }
}

fn rename_fn2(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // php_clear_stat_cache(1, ...): the realpath cache is dropped.
    REALPATH_CACHE.with(|c| c.borrow_mut().clear());
    let from = args.first().unwrap_or(&Value::Null).to_php_string();
    let to = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let from_str = from.to_string_lossy();
    let to_str = to.to_string_lossy();
    match std::fs::rename(&*from_str, &*to_str) {
        Ok(_) => Ok(Value::True),
        Err(e) => {
            let msg = strip_os_error(&e.to_string());
            vm.emit_warning(&format!("rename({},{}): {}", from_str, to_str, msg));
            Ok(Value::False)
        }
    }
}

fn copy_fn2(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let from = args.first().unwrap_or(&Value::Null).to_php_string();
    let to = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let from_str = from.to_string_lossy();
    let to_str = to.to_string_lossy();
    // PHP: if source and destination resolve to the same file (using cwd to
    // resolve relative paths, lexically normalized), return false with NO
    // warning, regardless of whether either file exists.
    let resolve_rel = |p: &str| -> String {
        let norm = normalize_path_lexical(p);
        if norm.starts_with('/') {
            norm
        } else if let Ok(cwd) = std::env::current_dir() {
            normalize_path_lexical(&format!("{}/{}", cwd.display(), norm))
        } else {
            norm
        }
    };
    if resolve_rel(&from_str) == resolve_rel(&to_str) {
        return Ok(Value::False);
    }
    // php_copy_file_ctx() through the stream layer for data:/php:// sources.
    let fb = from.as_bytes();
    if fb.starts_with(b"data:") || (fb.len() > 6 && fb[..6].eq_ignore_ascii_case(b"php://")) {
        let fp = fopen_fn(
            vm,
            &[
                Value::String(from.clone()),
                Value::String(PhpString::from_bytes(b"rb")),
            ],
        )?;
        let fid = match fp {
            Value::Resource(n) => n.id(),
            _ => return Ok(Value::False),
        };
        let data = stream_get_contents_fn(vm, &[Value::resource(fid)])?;
        let _ = fclose_fn(vm, &[Value::resource(fid)]);
        if !vm.check_open_basedir("copy", &to_str) {
            return Ok(Value::False);
        }
        return Ok(
            match std::fs::write(&*to_str, data.to_php_string().as_bytes()) {
                Ok(_) => Value::True,
                Err(e) => {
                    vm.emit_warning(&format!(
                        "copy({}): Failed to open stream: {}",
                        to_str,
                        strip_os_error(&e.to_string())
                    ));
                    Value::False
                }
            },
        );
    }
    let from_path = std::path::Path::new(&*from_str);
    if !from_path.exists() {
        vm.emit_warning(&format!(
            "copy({}): Failed to open stream: No such file or directory",
            from_str
        ));
        return Ok(Value::False);
    }
    // PHP: source cannot be a directory
    if from_path.is_dir() {
        vm.emit_warning("copy(): The first argument to copy() function cannot be a directory");
        return Ok(Value::False);
    }
    // PHP: destination cannot be a directory
    if std::path::Path::new(&*to_str).is_dir() {
        vm.emit_warning("copy(): The second argument to copy() function cannot be a directory");
        return Ok(Value::False);
    }
    // PHP: copy of same source -> destination returns false (no warning),
    // resolved via canonicalize as a fallback.
    if let (Ok(c1), Ok(c2)) = (
        std::fs::canonicalize(&*from_str),
        std::fs::canonicalize(&*to_str),
    ) {
        if c1 == c2 {
            return Ok(Value::False);
        }
    }
    // php_copy_file_ctx(): the source is read and the destination opened
    // like fopen($to, "wb") (a new file gets 0666 & ~umask, an existing one
    // keeps its permissions; std::fs::copy() would copy the source's).
    let src = match std::fs::File::open(&*from_str) {
        Ok(f) => f,
        Err(e) => {
            let msg = strip_os_error(&e.to_string());
            vm.emit_warning(&format!(
                "copy({}): Failed to open stream: {}",
                from_str, msg
            ));
            return Ok(Value::False);
        }
    };
    let dst = match std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&*to_str)
    {
        Ok(f) => f,
        Err(e) => {
            let msg = strip_os_error(&e.to_string());
            vm.emit_warning(&format!("copy({}): Failed to open stream: {}", to_str, msg));
            return Ok(Value::False);
        }
    };
    // (read()/write() in chunks, as the stream copy does)
    let (mut src, mut dst) = (src, dst);
    let mut buf = vec![0u8; 8192];
    loop {
        use std::io::{Read, Write};
        match src.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if dst.write_all(&buf[..n]).is_err() {
                    return Ok(Value::False);
                }
            }
            Err(_) => return Ok(Value::False),
        }
    }
    Ok(Value::True)
}

fn readfile_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    if let Some(e) = path_arg_nul_error(vm, path.as_bytes(), "readfile", 1, "filename") {
        return Err(e);
    }
    let path_str = path.to_string_lossy();
    if path_str.is_empty() {
        let msg = "Path must not be empty".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    let use_include_path = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
    // A stream wrapper URL (data:, php://, userspace wrappers, ...) is opened
    // like fopen() does and passed through (php_stream_open_wrapper_ex()).
    if path.as_bytes().starts_with(b"data:") || path_str.contains("://") {
        let mut fargs = vec![
            Value::String(path.clone()),
            Value::String(PhpString::from_bytes(b"rb")),
            if use_include_path {
                Value::True
            } else {
                Value::False
            },
        ];
        if let Some(ctx) = args.get(2) {
            fargs.push(ctx.clone());
        }
        let fp = call_registered(vm, b"fopen", fopen_fn, &fargs)?;
        let Value::Resource(r) = fp else {
            return Ok(Value::False);
        };
        let fid = r.id();
        let res = call_registered(vm, b"fpassthru", fpassthru_fn, &[Value::resource(fid)])?;
        let _ = call_registered(vm, b"fclose", fclose_fn, &[Value::resource(fid)]);
        return Ok(res);
    }
    let resolved = resolve_with_include_path(vm, &path_str, use_include_path);
    // A directory opens as a plain-file stream whose first read fails.
    if std::path::Path::new(&resolved).is_dir() {
        goro_core::value::alloc_resource_id();
        let line = vm.current_line;
        vm.emit_notice_at(
            "readfile(): Read of 8192 bytes failed with errno=21 Is a directory",
            line,
        );
        return Ok(Value::Long(-1));
    }
    match read_path_with_wrappers(&resolved) {
        Ok(data) => {
            let len = data.len();
            vm.write_output(&data);
            Ok(Value::Long(len as i64))
        }
        Err(_) => {
            vm.emit_warning(&format!(
                "readfile({}): Failed to open stream: No such file or directory",
                path_str
            ));
            Ok(Value::False)
        }
    }
}

pub(crate) fn resolve_with_include_path(vm: &Vm, filename: &str, use_include_path: bool) -> String {
    if !use_include_path
        || filename.contains("://")
        || filename.starts_with('/')
        || filename.starts_with("./")
        || filename.starts_with("../")
    {
        return filename.to_string();
    }
    let inc = vm
        .constants
        .get(b"include_path".as_ref())
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_else(|| ".".to_string());
    for entry in inc.split(':') {
        if entry.is_empty() {
            continue;
        }
        let candidate = if entry == "." {
            filename.to_string()
        } else {
            format!("{}/{}", entry.trim_end_matches('/'), filename)
        };
        if std::fs::metadata(&candidate).is_ok() {
            return candidate;
        }
    }
    let script = vm.current_file.clone();
    if let Some(parent) = std::path::Path::new(&script).parent() {
        let candidate = parent.join(filename);
        if candidate.exists() {
            return candidate.to_string_lossy().to_string();
        }
    }
    filename.to_string()
}

fn file_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let flags = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    // Valid flags for file(): FILE_USE_INCLUDE_PATH (1) | FILE_IGNORE_NEW_LINES (2)
    // | FILE_SKIP_EMPTY_LINES (4) | FILE_NO_DEFAULT_CONTEXT (16) = 0x17.
    if flags & !0x17 != 0 {
        let msg = "file(): Argument #2 ($flags) must be a valid flag value".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    let ignore_new_lines = flags & 2 != 0; // FILE_IGNORE_NEW_LINES
    let skip_empty = flags & 4 != 0; // FILE_SKIP_EMPTY_LINES
    let use_inc = flags & 1 != 0; // FILE_USE_INCLUDE_PATH
    let path_str = path.to_string_lossy();
    let path_str = resolve_with_include_path(vm, &path_str, use_inc);
    // A directory opens as a plain-file stream whose first read fails.
    if !path_str.contains("://") && std::path::Path::new(&*path_str).is_dir() {
        goro_core::value::alloc_resource_id();
        let line = vm.current_line;
        vm.emit_notice_at(
            "file(): Read of 8192 bytes failed with errno=21 Is a directory",
            line,
        );
        return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
    }
    // php:// streams (php://filter, php://memory, ...) go through the
    // stream layer.
    let content = if (path_str.len() > 6 && path_str[..6].eq_ignore_ascii_case("php://"))
        || path.as_bytes().starts_with(b"data:")
    {
        let mut oargs = vec![
            Value::String(path.clone()),
            Value::String(PhpString::from_bytes(b"rb")),
            if use_inc { Value::True } else { Value::False },
        ];
        if let Some(ctx) = args.get(2) {
            oargs.push(ctx.clone());
        }
        let fp = call_registered(vm, b"fopen", fopen_fn, &oargs)?;
        let fid = match fp {
            Value::Resource(n) => n.id(),
            _ => return Ok(Value::False),
        };
        let r = call_registered(
            vm,
            b"stream_get_contents",
            stream_get_contents_fn,
            &[Value::resource(fid)],
        );
        let _ = call_registered(vm, b"fclose", fclose_fn, &[Value::resource(fid)]);
        match r? {
            Value::String(s) => Ok(s.as_bytes().to_vec()),
            _ => Ok(Vec::new()),
        }
    } else {
        read_path_with_wrappers(&path_str)
    };
    match content {
        Ok(content) => {
            let mut result = PhpArray::new();
            if content.is_empty() {
                return Ok(Value::Array(Rc::new(RefCell::new(result))));
            }
            // PHP_FUNCTION(file): split at '\n'; without FILE_IGNORE_NEW_LINES
            // lines keep their terminator. A trailing piece without a newline
            // is added as is.
            let mut s = 0usize;
            let n = content.len();
            let mut p = content.iter().position(|&c| c == b'\n');
            if p.is_none() {
                result.push(Value::String(PhpString::from_vec(content)));
                return Ok(Value::Array(Rc::new(RefCell::new(result))));
            }
            while let Some(pos) = p {
                if !ignore_new_lines {
                    result.push(Value::String(PhpString::from_bytes(&content[s..=pos])));
                } else {
                    let windows_eol = (pos > 0 && content[pos - 1] == b'\r') as usize;
                    if !(skip_empty && pos - s == windows_eol) {
                        result.push(Value::String(PhpString::from_bytes(
                            &content[s..pos - windows_eol],
                        )));
                    }
                }
                s = pos + 1;
                p = content[s..].iter().position(|&c| c == b'\n').map(|q| q + s);
            }
            if s != n {
                result.push(Value::String(PhpString::from_bytes(&content[s..])));
            }
            Ok(Value::Array(Rc::new(RefCell::new(result))))
        }
        Err(_) => {
            vm.emit_warning(&format!(
                "file({}): Failed to open stream: No such file or directory",
                path_str
            ));
            Ok(Value::False)
        }
    }
}

fn lstat_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let path_str = path.to_string_lossy();
    let res = stat_path(&path_str, true)?;
    if matches!(res, Value::False) && !path_str.is_empty() {
        vm.emit_warning(&format!("lstat(): Lstat failed for {}", path_str));
    }
    Ok(res)
}

fn is_executable_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    #[cfg(unix)]
    {
        Ok(
            if check_access_bit(&path.to_string_lossy(), 0o100, 0o010, 0o001) {
                Value::True
            } else {
                Value::False
            },
        )
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(Value::False)
    }
}

fn tempnam_fn3(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Validate prefix type: must be a stringable scalar; arrays cause TypeError.
    let prefix_val = args.get(1).cloned().unwrap_or(Value::Null);
    if matches!(prefix_val, Value::Array(_)) {
        let msg =
            "tempnam(): Argument #2 ($prefix) must be of type string, array given".to_string();
        let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    let prefix = prefix_val.to_php_string();
    // Reject null bytes in prefix.
    if prefix.as_bytes().contains(&0) {
        let msg = "tempnam(): Argument #2 ($prefix) must not contain any null bytes".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    let dir = args.first().unwrap_or(&Value::Null).to_php_string();
    if let Some(e) = path_arg_nul_error(vm, dir.as_bytes(), "tempnam", 1, "directory") {
        return Err(e);
    }
    // php_basename() of the prefix, at most 63 bytes.
    let prefix_bytes = prefix.as_bytes();
    let mut pfx: Vec<u8> = match prefix_bytes.iter().rposition(|&c| c == b'/') {
        Some(pos) => prefix_bytes[pos + 1..].to_vec(),
        None => prefix_bytes.to_vec(),
    };
    pfx.truncate(63);

    // php_open_temporary_fd_ex(dir, pfx, PHP_TMP_FILE_OPEN_BASEDIR_CHECK_ALWAYS)
    let dir_s = dir.to_string_lossy();
    if !dir_s.is_empty() {
        if !vm.check_open_basedir("tempnam", &dir_s) {
            return Ok(Value::False);
        }
        if let Some(p) = do_open_temporary_file(vm, &dir_s, &pfx) {
            return Ok(Value::String(PhpString::from_vec(p)));
        }
        let line = vm.current_line;
        vm.emit_notice_at(
            "tempnam(): file created in the system's temporary directory",
            line,
        );
    }
    let temp_dir = php_get_temporary_directory(vm);
    if temp_dir.is_empty() || !vm.check_open_basedir("tempnam", &temp_dir) {
        return Ok(Value::False);
    }
    match do_open_temporary_file(vm, &temp_dir, &pfx) {
        Some(p) => Ok(Value::String(PhpString::from_vec(p))),
        None => Ok(Value::False),
    }
}

/// php_get_temporary_directory(): sys_temp_dir, $TMPDIR or /tmp, without
/// a trailing slash.
pub(crate) fn php_get_temporary_directory(vm: &Vm) -> String {
    let sys = vm
        .constants
        .get(b"sys_temp_dir".as_slice())
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_default();
    let pick = |s: String| -> Option<String> {
        if s.len() >= 2 && s.ends_with('/') {
            Some(s[..s.len() - 1].to_string())
        } else if !s.is_empty() && !s.ends_with('/') {
            Some(s)
        } else {
            None
        }
    };
    if let Some(d) = pick(sys) {
        return d;
    }
    if let Some(d) = std::env::var("TMPDIR").ok().filter(|s| !s.is_empty()) {
        let d = if d.len() > 1 {
            d.trim_end_matches('/').to_string()
        } else {
            d
        };
        return if d.is_empty() { "/".to_string() } else { d };
    }
    "/tmp".to_string()
}

/// php_do_open_temporary_file(): mkstemp() of "<realpath(dir)>/<pfx>XXXXXX".
fn do_open_temporary_file(_vm: &mut Vm, dir: &str, pfx: &[u8]) -> Option<Vec<u8>> {
    use std::os::unix::ffi::OsStrExt;
    let real = std::fs::canonicalize(dir).ok()?;
    let mut path = real.as_os_str().as_bytes().to_vec();
    if path.last() != Some(&b'/') {
        path.push(b'/');
    }
    path.extend_from_slice(pfx);
    // The prefix is extended with 13 base32 digits of a random number.
    const B32: &[u8; 32] = b"0123456789abcdefghijklmnopqrstuv";
    let mut random = [0u8; 8];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let _ = f.read_exact(&mut random);
    }
    let mut r = u64::from_le_bytes(random);
    for _ in 0..13 {
        path.push(B32[(r % 32) as usize]);
        r /= 32;
    }
    path.extend_from_slice(b"XXXXXX");
    if path.len() >= 4096 {
        return None;
    }
    let mut buf = path.clone();
    buf.push(0);
    let fd = unsafe { libc::mkstemp(buf.as_mut_ptr() as *mut libc::c_char) };
    if fd < 0 {
        return None;
    }
    unsafe {
        libc::close(fd);
    }
    buf.pop();
    Some(buf)
}

fn sys_get_temp_dir_fn3(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(PhpString::from_string(
        php_get_temporary_directory(vm),
    )))
}

fn array_replace_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let mut result = PhpArray::new();
    for arg in args {
        if let Value::Array(arr) = arg {
            let arr = arr.borrow();
            for (key, value) in arr.iter() {
                // (zend_array_dup() / zval_add_ref(): a reference only the
                // array holds is copied as its value)
                result.set(key.clone(), unwrap_rc1_ref(value));
            }
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn array_replace_recursive_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
    }

    let mut result = if let Value::Array(arr) = &args[0] {
        arr.borrow().clone()
    } else {
        PhpArray::new()
    };

    for arg in &args[1..] {
        let arg_deref = arg.deref();
        if let Value::Array(arr) = &arg_deref {
            let arr_ptr = Rc::as_ptr(arr) as usize;
            let entries: Vec<_> = arr
                .borrow()
                .iter()
                .map(|(k, v)| (k.clone(), unwrap_rc1_ref(v)))
                .collect();
            for (key, value) in entries {
                let value_deref = value.deref();
                if let Value::Array(inner_new) = &value_deref {
                    let inner_new_ptr = Rc::as_ptr(inner_new) as usize;
                    // Detect direct self-reference before recursing.
                    if inner_new_ptr == arr_ptr {
                        let e = "Recursion detected".to_string();
                        let exc = vm.create_exception(b"Error", &e, vm.current_line);
                        vm.current_exception = Some(exc);
                        return Err(VmError {
                            message: format!("Uncaught Error: {}", e),
                            line: vm.current_line,
                        });
                    }
                    let existing_inner = if let Some(existing) = result.get(&key) {
                        let existing = existing.deref();
                        if let Value::Array(inner_old) = existing {
                            Some(inner_old.clone())
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    if let Some(inner_old) = existing_inner {
                        let mut visited = vec![arr_ptr, inner_new_ptr];
                        match array_replace_recursive_inner(
                            &inner_old.borrow(),
                            inner_new,
                            &mut visited,
                        ) {
                            Ok(merged_val) => {
                                result.set(key.clone(), merged_val);
                                continue;
                            }
                            Err(e) => {
                                let exc = vm.create_exception(b"Error", &e, vm.current_line);
                                vm.current_exception = Some(exc);
                                return Err(VmError {
                                    message: format!("Uncaught Error: {}", e),
                                    line: vm.current_line,
                                });
                            }
                        }
                    }
                }
                result.set(key.clone(), value.clone());
            }
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn array_replace_recursive_inner(
    base: &PhpArray,
    replacement: &Rc<RefCell<PhpArray>>,
    visited: &mut Vec<usize>,
) -> Result<Value, String> {
    let mut result = base.clone();
    let replacement_borrow = replacement.borrow();
    for (key, value) in replacement_borrow.iter() {
        // Unwrap references so that recursive structures created via
        // `$x[] = &$x;` are detected via their inner Rc pointer.
        let value_owned = value.deref();
        if let Value::Array(inner_new) = &value_owned {
            let ptr = Rc::as_ptr(inner_new) as usize;
            if visited.contains(&ptr) {
                return Err("Recursion detected".to_string());
            }
            if let Some(existing) = result.get(key) {
                let existing = existing.deref();
                if let Value::Array(inner_old) = existing {
                    visited.push(ptr);
                    let merged =
                        array_replace_recursive_inner(&inner_old.borrow(), inner_new, visited)?;
                    visited.pop();
                    result.set(key.clone(), merged);
                    continue;
                }
            }
        }
        result.set(key.clone(), unwrap_rc1_ref(value));
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

/// A reference held only by its container (refcount 1) is copied as a
/// plain value, like zend_array_dup()/php_array_merge_recursive do.
fn unwrap_rc1_ref(v: &Value) -> Value {
    if let Value::Reference(r) = v {
        if Rc::strong_count(r) == 1 {
            return r.borrow().clone();
        }
    }
    v.clone()
}

fn array_find_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let arr = match args.first() {
        Some(Value::Array(arr)) => arr.clone(),
        _ => return Ok(Value::Null),
    };
    let callback = match args.get(1) {
        Some(v) => v.clone(),
        _ => return Ok(Value::Null),
    };
    let arr_data: Vec<(ArrayKey, Value)> = arr
        .borrow()
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    for (key, value) in &arr_data {
        let key_val = match key {
            ArrayKey::Int(n) => Value::Long(*n),
            ArrayKey::String(s) => Value::String(s.clone()),
        };
        let result = call_user_func(vm, &[callback.clone(), value.clone(), key_val])?;
        if result.is_truthy() {
            return Ok(value.deref());
        }
    }
    Ok(Value::Null)
}

fn array_find_key_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let arr = match args.first() {
        Some(Value::Array(arr)) => arr.clone(),
        _ => return Ok(Value::Null),
    };
    let callback = match args.get(1) {
        Some(v) => v.clone(),
        _ => return Ok(Value::Null),
    };
    let arr_data: Vec<(ArrayKey, Value)> = arr
        .borrow()
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    for (key, value) in &arr_data {
        let key_val = match key {
            ArrayKey::Int(n) => Value::Long(*n),
            ArrayKey::String(s) => Value::String(s.clone()),
        };
        let result = call_user_func(vm, &[callback.clone(), value.clone(), key_val])?;
        if result.is_truthy() {
            return Ok(match key {
                ArrayKey::Int(n) => Value::Long(*n),
                ArrayKey::String(s) => Value::String(s.clone()),
            });
        }
    }
    Ok(Value::Null)
}

/// php_array_binop() (array_sum / array_product).
fn php_array_binop(
    vm: &mut Vm,
    args: &[Value],
    fname: &str,
    op_name: &str,
    mul: bool,
) -> Result<Value, VmError> {
    let first = args.first().unwrap_or(&Value::Null).deref();
    require_array_arg(vm, &first, fname, "array", 1)?;
    let entries: Vec<Value> = match &first {
        Value::Array(a) => a.borrow().values().cloned().collect(),
        _ => return Ok(Value::Long(0)),
    };
    let mut acc = Value::Long(if mul { 1 } else { 0 });
    let apply = |acc: &Value, n: &Value| -> Value {
        match (acc, n) {
            (Value::Long(x), Value::Long(y)) => {
                let r = if mul {
                    x.checked_mul(*y)
                } else {
                    x.checked_add(*y)
                };
                match r {
                    Some(v) => Value::Long(v),
                    None => Value::Double(if mul {
                        *x as f64 * *y as f64
                    } else {
                        *x as f64 + *y as f64
                    }),
                }
            }
            _ => {
                let (x, y) = (acc.to_double(), n.to_double());
                Value::Double(if mul { x * y } else { x + y })
            }
        }
    };
    for entry in entries {
        let v = entry.deref();
        let num: Option<Value> = match &v {
            Value::Long(_) | Value::Double(_) => Some(v.clone()),
            Value::Null | Value::Undef | Value::False => Some(Value::Long(0)),
            Value::True => Some(Value::Long(1)),
            Value::String(s) => {
                let st = s.to_string_lossy();
                match goro_core::value::parse_leading_numeric_string(s.as_bytes()) {
                    Some((f, fully)) => {
                        if !fully {
                            vm.emit_warning("A non-numeric value encountered");
                        }
                        let t = st.trim_start();
                        let numpart: String = t
                            .chars()
                            .take_while(|c| c.is_ascii_digit() || *c == '-' || *c == '+')
                            .collect();
                        let is_int = !t[numpart.len()..].starts_with(['.', 'e', 'E']);
                        match (is_int, numpart.parse::<i64>()) {
                            (true, Ok(n)) => Some(Value::Long(n)),
                            _ => Some(Value::Double(f)),
                        }
                    }
                    None => {
                        acc = apply(&acc, &Value::Long(0));
                        vm.emit_warning(&format!(
                            "{}(): {} is not supported on type string",
                            fname, op_name
                        ));
                        None
                    }
                }
            }
            Value::Resource(id) => {
                acc = apply(&acc, &Value::Long(id.id()));
                vm.emit_warning(&format!(
                    "{}(): {} is not supported on type resource",
                    fname, op_name
                ));
                None
            }
            Value::Object(o) => {
                let cast = vm.object_to_long.and_then(|f| f(&v));
                match cast {
                    Some(n) if vm.object_is_operator_type.map(|f| f(&v)).unwrap_or(false) => {
                        Some(Value::Long(n))
                    }
                    _ => {
                        let name = goro_core::value::display_class_name(&o.borrow().class_name);
                        vm.emit_warning(&format!(
                            "{}(): {} is not supported on type {}",
                            fname, op_name, name
                        ));
                        None
                    }
                }
            }
            _ => {
                vm.emit_warning(&format!(
                    "{}(): {} is not supported on type {}",
                    fname,
                    op_name,
                    Vm::value_type_name(&v)
                ));
                None
            }
        };
        if let Some(n) = num {
            acc = apply(&acc, &n);
        }
    }
    Ok(acc)
}

fn array_product_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    php_array_binop(vm, args, "array_product", "Multiplication", true)
}

fn array_sum_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    php_array_binop(vm, args, "array_sum", "Addition", false)
}

fn parse_ini_string_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = match args.first().map(|v| v.deref()) {
        Some(v) => v.to_php_string().as_bytes().to_vec(),
        None => return Ok(Value::False),
    };
    let process_sections = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
    let scanner_mode = args
        .get(2)
        .map(|v| v.to_long())
        .unwrap_or(crate::ini_parser::SCANNER_NORMAL);
    Ok(crate::ini_parser::parse_ini(
        vm,
        &data,
        process_sections,
        scanner_mode,
        "Unknown",
    ))
}

fn assert_options_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let option = args.first().map(|v| v.to_long()).unwrap_or(0);
    let new_value = args.get(1).map(|v| v.deref());

    // The ASSERT_* constant access happens before the call itself.
    let const_name = match option {
        1 => Some("ASSERT_ACTIVE"),
        2 => Some("ASSERT_CALLBACK"),
        3 => Some("ASSERT_BAIL"),
        4 => Some("ASSERT_WARNING"),
        5 => Some("ASSERT_EXCEPTION"),
        _ => None,
    };
    if let Some(name) = const_name {
        vm.emit_deprecated(&format!(
            "Constant {} is deprecated since 8.3, as assert_options() is deprecated",
            name
        ));
    }
    vm.emit_deprecated("Function assert_options() is deprecated since 8.3");

    let (key, default): (&[u8], bool) = match option {
        1 => (b"assert.active", true),
        3 => (b"assert.bail", false),
        4 => (b"assert.warning", true),
        5 => (b"assert.exception", true),
        2 => {
            // ASSERT_CALLBACK
            let prev = ASSERT_CALLBACK
                .with(|c| c.borrow().clone())
                .unwrap_or_else(|| match vm.constants.get(b"assert.callback".as_ref()) {
                    Some(v) if !v.to_php_string().as_bytes().is_empty() => {
                        Value::String(v.to_php_string())
                    }
                    _ => Value::Null,
                });
            if let Some(nv) = new_value {
                ASSERT_CALLBACK.with(|c| {
                    *c.borrow_mut() = if matches!(nv, Value::Null) {
                        None
                    } else {
                        Some(nv)
                    }
                });
            }
            return Ok(prev);
        }
        _ => {
            let msg = "assert_options(): Argument #1 ($option) must be an ASSERT_* constant";
            let exc = vm.create_exception(b"ValueError", msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: format!("Uncaught ValueError: {}", msg),
                line: vm.current_line,
            });
        }
    };
    let old = assert_ini_bool(vm, key, default);
    if let Some(nv) = new_value {
        let sv = match &nv {
            Value::Object(_) | Value::Array(_) => nv.to_php_string(),
            other => other.to_php_string(),
        };
        vm.constants.insert(key.to_vec(), Value::String(sv));
    }
    Ok(Value::Long(old as i64))
}

fn ftruncate_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let arg = args.first().unwrap_or(&Value::Null);
    let fid = require_open_stream(vm, arg, "ftruncate")?;
    let size_long = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    if size_long < 0 {
        let msg = "ftruncate(): Argument #2 ($size) must be greater than or equal to 0".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    let size = size_long as u64;
    // php://memory (and php://temp while it is held in memory): growing is
    // a zend_string_realloc() of the buffer.
    let in_memory_len = FILE_HANDLES.with(|handles| {
        let handles = handles.borrow();
        let fh = handles.get(&fid)?;
        let len = fh.file.metadata().map(|m| m.len()).unwrap_or(0);
        match fh.special {
            SpecialStream::Memory => Some(len),
            SpecialStream::Temp if len <= 2 * 1024 * 1024 => Some(len),
            _ => None,
        }
    });
    if let Some(len) = in_memory_len
        && size > len
    {
        vm.memory_check((size as usize).saturating_add(25 + 7) & !7)?;
    }
    FILE_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(fh) = handles.get_mut(&fid) {
            match fh.file.set_len(size) {
                Ok(_) => {
                    // A memory stream's own offset is clamped to the new
                    // size, while stream->position (ftell()) stays: it then
                    // advances from there by what is read and written.
                    if matches!(fh.special, SpecialStream::Memory | SpecialStream::Temp) {
                        let logical = fh.append_position().or_else(|| fh.logical_os_pos());
                        if let Ok(pos) = fh.file.stream_position() {
                            if pos > size {
                                let _ = fh.file.seek(std::io::SeekFrom::Start(size));
                                fh.append_pos = logical.map(|l| (l, size));
                            }
                        }
                    }
                    Ok(Value::True)
                }
                Err(_) => Ok(Value::False),
            }
        } else {
            Ok(Value::False)
        }
    })
}

fn tmpfile_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // Create a temporary file
    // Use libc mkstemp for a proper temp file
    let template = b"/tmp/goro_tmp_XXXXXX\0";
    let mut buf = template.to_vec();
    let fd = unsafe { libc::mkstemp(buf.as_mut_ptr() as *mut libc::c_char) };
    if fd < 0 {
        return Ok(Value::False);
    }
    // Remove the file immediately so it's automatically cleaned up
    let path = std::ffi::CStr::from_bytes_with_nul(&buf).unwrap();
    unsafe {
        libc::unlink(path.as_ptr());
    }
    // Convert fd to std::fs::File
    use std::os::unix::io::FromRawFd;
    let file = unsafe { std::fs::File::from_raw_fd(fd) };
    let fid = alloc_file_handle(file, "w+b");
    goro_core::value::register_resource(fid, b"stream");
    Ok(Value::resource(fid))
}

fn filemtime_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let path_str = path.to_string_lossy();
    if path_str.is_empty() {
        return Ok(Value::False);
    }
    if !vm.check_open_basedir("filemtime", &path_str) {
        return Ok(Value::False);
    }
    match std::fs::metadata(&*path_str) {
        Ok(meta) => {
            if let Ok(modified) = meta.modified() {
                let secs = modified
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                Ok(Value::Long(secs as i64))
            } else {
                Ok(Value::False)
            }
        }
        Err(_) => {
            vm.emit_warning(&format!("filemtime(): stat failed for {}", path_str));
            Ok(Value::False)
        }
    }
}

fn fileatime_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let path_str = path.to_string_lossy();
    if path_str.is_empty() {
        return Ok(Value::False);
    }
    if !vm.check_open_basedir("fileatime", &path_str) {
        return Ok(Value::False);
    }
    match std::fs::metadata(&*path_str) {
        Ok(meta) => {
            if let Ok(accessed) = meta.accessed() {
                let secs = accessed
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                Ok(Value::Long(secs as i64))
            } else {
                Ok(Value::False)
            }
        }
        Err(_) => {
            vm.emit_warning(&format!("fileatime(): stat failed for {}", path_str));
            Ok(Value::False)
        }
    }
}

fn filectime_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let path_str = path.to_string_lossy();
    if path_str.is_empty() {
        return Ok(Value::False);
    }
    if !vm.check_open_basedir("filectime", &path_str) {
        return Ok(Value::False);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match std::fs::metadata(&*path_str) {
            Ok(meta) => Ok(Value::Long(meta.ctime())),
            Err(_) => {
                vm.emit_warning(&format!("filectime(): stat failed for {}", path_str));
                Ok(Value::False)
            }
        }
    }
    #[cfg(not(unix))]
    {
        // Fall back to created on non-Unix
        match std::fs::metadata(&*path_str) {
            Ok(meta) => {
                if let Ok(c) = meta.created() {
                    let secs = c
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    Ok(Value::Long(secs as i64))
                } else {
                    Ok(Value::False)
                }
            }
            Err(_) => {
                vm.emit_warning(&format!("filectime(): stat failed for {}", path_str));
                Ok(Value::False)
            }
        }
    }
}

fn fileinode_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let path_str = path.to_string_lossy();
    if path_str.is_empty() {
        return Ok(Value::False);
    }
    if path.as_bytes().contains(&0) {
        let line = vm.current_line;
        vm.emit_warning_at("fileinode(): Filename contains null byte", line);
        return Ok(Value::False);
    }
    if !vm.check_open_basedir("fileinode", &path_str) {
        return Ok(Value::False);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match std::fs::metadata(&*path_str) {
            Ok(meta) => Ok(Value::Long(meta.ino() as i64)),
            Err(_) => {
                vm.emit_warning(&format!("fileinode(): stat failed for {}", path_str));
                Ok(Value::False)
            }
        }
    }
    #[cfg(not(unix))]
    Ok(Value::False)
}

fn fileowner_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let path_str = path.to_string_lossy();
    if path_str.is_empty() {
        return Ok(Value::False);
    }
    if path.as_bytes().contains(&0) {
        let line = vm.current_line;
        vm.emit_warning_at("fileowner(): Filename contains null byte", line);
        return Ok(Value::False);
    }
    if !vm.check_open_basedir("fileowner", &path_str) {
        return Ok(Value::False);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match std::fs::metadata(&*path_str) {
            Ok(meta) => Ok(Value::Long(meta.uid() as i64)),
            Err(_) => {
                vm.emit_warning(&format!("fileowner(): stat failed for {}", path_str));
                Ok(Value::False)
            }
        }
    }
    #[cfg(not(unix))]
    Ok(Value::Long(0))
}

fn filegroup_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let path_str = path.to_string_lossy();
    if path_str.is_empty() {
        return Ok(Value::False);
    }
    if path.as_bytes().contains(&0) {
        let line = vm.current_line;
        vm.emit_warning_at("filegroup(): Filename contains null byte", line);
        return Ok(Value::False);
    }
    if !vm.check_open_basedir("filegroup", &path_str) {
        return Ok(Value::False);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match std::fs::metadata(&*path_str) {
            Ok(meta) => Ok(Value::Long(meta.gid() as i64)),
            Err(_) => {
                vm.emit_warning(&format!("filegroup(): stat failed for {}", path_str));
                Ok(Value::False)
            }
        }
    }
    #[cfg(not(unix))]
    Ok(Value::Long(0))
}

/// php_do_chown() / php_do_chgrp() on a plain file (user wrappers are
/// dispatched before reaching here).
fn chown_common(
    vm: &mut Vm,
    args: &[Value],
    name: &str,
    group: bool,
    link: bool,
) -> Result<Value, VmError> {
    let filename = args.first().unwrap_or(&Value::Null).to_php_string();
    let pname = if group { "group" } else { "user" };
    if let Some(e) = path_arg_nul_error(vm, filename.as_bytes(), name, 1, "filename") {
        return Err(e);
    }
    let who = args.get(1).map(|v| v.deref()).unwrap_or(Value::Null);
    let id: u32 = match &who {
        Value::Long(n) => *n as u32,
        Value::String(s) => {
            let mut buf = s.as_bytes().to_vec();
            buf.push(0);
            let found = unsafe {
                if group {
                    let g = libc::getgrnam(buf.as_ptr() as *const libc::c_char);
                    (!g.is_null()).then(|| (*g).gr_gid)
                } else {
                    let p = libc::getpwnam(buf.as_ptr() as *const libc::c_char);
                    (!p.is_null()).then(|| (*p).pw_uid)
                }
            };
            match found {
                Some(id) => id,
                None => {
                    vm.emit_warning(&format!(
                        "{}(): Unable to find {} for {}",
                        name,
                        if group { "gid" } else { "uid" },
                        s.to_msg_string()
                    ));
                    return Ok(Value::False);
                }
            }
        }
        Value::Double(d) if d.is_finite() && d.fract() == 0.0 => *d as i64 as u32,
        Value::True | Value::False => who.to_long() as u32,
        other => {
            let msg = format!(
                "{}(): Argument #2 (${}) must be of type string|int, {} given",
                name,
                pname,
                Vm::value_type_name(other)
            );
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
    };
    let mut path = filename.as_bytes().to_vec();
    if path.len() >= 7 && path[..7].eq_ignore_ascii_case(b"file://") {
        path.drain(..7);
    }
    path.push(0);
    let (uid, gid) = if group {
        (u32::MAX, id)
    } else {
        (id, u32::MAX)
    };
    let ret = unsafe {
        let p = path.as_ptr() as *const libc::c_char;
        if link {
            libc::lchown(p, uid, gid)
        } else {
            libc::chown(p, uid, gid)
        }
    };
    if ret == -1 {
        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        vm.emit_warning(&format!("{}(): {}", name, xp_socket::strerror(errno)));
        return Ok(Value::False);
    }
    Ok(Value::True)
}

fn chown_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    chown_common(vm, args, "chown", false, false)
}

fn chgrp_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    chown_common(vm, args, "chgrp", true, false)
}

fn lchown_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    chown_common(vm, args, "lchown", false, true)
}

fn lchgrp_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    chown_common(vm, args, "lchgrp", true, true)
}

fn fpassthru_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let r = fpassthru_fn_inner(vm, args);
    xp_socket::flush_crypto_warnings(vm, "fpassthru");
    r
}

fn fpassthru_fn_inner(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    require_resource_arg(
        vm,
        args.first().unwrap_or(&Value::Null),
        &called_as(vm, "fpassthru"),
    )?;
    let fid = args.first().unwrap_or(&Value::Null).to_long();
    // A write-only plain file: read() fails with EBADF (php_stdiop_read()).
    let write_only = FILE_HANDLES.with(|h| {
        h.borrow().get(&fid).is_some_and(|fh| {
            fh.special == SpecialStream::None
                && fh.sock.is_none()
                && fh.user_wrapper.is_none()
                && !fh.mode.contains('+')
                && matches!(fh.mode.chars().next(), Some('w' | 'a' | 'x' | 'c'))
        })
    });
    if write_only {
        let line = vm.current_line;
        vm.emit_notice_at(
            "fpassthru(): Read of 8192 bytes failed with errno=9 Bad file descriptor",
            line,
        );
        return Ok(Value::Long(0));
    }
    let has_read_filters =
        crate::stream_filters::chain_has_direction(fid, crate::stream_filters::STREAM_FILTER_READ);
    if has_read_filters {
        let mut total = 0i64;
        loop {
            let chunk = read_filtered(vm, fid, 8192);
            if chunk.is_empty() {
                break;
            }
            total += chunk.len() as i64;
            vm.write_output(&chunk);
        }
        return Ok(Value::Long(total));
    }
    let buf = FILE_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(fh) = handles.get_mut(&fid) {
            let mut buf = Vec::new();
            let mut chunk = vec![0u8; STREAM_CHUNK_SIZE];
            loop {
                match fh.read_raw(&mut chunk, false) {
                    Ok(0) => {
                        fh.eof = true;
                        break;
                    }
                    Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    Err(_) if !buf.is_empty() || fh.sock.is_some() => break,
                    Err(_) => return None,
                }
            }
            Some(buf)
        } else {
            None
        }
    });
    match buf {
        Some(b) => {
            let n = b.len() as i64;
            vm.write_output(&b);
            Ok(Value::Long(n))
        }
        None => Ok(Value::False),
    }
}

fn linkinfo_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let path_s = path.to_string_lossy();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match std::fs::symlink_metadata(&*path_s) {
            Ok(meta) => Ok(Value::Long(meta.dev() as i64)),
            Err(e) => {
                let msg = strip_os_error(&e.to_string());
                vm.emit_warning(&format!("linkinfo(): {}", msg));
                Ok(Value::Long(-1))
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path_s;
        vm.emit_warning("linkinfo(): Not supported on this platform");
        Ok(Value::Long(-1))
    }
}

fn parse_ini_file_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let path_str = path.to_string_lossy();
    if path.as_bytes().is_empty() {
        return Err(throw_value_error_in(
            vm,
            "parse_ini_file",
            args,
            "parse_ini_file(): Argument #1 ($filename) must not be empty",
        ));
    }
    let process_sections = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
    let scanner_mode = args.get(2).map(|v| v.to_long()).unwrap_or(0);

    // Search include_path for relative filenames.
    let read_result = std::fs::read_to_string(&*path_str).or_else(|e| {
        // Only fall back to include_path for non-absolute, non-current-dir paths.
        if path_str.starts_with('/') || path_str.starts_with("./") || path_str.starts_with("../") {
            return Err(e);
        }
        // include_path, then the running script's directory
        std::fs::read_to_string(vm.resolve_include_path(&path_str)).map_err(|_| e)
    });

    match read_result {
        Ok(content) => Ok(crate::ini_parser::parse_ini(
            vm,
            content.as_bytes(),
            process_sections,
            scanner_mode,
            &path_str,
        )),
        Err(_) => {
            vm.emit_warning(&format!(
                "parse_ini_file({}): Failed to open stream: No such file or directory",
                path_str
            ));
            Ok(Value::False)
        }
    }
}

fn error_get_last_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if !args.is_empty() {
        let msg = format!(
            "error_get_last() expects exactly 0 arguments, {} given",
            args.len()
        );
        let exc = vm.create_exception(b"ArgumentCountError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: format!("Uncaught ArgumentCountError: {}", msg),
            line: vm.current_line,
        });
    }
    match vm.last_error.clone() {
        None => Ok(Value::Null),
        Some((ty, msg, file, line)) => {
            let mut arr = PhpArray::new();
            arr.set(
                ArrayKey::String(PhpString::from_bytes(b"type")),
                Value::Long(ty),
            );
            arr.set(
                ArrayKey::String(PhpString::from_bytes(b"message")),
                Value::String(PhpString::from_vec(
                    goro_core::errdisplay::raw_error_bytes(&msg).into_owned(),
                )),
            );
            arr.set(
                ArrayKey::String(PhpString::from_bytes(b"file")),
                Value::String(PhpString::from_string(file)),
            );
            arr.set(
                ArrayKey::String(PhpString::from_bytes(b"line")),
                Value::Long(line as i64),
            );
            if let Some(t @ Value::Array(_)) = vm.last_error_trace.clone() {
                arr.set(ArrayKey::String(PhpString::from_bytes(b"trace")), t);
            }
            Ok(Value::Array(Rc::new(RefCell::new(arr))))
        }
    }
}

fn error_clear_last_fn(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    vm.last_error = None;
    vm.last_error_trace = None;
    Ok(Value::Null)
}

fn phpversion_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(ext) = args.first() {
        if !matches!(ext, Value::Null | Value::Undef) {
            let ext_name = ext.to_php_string().to_string_lossy().to_ascii_lowercase();
            match ext_name.as_str() {
                "standard" | "core" | "date" | "pcre" | "json" | "ctype" | "hash" | "spl"
                | "tidy" | "ldap" => {
                    return Ok(Value::String(PhpString::from_bytes(b"8.5.4")));
                }
                // PECL extensions report their own version.
                "uuid" if _vm.loaded_extensions.contains(b"uuid".as_slice()) => {
                    return Ok(Value::String(PhpString::from_bytes(b"1.3.0")));
                }
                "mailparse" if _vm.loaded_extensions.contains(b"mailparse".as_slice()) => {
                    return Ok(Value::String(PhpString::from_bytes(b"3.2.1-dev")));
                }
                "imagick" if _vm.loaded_extensions.contains(b"imagick".as_slice()) => {
                    return Ok(Value::String(PhpString::from_bytes(b"3.8.1")));
                }
                _ => return Ok(Value::False),
            }
        }
    }
    Ok(Value::String(PhpString::from_bytes(b"8.5.4")))
}

fn php_uname_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let mode = args
        .first()
        .map(|v| v.to_php_string())
        .unwrap_or_else(|| PhpString::from_bytes(b"a"));
    if mode.len() != 1 {
        return Err(throw_value_error_msg(
            _vm,
            "php_uname(): Argument #1 ($mode) must be a single character",
        ));
    }
    let mode_char = mode.as_bytes()[0];
    if !matches!(mode_char, b'a' | b'm' | b'n' | b'r' | b's' | b'v') {
        return Err(throw_value_error_msg(
            _vm,
            "php_uname(): Argument #1 ($mode) must be one of \"a\", \"m\", \"n\", \"r\", \"s\", or \"v\"",
        ));
    }
    let result = match mode_char {
        b's' => "Linux".to_string(),
        b'n' => "localhost".to_string(),
        b'r' => "6.0.0".to_string(),
        b'v' => "#1".to_string(),
        b'm' => "x86_64".to_string(),
        _ => "Linux localhost 6.0.0 #1 x86_64".to_string(),
    };
    Ok(Value::String(PhpString::from_string(result)))
}

fn php_sapi_name_fn(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(match vm.constants.get(b"PHP_SAPI".as_slice()) {
        Some(Value::String(s)) => Value::String(s.clone()),
        _ => Value::String(PhpString::from_bytes(b"cli")),
    })
}

thread_local! {
    /// Startup configuration directives (php.ini / -d / phpt --INI--).
    static CFG_VARS: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
    /// The php.ini file that was loaded (php_ini_opened_path).
    static INI_OPENED_PATH: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Record the php.ini file the SAPI loaded (php_ini_loaded_file(),
/// get_cfg_var("cfg_file_path")).
pub fn set_ini_opened_path(path: Option<String>) {
    INI_OPENED_PATH.with(|p| *p.borrow_mut() = path);
}

/// The INI entries in effect at startup: every directive from php.ini, the
/// SAPI's built-in settings and `-d` (or a phpt --INI-- section), in the
/// order they apply, goes through the same handling (php_init_config() then
/// zend_register_ini_entries() reading the configuration hash): the
/// directive's on-modify handler (validators, the values the engine caches)
/// and its value for ini_get(). Returns the "PHP Startup" messages to show
/// before the request's output.
pub fn apply_startup_ini(vm: &mut Vm, ini: &[(String, String)]) -> Vec<u8> {
    let mut startup_deprecations: Vec<String> = Vec::new();
    for (key, value) in ini {
        register_cfg_var(key, value);
        match key.as_str() {
            "precision" => {
                if let Ok(p) = value.trim().parse::<i32>() {
                    goro_core::value::set_php_precision(p);
                }
            }
            "serialize_precision" => {
                if let Ok(p) = value.trim().parse::<i32>() {
                    goro_core::value::set_php_serialize_precision(p);
                }
            }
            "memory_limit" => {
                let (v, _) = goro_core::ini_quantity::parse_quantity(value.as_bytes(), false);
                goro_core::value::set_memory_limit(if v == u64::MAX { -1 } else { v as i64 });
            }
            "disable_functions" => {
                for func_name in value.split(',') {
                    let func_name = func_name.trim();
                    if !func_name.is_empty() {
                        let lower: Vec<u8> =
                            func_name.bytes().map(|b| b.to_ascii_lowercase()).collect();
                        // exit()/die() are language constructs that cannot be disabled.
                        if lower == b"exit" || lower == b"die" {
                            if vm.error_reporting & 2 != 0 {
                                vm.write_output(
                                    format!(
                                        "\nWarning: Cannot disable function {}() in Unknown on line 0\n",
                                        func_name
                                    )
                                    .as_bytes(),
                                );
                            }
                            continue;
                        }
                        if let Some(f) = vm.functions.remove(&lower) {
                            vm.disabled_function_impls.insert(lower.clone(), f);
                        }
                        vm.disabled_functions.insert(lower);
                    }
                }
                continue;
            }
            "hard_timeout" => {
                if let Ok(n) = value.trim().parse::<i64>() {
                    goro_core::value::HARD_TIMEOUT.store(n, std::sync::atomic::Ordering::Relaxed);
                }
            }
            "max_execution_time" => {
                // php_request_startup(): arm the max_execution_time timer.
                if let Ok(n) = value.trim().parse::<i64>() {
                    set_execution_timeout(n);
                }
            }
            // OnUpdateStringUnempty / OnUpdateStrNotEmpty: an empty value
            // fails, the default stays.
            "variables_order"
            | "arg_separator.input"
            | "arg_separator.output"
            | "include_path"
            | "extension_dir"
                if value.is_empty() =>
            {
                continue;
            }
            _ => {}
        }
        let val = match value.parse::<i64>() {
            Ok(n) => Value::Long(n),
            Err(_) => Value::String(PhpString::from_string(value.clone())),
        };
        if key == "error_reporting" {
            vm.error_reporting = val.to_long();
        }
        if let Some(msg) = ini_handler_deprecation(key.as_bytes(), &val) {
            startup_deprecations.push(msg);
        }
        if let Some(msg) = assert_ini_deprecation(key.as_bytes(), &val) {
            startup_deprecations.push(format!("PHP Startup: {}", msg));
        }
        if let Some(validate) = vm.ini_validators.get(key.as_bytes()).copied() {
            if !validate(vm, value.as_bytes(), true) {
                continue;
            }
        }
        vm.constants.insert(key.as_bytes().to_vec(), val);
    }
    // php_error_cb() output, with error_prepend_string / error_append_string
    let ini_str = |vm: &Vm, k: &[u8]| {
        vm.constants
            .get(k)
            .map(|v| v.to_php_string().as_bytes().to_vec())
            .unwrap_or_default()
    };
    let (prepend, append) = (
        ini_str(vm, b"error_prepend_string"),
        ini_str(vm, b"error_append_string"),
    );
    let mut startup_messages: Vec<u8> = Vec::new();
    for msg in startup_deprecations {
        startup_messages.extend_from_slice(&prepend);
        startup_messages
            .extend_from_slice(format!("\nDeprecated: {} in Unknown on line 0\n", msg).as_bytes());
        startup_messages.extend_from_slice(&append);
    }
    // php_ini_register_extensions(): goro has no shared extensions, so an
    // `extension=` naming a module that is not built in cannot be loaded.
    for (key, value) in ini {
        if key == "extension" {
            startup_load_extension(vm, value);
        }
    }
    // OnChangeMemoryLimit at startup: memory_limit above max_memory_limit is
    // clamped with a startup warning.
    if let (Some(ml), Some(mx)) = (
        vm.constants
            .get(b"memory_limit".as_ref())
            .map(|v| v.to_php_string()),
        vm.constants
            .get(b"max_memory_limit".as_ref())
            .map(|v| v.to_php_string()),
    ) {
        let (v, _) = goro_core::ini_quantity::parse_quantity(ml.as_bytes(), false);
        let (max, _) = goro_core::ini_quantity::parse_quantity(mx.as_bytes(), false);
        if v > max {
            if v != u64::MAX {
                vm.write_output(format!(
                    "\nWarning: Failed to set memory_limit to {} bytes. Setting to max_memory_limit instead (currently: {} bytes) in Unknown on line 0\n",
                    v as i64, max as i64
                ).as_bytes());
            }
            goro_core::value::set_memory_limit(max as i64);
            vm.constants
                .insert(b"memory_limit".to_vec(), Value::String(mx));
        }
    }
    startup_messages
}

/// OnSetExceptionStringParamMaxLen: 0..=1000000.
/// php_load_extension() with MODULE_PERSISTENT, for an `extension=` INI
/// entry: built-in modules load silently, anything else fails with
/// E_CORE_WARNING.
fn startup_load_extension(vm: &mut Vm, filename: &str) {
    let base = filename.rsplit('/').next().unwrap_or(filename);
    let base = base.strip_suffix(".so").unwrap_or(base);
    let base = base.strip_prefix("php_").unwrap_or(base);
    let base = base.strip_suffix(".dll").unwrap_or(base);
    if vm
        .loaded_extensions
        .contains(base.to_ascii_lowercase().as_bytes())
    {
        return;
    }
    // A module goro has built in but loads on request.
    let lower = base.to_ascii_lowercase();
    if let Some(&(_, minit)) = vm
        .loadable_modules
        .iter()
        .find(|(m, _)| *m == lower.as_bytes())
    {
        minit(vm);
        return;
    }
    let ini_s = |vm: &Vm, k: &[u8]| {
        vm.constants
            .get(k)
            .map(|v| v.to_php_string().to_string_lossy())
            .unwrap_or_default()
    };
    let (libpath, libpath2) = if filename.contains('/') {
        (filename.to_string(), None)
    } else {
        let dir = ini_s(vm, b"extension_dir");
        if dir.is_empty() {
            return;
        }
        let sep = if dir.ends_with('/') { "" } else { "/" };
        (
            format!("{}{}{}", dir, sep, filename),
            Some(format!("{}{}{}.so", dir, sep, filename)),
        )
    };
    let err = |p: &str| {
        if std::path::Path::new(p).exists() {
            format!("{}: invalid ELF header", p)
        } else {
            format!(
                "{}: cannot open shared object file: No such file or directory",
                p
            )
        }
    };
    let msg = match &libpath2 {
        Some(p2) => format!(
            "PHP Startup: Unable to load dynamic library '{}' (tried: {} ({}), {} ({}))",
            filename,
            libpath,
            err(&libpath),
            p2,
            err(p2)
        ),
        // A full path is tried as given only.
        None => format!(
            "PHP Startup: Unable to load dynamic library '{}' (tried: {} ({}))",
            filename,
            libpath,
            err(&libpath)
        ),
    };
    let ini_on = |vm: &Vm, k: &[u8], default: bool| {
        vm.constants
            .get(k)
            .map(|v| ini_parse_bool(&Value::String(v.to_php_string())))
            .unwrap_or(default)
    };
    // php_error_cb(): E_CORE_WARNING, shown when display_startup_errors is on
    if vm.error_reporting & 32 == 0
        || !ini_on(vm, b"display_errors", true)
        || !ini_on(vm, b"display_startup_errors", true)
    {
        return;
    }
    let html = ini_on(vm, b"html_errors", false);
    let out = if html {
        format!(
            "<br />\n<b>Warning</b>:  {} in <b>Unknown</b> on line <b>0</b><br />\n",
            msg
        )
    } else {
        format!("\nWarning: {} in Unknown on line 0\n", msg)
    };
    vm.write_output(out.as_bytes());
}

/// Record a startup configuration directive for get_cfg_var().
pub fn register_cfg_var(key: &str, value: &str) {
    CFG_VARS.with(|c| c.borrow_mut().push((key.to_string(), value.to_string())));
}

fn get_cfg_var_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let name = args
        .first()
        .unwrap_or(&Value::Null)
        .to_php_string()
        .to_string_lossy();
    if name == "cfg_file_path" {
        let path = INI_OPENED_PATH
            .with(|p| p.borrow().clone())
            .unwrap_or_default();
        return Ok(Value::String(PhpString::from_string(path)));
    }
    CFG_VARS.with(|c| {
        let vars = c.borrow();
        let mut found: Option<Value> = None;
        let mut arr: Option<PhpArray> = None;
        for (k, v) in vars.iter() {
            if *k == name {
                found = Some(Value::String(PhpString::from_string(v.clone())));
            } else if let Some(rest) = k.strip_prefix(name.as_str()) {
                // name[offset] = value entries form an array
                if let Some(off) = rest.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
                    let a = arr.get_or_insert_with(PhpArray::new);
                    let val = Value::String(PhpString::from_string(v.clone()));
                    if off.is_empty() {
                        a.push(val);
                    } else {
                        a.set(
                            Vm::value_to_array_key(Value::String(PhpString::from_string(
                                off.trim().to_string(),
                            ))),
                            val,
                        );
                    }
                }
            }
        }
        Ok(match (arr, found) {
            (Some(a), _) => Value::Array(Rc::new(RefCell::new(a))),
            (None, Some(v)) => v,
            (None, None) => Value::False,
        })
    })
}

fn php_ini_loaded_file_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(INI_OPENED_PATH
        .with(|p| p.borrow().clone())
        .map_or(Value::False, |p| Value::String(PhpString::from_string(p))))
}

thread_local! {
    /// php_ini_scanned_files: the additional .ini files parsed at startup.
    static INI_SCANNED_FILES: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
}

/// Record the .ini files the SAPI scanned (php_ini_scanned_files()).
pub fn set_ini_scanned_files(list: Option<Vec<u8>>) {
    INI_SCANNED_FILES.with(|p| *p.borrow_mut() = list);
}

fn php_ini_scanned_files_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(INI_SCANNED_FILES
        .with(|p| p.borrow().clone())
        .map_or(Value::False, |p| Value::String(PhpString::from_vec(p))))
}

/// ftok(): a System V IPC key from a path and a project identifier.
fn ftok_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args
        .first()
        .map(|v| v.to_php_string())
        .unwrap_or_else(PhpString::empty);
    let proj = args
        .get(1)
        .map(|v| v.to_php_string())
        .unwrap_or_else(PhpString::empty);
    if path.as_bytes().contains(&0) {
        return Err(throw_value_error_in(
            vm,
            "ftok",
            args,
            "ftok(): Argument #1 ($filename) must not contain any null bytes",
        ));
    }
    if path.is_empty() {
        return Err(throw_value_error_in(
            vm,
            "ftok",
            args,
            "ftok(): Argument #1 ($filename) must not be empty",
        ));
    }
    if proj.len() != 1 {
        return Err(throw_value_error_in(
            vm,
            "ftok",
            args,
            "ftok(): Argument #2 ($project_id) must be a single character",
        ));
    }
    let p = path.to_string_lossy();
    if !vm.check_open_basedir("ftok", &p) {
        return Ok(Value::Long(-1));
    }
    let c = std::ffi::CString::new(path.as_bytes().to_vec()).unwrap_or_default();
    let k = unsafe { libc::ftok(c.as_ptr(), proj.as_bytes()[0] as libc::c_int) };
    if k == -1 {
        let e = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        let msg = unsafe { std::ffi::CStr::from_ptr(libc::strerror(e)) }.to_string_lossy();
        vm.emit_warning(&format!("ftok(): ftok() failed - {}", msg));
    }
    Ok(Value::Long(k as i64))
}

fn getmypid_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(std::process::id() as i64))
}

fn getmyuid_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(1000))
}

fn getmygid_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(1000))
}

thread_local! {
    static TRANSLATED_PATH: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// SG(request_info).path_translated, set by the SAPI (the resolved path of
/// the main script).
pub fn set_translated_path(p: Option<String>) {
    TRANSLATED_PATH.with(|t| *t.borrow_mut() = p);
}

/// php_statpage(): stat() of the main script (SG(request_info).path_translated).
fn statpage(vm: &Vm) -> Option<std::fs::Metadata> {
    let mut path = TRANSLATED_PATH.with(|p| p.borrow().clone());
    if let (None, Some(Value::Array(s))) = (&path, vm.get_global(b"_SERVER")) {
        for k in [&b"PATH_TRANSLATED"[..], b"SCRIPT_FILENAME"] {
            if let Some(Value::String(p)) =
                s.borrow().get(&ArrayKey::String(PhpString::from_bytes(k)))
            {
                if !p.as_bytes().is_empty() {
                    path = Some(p.to_string_lossy());
                    break;
                }
            }
        }
    }
    let path = path.unwrap_or_else(|| vm.current_file.to_string());
    std::fs::metadata(path).ok()
}

fn getlastmod_fn(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    use std::os::unix::fs::MetadataExt;
    Ok(match statpage(vm) {
        Some(m) => Value::Long(m.mtime()),
        None => Value::False,
    })
}

fn getmyinode_fn(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    use std::os::unix::fs::MetadataExt;
    Ok(match statpage(vm) {
        Some(m) => Value::Long(m.ino() as i64),
        None => Value::False,
    })
}

fn get_current_user_fn(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    use std::os::unix::fs::MetadataExt;
    let uid = statpage(vm)
        .map(|m| m.uid())
        .unwrap_or_else(|| unsafe { libc::getuid() });
    let name = unsafe {
        let pw = libc::getpwuid(uid);
        if pw.is_null() {
            String::new()
        } else {
            std::ffi::CStr::from_ptr((*pw).pw_name)
                .to_string_lossy()
                .into_owned()
        }
    };
    Ok(Value::String(PhpString::from_string(name)))
}

fn defined_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let name = args.first().unwrap_or(&Value::Null).to_php_string();
    let name_bytes = name.as_bytes();
    let name_str = name.to_string_lossy();
    // Check for class constants
    if let Some(pos) = name_str.find("::") {
        let class_name = &name_str[..pos];
        let const_name = &name_str[pos + 2..];
        // Strip leading backslash from class name
        let class_name_stripped = class_name.strip_prefix('\\').unwrap_or(class_name);
        let mut class_lower: Vec<u8> = class_name_stripped
            .as_bytes()
            .iter()
            .map(|b| b.to_ascii_lowercase())
            .collect();
        // zend_get_class_constant_ex(): self/parent/static of the caller
        let scope = vm.current_class_scope().map(|s| s.to_ascii_lowercase());
        match class_lower.as_slice() {
            b"self" => match scope {
                Some(s) => class_lower = s,
                None => return Ok(Value::False),
            },
            b"parent" => {
                match scope.and_then(|s| vm.classes.get(&s).and_then(|c| c.parent.clone())) {
                    Some(p) => class_lower = p.to_ascii_lowercase(),
                    None => return Ok(Value::False),
                }
            }
            b"static" => match vm.called_class_stack.last() {
                Some(c) => class_lower = c.to_ascii_lowercase(),
                None => return Ok(Value::False),
            },
            _ => {}
        }
        if let Some(class) = vm.classes.get(&class_lower) {
            // Trait constants cannot be accessed directly.
            if class.is_trait {
                return Ok(Value::False);
            }
            if class.constants.contains_key(const_name.as_bytes()) {
                // Constants not visible from the calling scope are "not defined"
                if let Some(meta) = class.constants_meta.get(const_name.as_bytes()) {
                    use goro_core::object::Visibility;
                    if meta.visibility != Visibility::Public {
                        let decl = meta.declaring_class.to_ascii_lowercase();
                        let scope = vm.current_class_scope().map(|s| s.to_ascii_lowercase());
                        let ok = match meta.visibility {
                            Visibility::Private => scope.as_deref() == Some(decl.as_slice()),
                            _ => scope
                                .map(|sc| {
                                    sc == decl
                                        || vm.class_extends(&sc, &decl)
                                        || vm.class_extends(&decl, &sc)
                                })
                                .unwrap_or(false),
                        };
                        if !ok {
                            return Ok(Value::False);
                        }
                    }
                }
                return Ok(Value::True);
            }
        } else if vm.internal_class(&class_lower).is_some()
            && vm
                .get_builtin_class_constant(&class_lower, const_name.as_bytes())
                .is_some()
        {
            // a constant of a natively implemented internal class
            return Ok(Value::True);
        }
        return Ok(Value::False);
    }
    if is_ini_entry_name(name_bytes) {
        return Ok(Value::False);
    }
    if vm.constants.contains_key(name_bytes) {
        Ok(Value::True)
    } else {
        // Also check with stripped leading backslash for FQN
        let check_bytes = if name_bytes.starts_with(b"\\") {
            &name_bytes[1..]
        } else {
            name_bytes
        };
        if vm.constants.contains_key(check_bytes) {
            return Ok(Value::True);
        }
        // Normalize namespace prefix for case-insensitive lookup
        let normalized = normalize_ns_const_name(check_bytes);
        if vm.constants.contains_key(&normalized) {
            return Ok(Value::True);
        }
        Ok(Value::False)
    }
}

fn zend_version_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(PhpString::from_bytes(b"4.5.4")))
}

fn extension_loaded_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let ext = args
        .first()
        .unwrap_or(&Value::Null)
        .to_php_string()
        .to_string_lossy()
        .to_ascii_lowercase();
    let loaded = matches!(
        ext.as_str(),
        "standard"
            | "core"
            | "date"
            | "pcre"
            | "json"
            | "ctype"
            | "hash"
            | "spl"
            | "random"
            | "reflection"
            | "uri"
            | "lexbor"
    ) || vm.loaded_extensions.contains(ext.as_bytes());
    Ok(if loaded { Value::True } else { Value::False })
}

/// Module names as PHP spells them, in a php-src-like startup order
/// (static modules first). Extensions goro registers but this list does not
/// name are appended in sorted order.
const MODULE_ORDER: &[&str] = &[
    "Core",
    "date",
    "libxml",
    "openssl",
    "pcre",
    "sqlite3",
    "zlib",
    "bcmath",
    "bz2",
    "calendar",
    "ctype",
    "curl",
    "dba",
    "dom",
    "hash",
    "fileinfo",
    "filter",
    "ftp",
    "gd",
    "gettext",
    "gmp",
    "json",
    "iconv",
    "SPL",
    "ldap",
    "intl",
    "session",
    "lexbor",
    "mbstring",
    "standard",
    "mysqlnd",
    "Zend OPcache",
    "pcntl",
    "pgsql",
    "exif",
    "mysqli",
    "PDO",
    "pdo_mysql",
    "pdo_pgsql",
    "pdo_sqlite",
    "Phar",
    "posix",
    "random",
    "readline",
    "Reflection",
    "shmop",
    "SimpleXML",
    "soap",
    "sockets",
    "sodium",
    "sysvmsg",
    "sysvsem",
    "sysvshm",
    "tidy",
    "tokenizer",
    "uri",
    "xml",
    "xmlreader",
    "xmlwriter",
    "xsl",
    "zip",
    "zend_test",
];

fn get_loaded_extensions_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let mut result = PhpArray::new();
    // zend_extensions: true lists the Zend extensions (OPcache registers
    // itself as one besides its module).
    if args.first().is_some_and(|v| v.is_truthy()) {
        if vm.loaded_extensions.contains(b"zend opcache".as_slice()) {
            result.push(Value::String(PhpString::from_bytes(b"Zend OPcache")));
        }
        return Ok(Value::Array(Rc::new(RefCell::new(result))));
    }
    let always = [
        "core",
        "standard",
        "date",
        "pcre",
        "json",
        "ctype",
        "hash",
        "spl",
        "random",
        "reflection",
        "uri",
        "lexbor",
    ];
    let is_loaded = |lc: &str| always.contains(&lc) || vm.loaded_extensions.contains(lc.as_bytes());
    let mut seen = std::collections::HashSet::new();
    for name in MODULE_ORDER {
        let lc = name.to_ascii_lowercase();
        if is_loaded(&lc) {
            seen.insert(lc);
            result.push(Value::String(PhpString::from_bytes(name.as_bytes())));
        }
    }
    let mut rest: Vec<&Vec<u8>> = vm
        .loaded_extensions
        .iter()
        .filter(|e| !seen.contains(&String::from_utf8_lossy(e).into_owned()))
        .collect();
    rest.sort();
    for e in rest {
        result.push(Value::String(PhpString::from_bytes(e)));
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn get_extension_funcs_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let name = args.first().unwrap_or(&Value::Null).to_php_string();
    let name = String::from_utf8_lossy(name.as_bytes()).to_ascii_lowercase();
    // "zend" names the Core module.
    let name = if name == "zend" {
        "core".to_string()
    } else {
        name
    };
    let mut arr = PhpArray::new();
    for f in goro_core::stub_sigs::FUNCTIONS {
        if f.ext.eq_ignore_ascii_case(&name)
            && vm
                .functions
                .contains_key(f.name.to_ascii_lowercase().as_bytes())
        {
            arr.push(Value::String(PhpString::from_bytes(f.name.as_bytes())));
        }
    }
    if arr.is_empty() {
        return Ok(Value::False);
    }
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

// class_implements, class_parents, class_uses moved to goro-ext-spl

fn str_increment_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        let msg = "str_increment(): Argument #1 ($string) must not be empty";
        let exc = vm.create_exception(b"ValueError", msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg.to_string(),
            line: vm.current_line,
        });
    }
    // Validate all characters are alphanumeric first
    if !bytes.iter().all(|b| b.is_ascii_alphanumeric()) {
        let msg = "str_increment(): Argument #1 ($string) must be composed only of alphanumeric ASCII characters";
        let exc = vm.create_exception(b"ValueError", msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg.to_string(),
            line: vm.current_line,
        });
    }
    // PHP string increment: like spreadsheet columns
    let mut result: Vec<u8> = bytes.to_vec();
    let mut carry = true;
    for i in (0..result.len()).rev() {
        if !carry {
            break;
        }
        match result[i] {
            b'z' => {
                result[i] = b'a';
                carry = true;
            }
            b'Z' => {
                result[i] = b'A';
                carry = true;
            }
            b'9' => {
                result[i] = b'0';
                carry = true;
            }
            b'a'..=b'y' => {
                result[i] += 1;
                carry = false;
            }
            b'A'..=b'Y' => {
                result[i] += 1;
                carry = false;
            }
            b'0'..=b'8' => {
                result[i] += 1;
                carry = false;
            }
            _ => {
                let msg = "str_increment(): Argument #1 ($string) must be composed only of alphanumeric ASCII characters";
                let exc = vm.create_exception(b"ValueError", msg, vm.current_line);
                vm.current_exception = Some(exc);
                return Err(VmError {
                    message: msg.to_string(),
                    line: vm.current_line,
                });
            }
        }
    }
    if carry {
        // Need to prepend: if first char was digit, prepend '1', if letter, prepend 'a' or 'A'
        let prefix = match bytes[0] {
            b'0'..=b'9' => b'1',
            b'a'..=b'z' => b'a',
            b'A'..=b'Z' => b'A',
            _ => b'1',
        };
        let mut new_result = vec![prefix];
        new_result.extend_from_slice(&result);
        result = new_result;
    }
    Ok(Value::String(PhpString::from_vec(result)))
}

fn fgetc_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let r = fgetc_fn_inner(vm, args);
    xp_socket::flush_crypto_warnings(vm, "fgetc");
    r
}

fn fgetc_fn_inner(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let arg = args.first().unwrap_or(&Value::Null);
    let fid = require_open_stream(vm, arg, &called_as(vm, "fgetc"))?;
    // PHP emits a notice if the underlying mode is write-only.
    let stream_mode = FILE_HANDLES.with(|h| h.borrow().get(&fid).map(|fh| fh.mode.clone()));
    if let Some(m) = stream_mode {
        let clean: String = m.chars().filter(|&c| c != 'b' && c != 't').collect();
        if matches!(clean.as_str(), "w" | "a" | "x" | "c") {
            let line = vm.current_line;
            vm.emit_notice_at(
                "fgetc(): Read of 8192 bytes failed with errno=9 Bad file descriptor",
                line,
            );
            return Ok(Value::False);
        }
    }
    let mut failed_errno = 0;
    let r = FILE_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(fh) = handles.get_mut(&fid) {
            let mut buf = [0u8; 1];
            match fh.read_raw(&mut buf, false) {
                Ok(0) => {
                    fh.eof = true;
                    Ok(Value::False)
                }
                Ok(_) => Ok(Value::String(PhpString::from_bytes(&buf))),
                Err(e) => {
                    if let Some(n) = e.raw_os_error().filter(|n| *n != 9 && *n != libc::EAGAIN) {
                        failed_errno = n;
                        fh.eof = true;
                    }
                    Ok(Value::False)
                }
            }
        } else {
            Ok(Value::False)
        }
    });
    if failed_errno != 0 {
        read_failed_notice(vm, "fgetc", failed_errno);
    }
    r
}

/// php_stdiop_read()'s notice for a failed read() (8192: the buffer size
/// PHP reads with).
fn read_failed_notice(vm: &mut Vm, fname: &str, errno: i32) {
    let msg = std::io::Error::from_raw_os_error(errno).to_string();
    let msg = msg
        .split(" (os error")
        .next()
        .unwrap_or("Unknown error")
        .to_string();
    let line = vm.current_line;
    vm.emit_notice_at(
        &format!(
            "{}(): Read of 8192 bytes failed with errno={} {}",
            fname, errno, msg
        ),
        line,
    );
}

fn flock_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Validate $operation arg before $stream arg (PHP order).
    let op_arg = args.get(1).cloned().unwrap_or(Value::Long(0));
    // Type-check operation arg: int or bool only; array/string -> TypeError.
    let op = match &op_arg {
        Value::Long(n) => *n,
        Value::Double(d) => *d as i64,
        Value::True => 1,
        Value::False => 0,
        Value::Null => 0,
        Value::Array(_) => {
            let msg =
                "flock(): Argument #2 ($operation) must be of type int, array given".to_string();
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
        Value::String(_) => {
            let msg =
                "flock(): Argument #2 ($operation) must be of type int, string given".to_string();
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
        _ => op_arg.to_long(),
    };
    let basic = op & 3;
    if basic == 0 {
        let msg = "flock(): Argument #2 ($operation) must be one of LOCK_SH, LOCK_EX, or LOCK_UN"
            .to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    let arg = args.first().cloned().unwrap_or(Value::Null);
    // Allow dir-handle resources (Linux PHP flock on a dir returns false but
    // does not throw).
    let dir_fid = if let Value::Resource(rid) = &arg {
        let is_dir = DIR_HANDLES.with(|dh| dh.borrow().contains_key(&rid.id()));
        if is_dir { Some(rid.id()) } else { None }
    } else {
        None
    };
    if dir_fid.is_some() {
        return Ok(Value::False);
    }
    let fid = require_open_stream(vm, &arg, "flock")?;
    // The 3rd argument ($would_block) is a ref parameter. PHP sets it to 1 if
    // the lock would block (only meaningful with LOCK_NB), and to 0 otherwise.
    let mut would_block: i64 = 0;
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let fd_opt = FILE_HANDLES.with(|h| h.borrow().get(&fid).map(|fh| fh.file.as_raw_fd()));
        if let Some(fd) = fd_opt {
            // Translate PHP LOCK_* (1=SH, 2=EX, 3=UN, 4=NB) to libc flags.
            let mut native: i32 = 0;
            match basic {
                1 => native |= libc::LOCK_SH,
                2 => native |= libc::LOCK_EX,
                3 => native |= libc::LOCK_UN,
                _ => {
                    // Write back $would_block ref param before returning.
                    if let Some(w) = args.get(2) {
                        if let Value::Reference(r) = w {
                            *r.borrow_mut() = Value::Long(would_block);
                        }
                    }
                    return Ok(Value::False);
                }
            }
            if op & 4 != 0 {
                native |= libc::LOCK_NB;
            }
            let r = unsafe { libc::flock(fd, native) };
            let success = r == 0;
            if !success && (op & 4) != 0 {
                let errno = unsafe { *libc::__errno_location() };
                if errno == libc::EWOULDBLOCK {
                    would_block = 1;
                }
            }
            // Write back $would_block ref param.
            if let Some(w) = args.get(2) {
                if let Value::Reference(r) = w {
                    *r.borrow_mut() = Value::Long(would_block);
                }
            }
            return Ok(if success { Value::True } else { Value::False });
        }
    }
    // Non-unix fallback: write $would_block = 0 and return true.
    if let Some(w) = args.get(2) {
        if let Value::Reference(r) = w {
            *r.borrow_mut() = Value::Long(would_block);
        }
    }
    Ok(Value::True)
}

fn fstat_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let arg = args.first().cloned().unwrap_or(Value::Null);
    // Directory streams have no stat operation.
    if is_dir_handle(&arg) {
        return Ok(Value::False);
    }
    let fid = require_open_stream(vm, &arg, "fstat")?;
    FILE_HANDLES.with(|handles| {
        let handles = handles.borrow();
        if let Some(fh) = handles.get(&fid) {
            #[cfg(unix)]
            {
                use std::os::unix::io::AsRawFd;
                let fd = fh.file.as_raw_fd();
                let mut stat_buf: libc::stat = unsafe { std::mem::zeroed() };
                let ret = unsafe { libc::fstat(fd, &mut stat_buf) };
                if ret != 0 {
                    return Ok(Value::False);
                }
                let mut result = PhpArray::new();
                let fields: [(i64, &[u8], i64); 13] = [
                    (0, b"dev", stat_buf.st_dev as i64),
                    (1, b"ino", stat_buf.st_ino as i64),
                    (2, b"mode", stat_buf.st_mode as i64),
                    (3, b"nlink", stat_buf.st_nlink as i64),
                    (4, b"uid", stat_buf.st_uid as i64),
                    (5, b"gid", stat_buf.st_gid as i64),
                    (6, b"rdev", stat_buf.st_rdev as i64),
                    (7, b"size", stat_buf.st_size as i64),
                    (8, b"atime", stat_buf.st_atime as i64),
                    (9, b"mtime", stat_buf.st_mtime as i64),
                    (10, b"ctime", stat_buf.st_ctime as i64),
                    (11, b"blksize", stat_buf.st_blksize as i64),
                    (12, b"blocks", stat_buf.st_blocks as i64),
                ];
                // PHP stat returns all numeric keys first (0-12), then all string keys
                for (idx, _name, val) in &fields {
                    result.set(ArrayKey::Int(*idx), Value::Long(*val));
                }
                for (_idx, name, val) in &fields {
                    result.set(
                        ArrayKey::String(PhpString::from_bytes(name)),
                        Value::Long(*val),
                    );
                }
                Ok(Value::Array(Rc::new(RefCell::new(result))))
            }
            #[cfg(not(unix))]
            {
                match fh.file.metadata() {
                    Ok(meta) => {
                        let mut arr = PhpArray::new();
                        let len = meta.len() as i64;
                        let fields: [(i64, &[u8], i64); 13] = [
                            (0, b"dev", 0),
                            (1, b"ino", 0),
                            (2, b"mode", 0o100644),
                            (3, b"nlink", 1),
                            (4, b"uid", 0),
                            (5, b"gid", 0),
                            (6, b"rdev", 0),
                            (7, b"size", len),
                            (8, b"atime", 0),
                            (9, b"mtime", 0),
                            (10, b"ctime", 0),
                            (11, b"blksize", -1),
                            (12, b"blocks", -1),
                        ];
                        // PHP stat returns all numeric keys first (0-12), then all string keys
                        for (idx, _name, val) in &fields {
                            arr.set(ArrayKey::Int(*idx), Value::Long(*val));
                        }
                        for (_idx, name, val) in &fields {
                            arr.set(
                                ArrayKey::String(PhpString::from_bytes(name)),
                                Value::Long(*val),
                            );
                        }
                        Ok(Value::Array(Rc::new(RefCell::new(arr))))
                    }
                    Err(_) => Ok(Value::False),
                }
            }
        } else {
            Ok(Value::False)
        }
    })
}

pub(crate) fn stream_get_contents_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let r = stream_get_contents_fn_inner(vm, args);
    xp_socket::flush_crypto_warnings(vm, "stream_get_contents");
    r
}

fn stream_get_contents_fn_inner(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    require_resource_arg(
        vm,
        args.first().unwrap_or(&Value::Null),
        &called_as(vm, "stream_get_contents"),
    )?;
    let fid = args.first().unwrap_or(&Value::Null).to_long();
    let max_length = match args.get(1).map(|v| v.deref()) {
        None | Some(Value::Null) | Some(Value::Undef) => -1,
        Some(v) => v.to_long(),
    };
    let offset = args.get(2).map(|v| v.to_long()).unwrap_or(-1);
    if max_length < -1 {
        return Err(throw_value_error_msg(
            vm,
            "stream_get_contents(): Argument #2 ($length) must be greater than or equal to -1",
        ));
    }

    let has_read_filters =
        crate::stream_filters::chain_has_direction(fid, crate::stream_filters::STREAM_FILTER_READ);
    if has_read_filters {
        if offset >= 0 {
            FILE_HANDLES.with(|h| {
                if let Some(fh) = h.borrow_mut().get_mut(&fid) {
                    let _ = fh.file.seek(SeekFrom::Start(offset as u64));
                }
            });
        }
        let limit = if max_length < 0 {
            usize::MAX
        } else {
            max_length as usize
        };
        let mut out: Vec<u8> = Vec::new();
        while out.len() < limit {
            let want = (limit - out.len()).min(8192);
            let chunk = read_filtered(vm, fid, want);
            if chunk.is_empty() {
                break;
            }
            out.extend_from_slice(&chunk);
        }
        return Ok(Value::String(PhpString::from_vec(out)));
    }

    // desiredpos handling: SEEK_CUR forward, SEEK_SET backward.
    if offset >= 0 {
        let ok = FILE_HANDLES.with(|h| {
            let mut h = h.borrow_mut();
            let Some(fh) = h.get_mut(&fid) else {
                return true;
            };
            if !fh.seekable {
                // Non-seekable streams start at position 0.
                return offset == 0;
            }
            let buffered = fh.filter_read_ready.len() as u64;
            let ok = match fh.file.stream_position() {
                Ok(pos) if offset as u64 + buffered > pos => {
                    fh.filter_read_ready.clear();
                    fh.file.seek(SeekFrom::Current(offset - pos as i64)).is_ok()
                }
                Ok(pos) if (offset as u64) + buffered < pos => {
                    fh.filter_read_ready.clear();
                    fh.file.seek(SeekFrom::Start(offset as u64)).is_ok()
                }
                Ok(_) => return true,
                Err(_) => fh.file.seek(SeekFrom::Start(offset as u64)).is_ok(),
            };
            if ok {
                fh.eof = false;
            }
            ok
        });
        if !ok {
            vm.emit_warning(&format!(
                "stream_get_contents(): Failed to seek to position {} in the stream",
                offset
            ));
            return Ok(Value::False);
        }
    }
    if max_length == 0 {
        return Ok(Value::String(PhpString::from_bytes(b"")));
    }
    // _php_stream_copy_to_mem(): read until EOF (or maxlen bytes).
    let limit = if max_length < 0 {
        usize::MAX
    } else {
        max_length as usize
    };
    let mut out: Vec<u8> = Vec::new();
    let mut read_error: Option<i32> = None;
    FILE_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        let Some(fh) = handles.get_mut(&fid) else {
            return;
        };
        let mut buf = vec![0u8; 8192];
        while out.len() < limit {
            if limit != usize::MAX && fh.eof {
                break;
            }
            let want = (limit - out.len()).min(buf.len());
            match fh.read_raw(&mut buf[..want], false) {
                Ok(0) => {
                    fh.eof = true;
                    break;
                }
                Ok(n) => out.extend_from_slice(&buf[..n]),
                Err(e) => {
                    if fh.sock.is_none() && e.kind() != std::io::ErrorKind::WouldBlock {
                        read_error = e.raw_os_error();
                        if read_error != Some(libc::EBADF) {
                            fh.eof = true;
                        }
                    }
                    break;
                }
            }
        }
    });
    if let Some(errno) = read_error {
        let line = vm.current_line;
        vm.emit_notice_at(
            &format!(
                "stream_get_contents(): Read of 8192 bytes failed with errno={} {}",
                errno,
                xp_socket::strerror(errno)
            ),
            line,
        );
    }
    Ok(Value::String(PhpString::from_vec(out)))
}

fn get_included_files_fn(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut arr = PhpArray::new();
    for f in &vm.included_files_list {
        arr.push(Value::String(PhpString::from_string(f.clone())));
    }
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

// iterator_apply moved to goro-ext-spl

/// The readonly properties zend_enum_register_props() declares on an enum:
/// `name`, and `value` for a backed enum.
fn enum_declared_props(vm: &Vm, class_lower: &[u8]) -> &'static [&'static [u8]] {
    match vm.classes.get(class_lower) {
        Some(c) if c.is_enum && c.enum_backing_type.is_some() => &[b"name", b"value"],
        Some(c) if c.is_enum => &[b"name"],
        _ => &[],
    }
}

fn property_exists_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let obj_or_class = args.first().unwrap_or(&Value::Null);
    let prop_name = args
        .get(1)
        .map(|v| v.to_php_string())
        .unwrap_or_else(|| PhpString::from_bytes(b""));
    let prop_bytes = prop_name.as_bytes();
    // ArrayObject ARRAY_AS_PROPS: has_property(ZEND_PROPERTY_EXISTS) checks the storage.
    if let Value::Object(o) = obj_or_class {
        if let Some(r) = vm.spl_property_exists_hook(o, prop_bytes) {
            return Ok(if r { Value::True } else { Value::False });
        }
    }

    // Helper: a property is "visible" on `class_lower` if declared on it OR
    // declared on an ancestor with non-private visibility.
    fn class_has_visible_prop(vm: &Vm, class_lower: &[u8], pname: &[u8]) -> bool {
        vm.class_view_prop(class_lower, pname).is_some()
            || enum_declared_props(vm, class_lower).contains(&pname)
    }

    // Extension-provided property handlers (has_property with ZEND_PROPERTY_EXISTS)
    if let Value::Object(obj) = obj_or_class {
        if let Some(r) = vm.native_prop_read(obj, prop_bytes) {
            return Ok(if r.is_some() {
                Value::True
            } else {
                Value::False
            });
        }
    }
    match obj_or_class {
        Value::Object(obj) => {
            let obj_borrow = obj.borrow();
            let class_lower: Vec<u8> = obj_borrow
                .class_name
                .iter()
                .map(|b| b.to_ascii_lowercase())
                .collect();
            // Declared property check (filters out inherited private).
            if class_has_visible_prop(vm, &class_lower, prop_bytes) {
                return Ok(Value::True);
            }
            // Otherwise consider this as a dynamic property only if it is
            // present on the instance AND not declared anywhere on the
            // class hierarchy (so that inherited private props don't count).
            drop(obj_borrow);
            // has_property(ZEND_PROPERTY_EXISTS) from the calling scope.
            Ok(if vm.prop_exists_from_scope(obj, prop_bytes) {
                Value::True
            } else {
                Value::False
            })
        }
        Value::String(s) => {
            let class_lower: Vec<u8> = s
                .as_bytes()
                .iter()
                .map(|b| b.to_ascii_lowercase())
                .collect();
            if !vm.classes.contains_key(&class_lower) && !vm.is_known_builtin_class(&class_lower) {
                vm.try_autoload_class(s.as_bytes());
                if vm.current_exception.is_some() {
                    return Err(VmError {
                        message: "Uncaught exception".into(),
                        line: vm.current_line,
                    });
                }
            }
            Ok(if class_has_visible_prop(vm, &class_lower, prop_bytes) {
                Value::True
            } else {
                Value::False
            })
        }
        Value::Reference(r) => {
            let mut a = args.to_vec();
            a[0] = r.borrow().clone();
            property_exists_fn(vm, &a)
        }
        other => Err(throw_type_error_msg(
            vm,
            format!(
                "property_exists(): Argument #1 ($object_or_class) must be of type object|string, {} given",
                Vm::value_type_name(other)
            ),
        )),
    }
}

fn get_mangled_object_vars_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Object(outer)) = args.first() {
        // For initialized lazy proxy, redirect to the real backing object.
        let obj_rc = vm.proxy_redirect(outer);
        // (BcMath\Number's properties are virtual: zend_std_get_properties()
        // has none)
        if obj_rc
            .borrow()
            .class_name
            .eq_ignore_ascii_case(b"BcMath\\Number")
        {
            return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
        }
        Ok(Value::Array(Vm::proptable_to_symtable(
            vm.object_props_mangled(&obj_rc),
        )))
    } else {
        Ok(Value::Null)
    }
}

/// A `resource $resource` parameter (zend_parse_arg Z_PARAM_RESOURCE).
fn resource_param(vm: &mut Vm, val: &Value, func: &str) -> Result<(), VmError> {
    let v = val.deref();
    if matches!(v, Value::Resource(_)) {
        return Ok(());
    }
    let msg = format!(
        "{}(): Argument #1 ($resource) must be of type resource, {} given",
        func,
        Vm::value_type_name(&v)
    );
    let exc = vm.throw_type_error(msg.clone());
    vm.current_exception = Some(exc);
    Err(VmError {
        message: msg,
        line: vm.current_line,
    })
}

fn get_resource_id_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    resource_param(vm, val, "get_resource_id")?;
    let id = val.resource_id();
    if id >= 0 {
        Ok(Value::Long(id))
    } else {
        Ok(Value::Long(0))
    }
}

fn hrtime_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let as_number = args.first().map(|v| v.is_truthy()).unwrap_or(false);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let nanos = now.as_nanos() as i64;
    if as_number {
        Ok(Value::Long(nanos))
    } else {
        let mut arr = PhpArray::new();
        arr.push(Value::Long(now.as_secs() as i64));
        arr.push(Value::Long(now.subsec_nanos() as i64));
        Ok(Value::Array(Rc::new(RefCell::new(arr))))
    }
}

fn gethostname_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    #[cfg(unix)]
    {
        let mut buf = [0u8; 256];
        let ret = unsafe { libc::gethostname(buf.as_mut_ptr() as *mut i8, buf.len()) };
        if ret == 0 {
            let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            Ok(Value::String(PhpString::from_bytes(&buf[..len])))
        } else {
            Ok(Value::False)
        }
    }
    #[cfg(not(unix))]
    {
        Ok(Value::String(PhpString::from_bytes(b"localhost")))
    }
}

fn umask_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    #[cfg(unix)]
    {
        if let Some(mask) = args.first() {
            let new_mask = mask.to_long() as u32;
            let old = unsafe { libc::umask(new_mask as libc::mode_t) };
            Ok(Value::Long(old as i64))
        } else {
            // Get current umask by setting and restoring
            let current = unsafe { libc::umask(0o022) };
            unsafe { libc::umask(current) };
            Ok(Value::Long(current as i64))
        }
    }
    #[cfg(not(unix))]
    {
        Ok(Value::Long(0))
    }
}

/// php_exec_ex(): zpp of exec()/system()/passthru() ($command is a path
/// string: no NUL bytes, not empty).
fn exec_command_arg(vm: &mut Vm, fname: &str, args: &[Value]) -> Result<Vec<u8>, VmError> {
    let cmd = args.first().unwrap_or(&Value::Null).to_php_string();
    let cmd = cmd.as_bytes().to_vec();
    if cmd.contains(&0) {
        return Err(throw_value_error_in(
            vm,
            fname,
            args,
            &format!(
                "{}(): Argument #1 ($command) must not contain any null bytes",
                fname
            ),
        ));
    }
    if cmd.is_empty() {
        return Err(throw_value_error_in(
            vm,
            fname,
            args,
            &format!("{}(): Argument #1 ($command) must not be empty", fname),
        ));
    }
    Ok(cmd)
}

fn exec_strip_trailing_whitespace(line: &[u8]) -> &[u8] {
    let mut l = line.len();
    while l > 0 && matches!(line[l - 1], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
        l -= 1;
    }
    &line[..l]
}

/// php_exec(): run `cmd` through /bin/sh. Mode 1 (system) echoes each
/// line, mode 2 (exec) collects the lines without trailing whitespace into
/// `array`, mode 3 (passthru) copies the raw output. Returns the return
/// value (the last line for modes 1/2) and the exit status.
fn php_exec(
    vm: &mut Vm,
    fname: &str,
    mode: u8,
    cmd: &[u8],
    array: Option<&Rc<RefCell<PhpArray>>>,
) -> (Value, i64) {
    use std::io::{BufRead, Read};
    #[cfg(unix)]
    let cmd_os = {
        use std::os::unix::ffi::OsStrExt;
        std::ffi::OsStr::from_bytes(cmd).to_os_string()
    };
    #[cfg(not(unix))]
    let cmd_os = std::ffi::OsString::from(String::from_utf8_lossy(cmd).into_owned());
    // Output written so far reaches the terminal before the child's.
    vm.flush_cli_output();
    let child = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(cmd_os)
        .stdout(std::process::Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(_) => {
            let line = vm.current_line;
            vm.emit_warning_at(
                &format!(
                    "{}(): Unable to fork [{}]",
                    fname,
                    goro_core::string::msg_str(cmd)
                ),
                line,
            );
            return (Value::False, -1);
        }
    };
    let mut ret = Value::Null;
    if let Some(stdout) = child.stdout.take() {
        let mut reader = std::io::BufReader::new(stdout);
        if mode == 3 {
            let mut buf = [0u8; 4096];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                vm.write_output(&buf[..n]);
                vm.flush_cli_output();
            }
        } else {
            let mut last: Option<Vec<u8>> = None;
            loop {
                let mut line = Vec::new();
                match reader.read_until(b'\n', &mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                if mode == 1 {
                    vm.write_output(&line);
                    vm.flush_cli_output();
                } else if let Some(arr) = array {
                    let stripped = exec_strip_trailing_whitespace(&line).to_vec();
                    arr.borrow_mut()
                        .push(Value::String(PhpString::from_vec(stripped)));
                }
                last = Some(line);
            }
            let last = last.unwrap_or_default();
            ret = Value::String(PhpString::from_vec(
                exec_strip_trailing_whitespace(&last).to_vec(),
            ));
        }
    }
    let code = match child.wait() {
        Ok(st) => st.code().map(i64::from).unwrap_or(-1),
        Err(_) => -1,
    };
    (ret, code)
}

fn exec_assign_ref(v: Option<&Value>, val: Value) {
    if let Some(Value::Reference(r)) = v {
        *r.borrow_mut() = val;
    }
}

fn exec_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let cmd = exec_command_arg(vm, "exec", args)?;
    let array = match args.get(1) {
        Some(Value::Reference(r)) => {
            let cur = r.borrow().clone();
            let a = match cur {
                Value::Array(a) => {
                    // SEPARATE_ARRAY
                    let copy = a.borrow().clone();
                    Rc::new(RefCell::new(copy))
                }
                _ => Rc::new(RefCell::new(PhpArray::new())),
            };
            *r.borrow_mut() = Value::Array(a.clone());
            Some(a)
        }
        _ => None,
    };
    let (ret, code) = php_exec(vm, "exec", 2, &cmd, array.as_ref());
    exec_assign_ref(args.get(2), Value::Long(code));
    Ok(ret)
}

fn system_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let cmd = exec_command_arg(vm, "system", args)?;
    let (ret, code) = php_exec(vm, "system", 1, &cmd, None);
    exec_assign_ref(args.get(1), Value::Long(code));
    Ok(ret)
}

fn shell_exec_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    use std::io::Read;
    use std::os::unix::ffi::OsStrExt;
    let cmd = args
        .first()
        .map(|v| v.deref())
        .unwrap_or(Value::Null)
        .to_php_string();
    if cmd.as_bytes().contains(&0) {
        return Err(throw_value_error_msg(
            vm,
            "shell_exec(): Argument #1 ($command) must not contain any null bytes",
        ));
    }
    if cmd.as_bytes().is_empty() {
        return Err(throw_value_error_msg(
            vm,
            "shell_exec(): Argument #1 ($command) must not be empty",
        ));
    }
    vm.flush_cli_output();
    let child = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(std::ffi::OsStr::from_bytes(cmd.as_bytes()))
        .stdout(std::process::Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(_) => {
            vm.emit_warning(&format!(
                "shell_exec(): Unable to execute '{}'",
                cmd.to_msg_string()
            ));
            return Ok(Value::False);
        }
    };
    let mut out = Vec::new();
    if let Some(mut so) = child.stdout.take() {
        let _ = so.read_to_end(&mut out);
    }
    let _ = child.wait();
    // PHP returns null when the command produced no output
    if out.is_empty() {
        Ok(Value::Null)
    } else {
        Ok(Value::String(PhpString::from_vec(out)))
    }
}

fn passthru_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let cmd = exec_command_arg(vm, "passthru", args)?;
    let (ret, code) = php_exec(vm, "passthru", 3, &cmd, None);
    exec_assign_ref(args.get(1), Value::Long(code));
    Ok(ret)
}

struct GetoptOpt {
    opt_char: u8,
    opt_name: Option<Vec<u8>>,
    need_param: u8,
}

/// Port of main/getopt.c php_getopt(). Returns the option char (0 for a long
/// option, -2 for an invalid argument, -1 at the end) and sets *optidx.
#[allow(clippy::if_same_then_else)] // branches mirror php-src
fn php_getopt(
    argv: &[Vec<u8>],
    opts: &[GetoptOpt],
    optarg: &mut Option<Vec<u8>>,
    optind: &mut usize,
    optchr: &mut usize,
    dash: &mut bool,
    optidx: &mut isize,
) -> i32 {
    const INVALID: i32 = -2;
    let argc = argv.len();
    *optidx = -1;
    if *optind >= argc {
        return -1;
    }
    let at = |i: usize, j: usize| -> u8 { argv[i].get(j).copied().unwrap_or(0) };
    if !*dash {
        if at(*optind, 0) != b'-' {
            return -1;
        } else if at(*optind, 1) == 0 {
            return -1;
        }
    }
    let mut arg_start: usize;
    if at(*optind, 0) == b'-' && at(*optind, 1) == b'-' {
        if at(*optind, 2) == 0 {
            *optind += 1;
            return -1;
        }
        arg_start = 2;
        let arg = &argv[*optind];
        let mut arg_end = arg.len() - 1;
        let search_end = (arg_start + arg_end.saturating_sub(arg_start)).min(arg.len());
        if let Some(pos) = arg[arg_start..search_end].iter().position(|&c| c == b'=') {
            arg_end = pos;
            arg_start += 1;
        } else {
            arg_end -= 1;
        }
        loop {
            *optidx += 1;
            let o = &opts[*optidx as usize];
            if o.opt_char == b'-' {
                *optind += 1;
                return INVALID;
            }
            if let Some(name) = &o.opt_name {
                let cand = &arg[2..];
                let n = arg_end.min(cand.len());
                let name_prefix = &name[..arg_end.min(name.len())];
                if &cand[..n] == name_prefix && arg_end == name.len() {
                    break;
                }
            }
        }
        *optchr = 0;
        *dash = false;
        arg_start += opts[*optidx as usize]
            .opt_name
            .as_ref()
            .map(|n| n.len())
            .unwrap_or(0);
    } else {
        if !*dash {
            *dash = true;
            *optchr = 1;
        }
        if at(*optind, *optchr) == b':' {
            *dash = false;
            *optind += 1;
            return INVALID;
        }
        arg_start = 1 + *optchr;
    }
    if *optidx < 0 {
        loop {
            *optidx += 1;
            let o = &opts[*optidx as usize];
            if o.opt_char == b'-' {
                if at(*optind, *optchr + 1) == 0 {
                    *dash = false;
                    *optind += 1;
                } else {
                    *optchr += 1;
                }
                return INVALID;
            } else if at(*optind, *optchr) == o.opt_char {
                break;
            }
        }
    }
    let o = &opts[*optidx as usize];
    if o.need_param > 0 {
        *dash = false;
        if at(*optind, arg_start) == 0 {
            *optind += 1;
            if *optind == argc {
                if o.need_param == 1 {
                    return INVALID;
                }
            } else if o.need_param == 1 {
                *optarg = Some(argv[*optind].clone());
                *optind += 1;
            }
        } else if at(*optind, arg_start) == b'=' {
            *optarg = Some(argv[*optind][arg_start + 1..].to_vec());
            *optind += 1;
        } else {
            *optarg = Some(argv[*optind][arg_start..].to_vec());
            *optind += 1;
        }
        o.opt_char as i32
    } else {
        if arg_start >= 2 && !(at(*optind, 0) == b'-' && at(*optind, 1) == b'-') {
            if at(*optind, *optchr + 1) == 0 {
                *dash = false;
                *optind += 1;
            } else {
                *optchr += 1;
            }
        } else {
            *optind += 1;
        }
        o.opt_char as i32
    }
}

fn getopt_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let options = args
        .first()
        .map(|v| v.to_php_string().as_bytes().to_vec())
        .unwrap_or_default();
    let optind_ref = match args.get(2) {
        Some(Value::Reference(r)) => Some(r.clone()),
        _ => None,
    };
    if let Some(r) = &optind_ref {
        *r.borrow_mut() = Value::Long(1);
    }
    // argv from $_SERVER['argv'], else the global $argv.
    let argv_val = match vm.get_global(b"_SERVER") {
        Some(Value::Array(server)) => server.borrow().get_str(b"argv").cloned(),
        _ => None,
        // The CLI SAPI always provides $_SERVER['argv'] (at least the script name).
    }
    .or_else(|| {
        Some(Value::Array(Rc::new(RefCell::new({
            let mut a = PhpArray::new();
            a.push(Value::String(PhpString::from_string(
                vm.current_file.to_string(),
            )));
            a
        }))))
    });
    let argv: Vec<Vec<u8>> = match argv_val.map(|v| v.deref()) {
        Some(Value::Array(a)) => a
            .borrow()
            .values()
            .map(|v| v.to_php_string().as_bytes().to_vec())
            .collect(),
        Some(_) => return Ok(Value::False),
        None => return Ok(Value::False),
    };
    // parse_opts()
    let mut opts: Vec<GetoptOpt> = Vec::new();
    let mut i = 0;
    while i < options.len() && options[i].is_ascii_alphanumeric() {
        let c = options[i];
        i += 1;
        let mut need = 0;
        if options.get(i) == Some(&b':') {
            need = 1;
            i += 1;
            if options.get(i) == Some(&b':') {
                need = 2;
                i += 1;
            }
        }
        opts.push(GetoptOpt {
            opt_char: c,
            opt_name: None,
            need_param: need,
        });
    }
    if let Some(Value::Array(longopts)) = args.get(1).map(|v| v.deref()) {
        for v in longopts.borrow().values() {
            let mut name = v.to_php_string().as_bytes().to_vec();
            let mut need = 0;
            if name.last() == Some(&b':') {
                need += 1;
                name.pop();
                if name.last() == Some(&b':') {
                    need += 1;
                    name.pop();
                }
            }
            opts.push(GetoptOpt {
                opt_char: 0,
                opt_name: Some(name),
                need_param: need,
            });
        }
    }
    opts.push(GetoptOpt {
        opt_char: b'-',
        opt_name: None,
        need_param: 0,
    });

    let mut result = PhpArray::new();
    let mut optind = 1usize;
    let mut optchr = 0usize;
    let mut dash = false;
    let mut optidx: isize = -1;
    loop {
        let mut optarg: Option<Vec<u8>> = None;
        let o = php_getopt(
            &argv,
            &opts,
            &mut optarg,
            &mut optind,
            &mut optchr,
            &mut dash,
            &mut optidx,
        );
        if o == -1 {
            break;
        }
        if o == -2 {
            continue;
        }
        let optname: Vec<u8> = if o == 0 {
            opts[optidx as usize].opt_name.clone().unwrap_or_default()
        } else {
            vec![if o == 1 { b'-' } else { o as u8 }]
        };
        let val = match optarg {
            Some(a) => Value::String(PhpString::from_vec(a)),
            None => Value::False,
        };
        let key = if !(optname.len() > 1 && optname[0] == b'0') {
            match std::str::from_utf8(&optname)
                .ok()
                .and_then(|s| s.parse::<i64>().ok())
            {
                Some(n) => ArrayKey::Int(n),
                None => ArrayKey::String(PhpString::from_vec(optname.clone())),
            }
        } else {
            ArrayKey::String(PhpString::from_vec(optname.clone()))
        };
        let existing = result.get(&key).cloned();
        match existing {
            Some(Value::Array(a)) => a.borrow_mut().push(val),
            Some(other) => {
                let mut a = PhpArray::new();
                a.push(other);
                a.push(val);
                result.set(key, Value::Array(Rc::new(RefCell::new(a))));
            }
            None => result.set(key, val),
        }
    }
    if let Some(r) = &optind_ref {
        *r.borrow_mut() = Value::Long(optind as i64);
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

// Stream stubs
fn stream_set_write_buffer_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(0))
}
fn stream_set_read_buffer_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(0))
}

#[allow(clippy::slow_vector_initialization)] // resize is conditional
fn stream_copy_to_stream_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let src_fid = args.first().unwrap_or(&Value::Null).to_long();
    let dst_fid = args.get(1).unwrap_or(&Value::Null).to_long();
    let max_length = args.get(2).map(|v| v.to_long()).unwrap_or(-1);
    let offset = args.get(3).map(|v| v.to_long()).unwrap_or(0);
    if offset > 0 {
        FILE_HANDLES.with(|h| {
            if let Some(fh) = h.borrow_mut().get_mut(&src_fid) {
                let _ = fh.file.seek(SeekFrom::Start(offset as u64));
            }
        });
    }
    // Pull bytes (post READ-filter) from source. Push them through dst
    // (which routes through WRITE filters in fwrite_fn).
    let has_src_read_filters = crate::stream_filters::chain_has_direction(
        src_fid,
        crate::stream_filters::STREAM_FILTER_READ,
    );
    let mut buf: Vec<u8> = Vec::new();
    if has_src_read_filters {
        let limit = if max_length < 0 {
            usize::MAX
        } else {
            max_length as usize
        };
        while buf.len() < limit {
            let want = (limit - buf.len()).min(8192);
            let chunk = match read_filtered_ex(vm, src_fid, want) {
                Some(c) => c,
                None if buf.is_empty() => return Ok(Value::False),
                None => break,
            };
            if chunk.is_empty() {
                break;
            }
            buf.extend_from_slice(&chunk);
        }
    } else {
        let data = FILE_HANDLES.with(|h| {
            let mut handles = h.borrow_mut();
            if let Some(fh) = handles.get_mut(&src_fid) {
                let limit = if max_length < 0 {
                    usize::MAX
                } else {
                    max_length as usize
                };
                let mut out = Vec::new();
                let mut chunk = vec![0u8; 8192];
                while out.len() < limit {
                    let want = (limit - out.len()).min(chunk.len());
                    match fh.read_raw(&mut chunk[..want], false) {
                        Ok(0) => {
                            fh.eof = true;
                            break;
                        }
                        Ok(n) => out.extend_from_slice(&chunk[..n]),
                        Err(_) if !out.is_empty() || fh.sock.is_some() => break,
                        Err(_) => return None,
                    }
                }
                Some(out)
            } else {
                None
            }
        });
        match data {
            Some(b) => buf = b,
            None => return Ok(Value::False),
        }
    }
    let len = buf.len();
    let filtered = if crate::stream_filters::chain_has_direction(
        dst_fid,
        crate::stream_filters::STREAM_FILTER_WRITE,
    ) {
        crate::stream_filters::run_write_chain(
            vm,
            dst_fid,
            Some(&buf),
            crate::stream_filters::PSFS_FLAG_NORMAL,
        )
        .1
    } else {
        buf.clone()
    };
    let exists = dst_fid == 1
        || dst_fid == 2
        || dst_fid == 3
        || FILE_HANDLES.with(|h| h.borrow().contains_key(&dst_fid));
    if !exists {
        return Ok(Value::False);
    }
    write_to_stream(vm, dst_fid, &filtered);
    Ok(Value::Long(len as i64))
}

fn stream_filter_register_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let name = args
        .first()
        .map(|v| v.to_php_string().as_bytes().to_vec())
        .unwrap_or_default();
    let class = args
        .get(1)
        .map(|v| v.to_php_string().as_bytes().to_vec())
        .unwrap_or_default();
    if name.is_empty() {
        return Err(throw_value_error_msg(
            vm,
            "stream_filter_register(): Argument #1 ($filter_name) must be a non-empty string",
        ));
    }
    if class.is_empty() {
        return Err(throw_value_error_msg(
            vm,
            "stream_filter_register(): Argument #2 ($class) must be a non-empty string",
        ));
    }
    Ok(
        if crate::stream_filters::register_user_filter(&name, &class) {
            Value::True
        } else {
            Value::False
        },
    )
}

fn stream_filter_append_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    attach_filter_impl(vm, args, false)
}

fn stream_filter_prepend_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    attach_filter_impl(vm, args, true)
}

/// Mode string of a stream (user streams keep theirs on the user stream).
fn stream_mode_of(fid: i64) -> String {
    if let Some(us) = userstream::get_us(fid) {
        return us.borrow().mode.clone();
    }
    FILE_HANDLES.with(|h| {
        h.borrow()
            .get(&fid)
            .map(|fh| fh.mode.clone())
            .unwrap_or_default()
    })
}

/// Create a filter and link it into a stream's chain; Ok(None) on failure.
fn attach_one(
    vm: &mut Vm,
    fid: i64,
    name: &[u8],
    params: Option<&Value>,
    is_read: bool,
    append: bool,
) -> Result<Option<crate::stream_filters::FilterRef>, VmError> {
    let persistent =
        goro_core::value::resource_type_of(fid).as_deref() == Some(b"persistent stream");
    let Some(f) = crate::stream_filters::create_filter(vm, name, params, persistent)? else {
        return Ok(None);
    };
    // php_stream_filter_append_ex() filters the pre-buffered data.
    let us = userstream::get_us(fid);
    let mut buffered = match &us {
        Some(u) => userstream::take_read_buffer(u),
        None => FILE_HANDLES.with(|h| {
            h.borrow_mut()
                .get_mut(&fid)
                .map(|fh| std::mem::take(&mut fh.filter_read_ready))
                .unwrap_or_default()
        }),
    };
    let r = crate::stream_filters::attach(vm, fid, f, is_read, append, Some(&mut buffered));
    match &us {
        Some(u) => userstream::put_read_buffer(u, buffered),
        None => FILE_HANDLES.with(|h| {
            if let Some(fh) = h.borrow_mut().get_mut(&fid) {
                fh.filter_read_ready = buffered;
            }
        }),
    }
    Ok(r.ok())
}

/// php_stream_apply_filter_list() for php://filter.
fn apply_filter_list(
    vm: &mut Vm,
    fid: i64,
    list: &str,
    read: bool,
    write: bool,
) -> Result<(), VmError> {
    for p in list.split('|').filter(|p| !p.is_empty()) {
        let name = data_uri_urldecode(p.as_bytes());
        for (on, is_read) in [(read, true), (write, false)] {
            if !on {
                continue;
            }
            if attach_one(vm, fid, &name, None, is_read, true)?.is_none() {
                crate::stream_filters::docref_warning(
                    vm,
                    &format!(
                        "Unable to create filter ({})",
                        goro_core::string::msg_str(&name)
                    ),
                );
            }
        }
    }
    Ok(())
}

/// apply_filter_to_stream()
fn attach_filter_impl(vm: &mut Vm, args: &[Value], prepend: bool) -> Result<Value, VmError> {
    let fname = if prepend {
        "stream_filter_prepend"
    } else {
        "stream_filter_append"
    };
    if args.len() < 2 || args.len() > 4 {
        let (q, c) = if args.len() < 2 {
            ("at least", 2)
        } else {
            ("at most", 4)
        };
        let msg = format!(
            "{}() expects {} {} arguments, {} given",
            fname,
            q,
            c,
            args.len()
        );
        let exc = vm.create_exception(b"ArgumentCountError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    let fid = require_open_stream(vm, &args[0], fname)?;
    let name = args[1].to_php_string().as_bytes().to_vec();
    let mut read_write = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    let params = args.get(3).map(|v| v.deref());
    if read_write & crate::stream_filters::STREAM_FILTER_ALL == 0 {
        let mode = stream_mode_of(fid);
        if mode.contains('r') || mode.contains('+') {
            read_write |= crate::stream_filters::STREAM_FILTER_READ;
        }
        if mode.contains('w') || mode.contains('+') || mode.contains('a') {
            read_write |= crate::stream_filters::STREAM_FILTER_WRITE;
        }
    }
    let mut last = None;
    for (bit, is_read) in [
        (crate::stream_filters::STREAM_FILTER_READ, true),
        (crate::stream_filters::STREAM_FILTER_WRITE, false),
    ] {
        if read_write & bit == 0 {
            continue;
        }
        match attach_one(vm, fid, &name, params.as_ref(), is_read, !prepend)? {
            Some(f) => last = Some(f),
            None => return Ok(Value::False),
        }
    }
    match last {
        Some(f) => Ok(Value::resource(crate::stream_filters::register_resource(
            &f,
        ))),
        None => Ok(Value::False),
    }
}

fn stream_filter_remove_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let res = match args.first().map(|v| v.deref()) {
        Some(Value::Resource(n)) => n,
        Some(v) => {
            let msg = format!(
                "stream_filter_remove(): Argument #1 ($stream_filter) must be of type resource, {} given",
                Vm::value_type_name(&v)
            );
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
        None => {
            let msg = "stream_filter_remove() expects exactly 1 argument, 0 given".to_string();
            let exc = vm.create_exception(b"ArgumentCountError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
    };
    let Some(f) = crate::stream_filters::filter_by_resource(res.id()) else {
        let msg = "stream_filter_remove(): supplied resource is not a valid stream filter resource"
            .to_string();
        let exc = vm.throw_type_error(msg.clone());
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    };
    let stream = f.borrow().stream;
    match crate::stream_filters::flush_filter(vm, &f, true) {
        Err(_) => {
            vm.emit_warning("stream_filter_remove(): Unable to flush filter, not removing");
            return Ok(Value::False);
        }
        Ok(Some((true, data))) => match userstream::get_us(stream) {
            Some(u) => {
                let mut b = userstream::take_read_buffer(&u);
                b.extend_from_slice(&data);
                userstream::put_read_buffer(&u, b);
            }
            None => FILE_HANDLES.with(|h| {
                if let Some(fh) = h.borrow_mut().get_mut(&stream) {
                    fh.filter_read_ready.extend_from_slice(&data);
                }
            }),
        },
        Ok(Some((false, data))) => {
            if userstream::get_us(stream).is_some() {
                userstream::write_unfiltered(vm, stream, &data)?;
            } else {
                write_unfiltered(vm, stream, &data);
            }
        }
        Ok(None) => {}
    }
    crate::stream_filters::detach(vm, &f, true);
    Ok(Value::True)
}
/// Validate that `arr` has shape ["wrapper"]["opt"] = value (i.e. two levels of arrays).
/// On invalid, throws a ValueError matching PHP.
fn validate_context_options(vm: &mut Vm, arr: &Value) -> Result<CtxOptions, VmError> {
    let mut out = CtxOptions::default();
    let outer = match arr {
        Value::Array(a) => a.clone(),
        _ => {
            let msg = "Options should have the form [\"wrappername\"][\"optionname\"] = $value"
                .to_string();
            let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
    };
    let outer_b = outer.borrow();
    for (k, v) in outer_b.iter() {
        let key_bytes: Vec<u8> = match k {
            ArrayKey::String(s) => s.as_bytes().to_vec(),
            ArrayKey::Int(_) => {
                let msg = "Options should have the form [\"wrappername\"][\"optionname\"] = $value"
                    .to_string();
                let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
                vm.current_exception = Some(exc);
                return Err(VmError {
                    message: msg,
                    line: vm.current_line,
                });
            }
        };
        let inner = match v {
            Value::Array(a) => a.clone(),
            _ => {
                let msg = "Options should have the form [\"wrappername\"][\"optionname\"] = $value"
                    .to_string();
                let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
                vm.current_exception = Some(exc);
                return Err(VmError {
                    message: msg,
                    line: vm.current_line,
                });
            }
        };
        let inner_b = inner.borrow();
        let mut opts = goro_core::fxhash::FxIndexMap::default();
        for (ik, iv) in inner_b.iter() {
            let ikey: Vec<u8> = match ik {
                ArrayKey::String(s) => s.as_bytes().to_vec(),
                ArrayKey::Int(n) => n.to_string().into_bytes(),
            };
            // php_stream_context_set_option() stores a dereferenced copy.
            opts.insert(ikey, iv.deref());
        }
        out.insert(key_bytes, opts);
    }
    Ok(out)
}

fn type_name_of(v: &Value) -> &'static str {
    match v {
        Value::Null | Value::Undef => "null",
        Value::True | Value::False => "bool",
        Value::Long(_) => "int",
        Value::Double(_) => "float",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
        Value::Resource(_) => "resource",
        Value::Reference(_) => "reference",
        _ => "unknown",
    }
}

fn resolve_context_id(val: &Value) -> Option<i64> {
    let fid = val.to_long();
    let has_ctx = STREAM_CONTEXTS.with(|c| c.borrow().contains_key(&fid));
    if has_ctx {
        return Some(fid);
    }
    // Stream resource: lazily attach default context.
    let has_stream = FILE_HANDLES.with(|h| h.borrow().contains_key(&fid));
    if has_stream {
        STREAM_CONTEXTS.with(|c| {
            c.borrow_mut().insert(fid, StreamContext::default());
        });
        return Some(fid);
    }
    None
}

fn apply_context_params(
    vm: &mut Vm,
    ctx: &mut StreamContext,
    params: &Value,
) -> Result<(), VmError> {
    let arr = match params {
        Value::Array(a) => a.clone(),
        _ => {
            let msg = "Invalid stream/context parameter".to_string();
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
    };
    let arr_b = arr.borrow();
    for (k, v) in arr_b.iter() {
        if let ArrayKey::String(s) = k {
            match s.as_bytes() {
                b"notification" => {
                    // zend_is_callable_ex()
                    ctx.notification = None;
                    if let Some(err) = callable_error(vm, v) {
                        let msg = format!(
                            "stream_context_set_params(): Argument #1 ($context) must be an array with valid callbacks as values, {}",
                            err
                        );
                        let exc = vm.throw_type_error(msg.clone());
                        vm.current_exception = Some(exc);
                        return Err(VmError {
                            message: msg,
                            line: vm.current_line,
                        });
                    }
                    ctx.notification = Some(v.clone());
                }
                b"options" => {
                    // If $v is not an array, raise TypeError matching PHP.
                    if !matches!(v, Value::Array(_)) {
                        let msg = "Invalid stream/context parameter".to_string();
                        let exc = vm.throw_type_error(msg.clone());
                        vm.current_exception = Some(exc);
                        return Err(VmError {
                            message: msg,
                            line: vm.current_line,
                        });
                    }
                    ctx.options = validate_context_options(vm, v)?;
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn context_options_to_array(opts: &CtxOptions) -> PhpArray {
    let mut arr = PhpArray::new();
    for (wrapper, inner) in opts {
        let mut inner_arr = PhpArray::new();
        for (k, v) in inner {
            inner_arr.set(ArrayKey::String(PhpString::from_vec(k.clone())), v.clone());
        }
        arr.set(
            ArrayKey::String(PhpString::from_vec(wrapper.clone())),
            Value::Array(Rc::new(RefCell::new(inner_arr))),
        );
    }
    arr
}

fn stream_context_create_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let mut ctx = StreamContext::default();
    if let Some(opts_val) = args.first() {
        if !matches!(opts_val, Value::Null | Value::Undef) {
            if !matches!(opts_val, Value::Array(_)) {
                let msg = format!(
                    "stream_context_create(): Argument #1 ($options) must be of type ?array, {} given",
                    type_name_of(opts_val)
                );
                let exc = vm.throw_type_error(msg.clone());
                vm.current_exception = Some(exc);
                return Err(VmError {
                    message: msg,
                    line: vm.current_line,
                });
            }
            ctx.options = validate_context_options(vm, opts_val)?;
        }
    }
    if let Some(params_val) = args.get(1) {
        if !matches!(params_val, Value::Null | Value::Undef) {
            if !matches!(params_val, Value::Array(_)) {
                let msg = format!(
                    "stream_context_create(): Argument #2 ($params) must be of type ?array, {} given",
                    type_name_of(params_val)
                );
                let exc = vm.throw_type_error(msg.clone());
                vm.current_exception = Some(exc);
                return Err(VmError {
                    message: msg,
                    line: vm.current_line,
                });
            }
            apply_context_params(vm, &mut ctx, params_val)?;
        }
    }
    let fid = goro_core::value::alloc_resource_id();
    STREAM_CONTEXTS.with(|c| {
        c.borrow_mut().insert(fid, ctx);
    });
    goro_core::value::register_resource(fid, b"stream-context");
    Ok(Value::resource(fid))
}

/// PHP_FUNCTION(stream_context_set_options)
fn stream_context_set_options_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let ctx_val = args.first().unwrap_or(&Value::Null);
    let fid = match resolve_context_id(ctx_val) {
        Some(n) => n,
        None => {
            let msg = "stream_context_set_options(): Argument #1 ($context) must be a valid stream/context".to_string();
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
    };
    let opts_v = args.get(1).map(|v| v.deref()).unwrap_or(Value::Null);
    if !matches!(opts_v, Value::Array(_)) {
        let msg = format!(
            "stream_context_set_options(): Argument #2 ($options) must be of type array, {} given",
            Vm::value_type_name(&opts_v)
        );
        let exc = vm.throw_type_error(msg.clone());
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    let opts = validate_context_options(vm, &opts_v)?;
    STREAM_CONTEXTS.with(|c| {
        let mut cb = c.borrow_mut();
        if let Some(ctx) = cb.get_mut(&fid) {
            for (wrapper, opt_map) in opts {
                let entry = ctx.options.entry(wrapper).or_default();
                for (k, v) in opt_map {
                    entry.insert(k, v);
                }
            }
        }
    });
    Ok(Value::True)
}

fn stream_context_set_option_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.len() == 2 {
        vm.emit_deprecated(
            "Calling stream_context_set_option() with 2 arguments is deprecated, use stream_context_set_options() instead",
        );
        if vm.current_exception.is_some() {
            return Err(VmError {
                message: String::new(),
                line: vm.current_line,
            });
        }
    }
    let ctx_val = args.first().unwrap_or(&Value::Null);
    let fid = match resolve_context_id(ctx_val) {
        Some(n) => n,
        None => {
            let msg = "stream_context_set_option(): Argument #1 ($context) must be a valid stream/context".to_string();
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
    };
    let arg2 = args.get(1).unwrap_or(&Value::Null);
    let arg3 = args.get(2);
    let arg4 = args.get(3);
    // Array signature: stream_context_set_option($ctx, array $options)
    if matches!(arg2, Value::Array(_)) {
        if let Some(v) = arg3 {
            if !matches!(v, Value::Null | Value::Undef) {
                let msg = "stream_context_set_option(): Argument #3 ($option_name) must be null when argument #2 ($wrapper_or_options) is an array".to_string();
                let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
                vm.current_exception = Some(exc);
                return Err(VmError {
                    message: msg,
                    line: vm.current_line,
                });
            }
        }
        if let Some(v) = arg4 {
            if !matches!(v, Value::Null | Value::Undef) {
                let msg = "stream_context_set_option(): Argument #4 ($value) cannot be provided when argument #2 ($wrapper_or_options) is an array".to_string();
                let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
                vm.current_exception = Some(exc);
                return Err(VmError {
                    message: msg,
                    line: vm.current_line,
                });
            }
        }
        let opts = validate_context_options(vm, arg2)?;
        STREAM_CONTEXTS.with(|c| {
            let mut cb = c.borrow_mut();
            if let Some(ctx) = cb.get_mut(&fid) {
                for (wrapper, opt_map) in opts {
                    let entry = ctx.options.entry(wrapper).or_default();
                    for (k, v) in opt_map {
                        entry.insert(k, v);
                    }
                }
            }
        });
        return Ok(Value::True);
    }
    // String/wrapper signature
    if matches!(arg2, Value::String(_)) {
        // 2-arg form: emit deprecation, then error on missing $option_name
        if arg3.is_none() && arg4.is_none() {
            let msg = "stream_context_set_option(): Argument #3 ($option_name) cannot be null when argument #2 ($wrapper_or_options) is a string".to_string();
            let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
        if matches!(arg3, Some(Value::Null | Value::Undef) | None) {
            let msg = "stream_context_set_option(): Argument #3 ($option_name) cannot be null when argument #2 ($wrapper_or_options) is a string".to_string();
            let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
        if arg4.is_none() {
            let msg = "stream_context_set_option(): Argument #4 ($value) must be provided when argument #2 ($wrapper_or_options) is a string".to_string();
            let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
        let wrapper = arg2.to_php_string().as_bytes().to_vec();
        let opt = arg3.unwrap().to_php_string().as_bytes().to_vec();
        let val = arg4.unwrap().clone();
        STREAM_CONTEXTS.with(|c| {
            let mut cb = c.borrow_mut();
            if let Some(ctx) = cb.get_mut(&fid) {
                let e = ctx.options.entry(wrapper).or_default();
                e.insert(opt, val);
            }
        });
        return Ok(Value::True);
    }
    Ok(Value::False)
}

fn stream_context_get_options_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let ctx_val = args.first().unwrap_or(&Value::Null);
    let fid = match resolve_context_id(ctx_val) {
        Some(n) => n,
        None => {
            let msg = "stream_context_get_options(): Argument #1 ($stream_or_context) must be a valid stream/context".to_string();
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
    };
    let arr = STREAM_CONTEXTS.with(|c| {
        c.borrow()
            .get(&fid)
            .map(|ctx| context_options_to_array(&ctx.options))
            .unwrap_or_default()
    });
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

fn stream_context_set_params_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let ctx_val = args.first().unwrap_or(&Value::Null);
    let fid = match resolve_context_id(ctx_val) {
        Some(n) => n,
        None => {
            let msg = "stream_context_set_params(): Argument #1 ($context) must be a valid stream/context".to_string();
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
    };
    let params = args.get(1).unwrap_or(&Value::Null);
    let mut ctx_clone = STREAM_CONTEXTS.with(|c| c.borrow().get(&fid).cloned().unwrap_or_default());
    apply_context_params(vm, &mut ctx_clone, params)?;
    STREAM_CONTEXTS.with(|c| {
        c.borrow_mut().insert(fid, ctx_clone);
    });
    Ok(Value::True)
}

fn stream_context_get_params_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let ctx_val = args.first().unwrap_or(&Value::Null);
    let fid = match resolve_context_id(ctx_val) {
        Some(n) => n,
        None => {
            let msg = "stream_context_get_params(): Argument #1 ($stream_or_context) must be a valid stream/context".to_string();
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
    };
    let mut arr = PhpArray::new();
    STREAM_CONTEXTS.with(|c| {
        if let Some(ctx) = c.borrow().get(&fid) {
            if let Some(n) = &ctx.notification {
                arr.set(
                    ArrayKey::String(PhpString::from_bytes(b"notification")),
                    n.clone(),
                );
            }
            arr.set(
                ArrayKey::String(PhpString::from_bytes(b"options")),
                Value::Array(Rc::new(RefCell::new(context_options_to_array(
                    &ctx.options,
                )))),
            );
        } else {
            arr.set(
                ArrayKey::String(PhpString::from_bytes(b"options")),
                Value::Array(Rc::new(RefCell::new(PhpArray::new()))),
            );
        }
    });
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

/// Builtins that call php_stream_context_from_zval(zcontext, flags & NO_DEFAULT):
/// a missing context argument allocates FG(default_context) (a resource id).
macro_rules! default_ctx_entry {
    ($name:ident, $inner:path, $ctx:expr, $flags:expr) => {
        fn $name(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
            let flags: Option<usize> = $flags;
            let no_default = flags
                .and_then(|i| args.get(i))
                .is_some_and(|f| f.deref().to_long() & 16 != 0);
            touch_default_context(args.get($ctx), no_default);
            $inner(vm, args)
        }
    };
}
default_ctx_entry!(ctx_copy_fn2_entry, copy_fn2, 2, None);
default_ctx_entry!(ctx_dir_fn_entry, dir_fn, 1, None);
default_ctx_entry!(ctx_file_fn_entry, file_fn, 2, Some(1));
default_ctx_entry!(
    ctx_file_get_contents_fn_entry,
    file_get_contents_fn,
    2,
    None
);
default_ctx_entry!(
    ctx_file_put_contents_fn_entry,
    file_put_contents_fn,
    3,
    Some(2)
);
default_ctx_entry!(ctx_fopen_fn_entry, fopen_fn, 3, None);
default_ctx_entry!(ctx_mkdir_fn_entry, mkdir_fn, 3, None);
default_ctx_entry!(ctx_opendir_fn_entry, opendir_fn, 1, None);
default_ctx_entry!(ctx_readfile_fn_entry, readfile_fn, 2, None);
default_ctx_entry!(ctx_rename_fn2_entry, rename_fn2, 2, None);
default_ctx_entry!(ctx_rmdir_fn_entry, rmdir_fn, 1, None);
default_ctx_entry!(ctx_scandir_fn_entry, scandir_fn, 2, None);
default_ctx_entry!(
    ctx_stream_socket_client_entry,
    xp_socket::stream_socket_client,
    5,
    Some(4)
);
default_ctx_entry!(
    ctx_stream_socket_server_entry,
    xp_socket::stream_socket_server,
    4,
    Some(3)
);
default_ctx_entry!(ctx_unlink_fn2_entry, unlink_fn2, 1, None);

thread_local! {
    static DEFAULT_STREAM_CTX: std::cell::Cell<Option<i64>> = const { std::cell::Cell::new(None) };
}

/// The stream-context list destructor (php_stream_context_free()): a
/// context created by PHP code is freed with its last reference (not the
/// default context, which FG(default_context) holds).
fn stream_context_dtor(_vm: &mut Vm, id: i64) {
    if DEFAULT_STREAM_CTX.with(|d| d.get()) == Some(id) {
        return;
    }
    STREAM_CONTEXTS.with(|c| {
        c.borrow_mut().remove(&id);
    });
    goro_core::value::remove_resource(id);
}

/// FG(default_context), allocated on first use (php_stream_context_from_zval).
pub(crate) fn default_context() -> i64 {
    DEFAULT_STREAM_CTX.with(|d| {
        if let Some(id) = d.get() {
            return id;
        }
        let id = goro_core::value::alloc_resource_id();
        STREAM_CONTEXTS.with(|c| {
            c.borrow_mut().insert(id, StreamContext::default());
        });
        goro_core::value::register_resource(id, b"stream-context");
        d.set(Some(id));
        id
    })
}

/// php_stream_context_from_zval(zcontext, no_default) for its side effect:
/// a null / missing context argument allocates the default context.
pub(crate) fn touch_default_context(ctx_arg: Option<&Value>, no_default: bool) {
    if !no_default
        && matches!(
            ctx_arg.map(|v| v.deref()),
            None | Some(Value::Null | Value::Undef)
        )
    {
        default_context();
    }
}

fn stream_context_get_default_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Accept optional $options array. If provided, validate and merge.
    if let Some(opts) = args.first() {
        if !matches!(opts, Value::Null | Value::Undef) {
            let merged = validate_context_options(vm, opts)?;
            let fid = default_context();
            STREAM_CONTEXTS.with(|c| {
                let mut cb = c.borrow_mut();
                if let Some(ctx) = cb.get_mut(&fid) {
                    for (wrapper, opt_map) in merged {
                        let entry = ctx.options.entry(wrapper).or_default();
                        for (k, v) in opt_map {
                            entry.insert(k, v);
                        }
                    }
                }
            });
            return Ok(Value::resource(fid));
        }
    }
    let fid = default_context();
    Ok(Value::resource(fid))
}

fn stream_context_set_default_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(opts) = args.first() {
        if !matches!(opts, Value::Null | Value::Undef) {
            let merged = validate_context_options(vm, opts)?;
            let fid = default_context();
            STREAM_CONTEXTS.with(|c| {
                let mut cb = c.borrow_mut();
                if let Some(ctx) = cb.get_mut(&fid) {
                    for (wrapper, opt_map) in merged {
                        let entry = ctx.options.entry(wrapper).or_default();
                        for (k, v) in opt_map {
                            entry.insert(k, v);
                        }
                    }
                }
            });
        }
    }
    stream_context_get_default_fn(vm, args)
}

fn stream_is_local_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    match val {
        Value::String(s) => {
            let s_str = s.to_string_lossy();
            if s_str.starts_with("http://")
                || s_str.starts_with("https://")
                || s_str.starts_with("ftp://")
            {
                Ok(Value::False)
            } else {
                Ok(Value::True)
            }
        }
        _ => Ok(Value::True),
    }
}

fn stream_get_meta_data_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let first = args.first().unwrap_or(&Value::Null);
    // Accept either a stream resource or a directory resource.
    let raw_fid = first.to_long();
    let is_dir = DIR_HANDLES.with(|h| h.borrow().contains_key(&raw_fid));
    if is_dir {
        let mut arr = PhpArray::new();
        arr.set(
            ArrayKey::String(PhpString::from_bytes(b"timed_out")),
            Value::False,
        );
        arr.set(
            ArrayKey::String(PhpString::from_bytes(b"blocked")),
            Value::True,
        );
        arr.set(
            ArrayKey::String(PhpString::from_bytes(b"eof")),
            Value::False,
        );
        arr.set(
            ArrayKey::String(PhpString::from_bytes(b"wrapper_type")),
            Value::String(PhpString::from_bytes(b"plainfile")),
        );
        arr.set(
            ArrayKey::String(PhpString::from_bytes(b"stream_type")),
            Value::String(PhpString::from_bytes(b"dir")),
        );
        arr.set(
            ArrayKey::String(PhpString::from_bytes(b"mode")),
            Value::String(PhpString::from_bytes(b"r")),
        );
        arr.set(
            ArrayKey::String(PhpString::from_bytes(b"unread_bytes")),
            Value::Long(0),
        );
        arr.set(
            ArrayKey::String(PhpString::from_bytes(b"seekable")),
            Value::True,
        );
        return Ok(Value::Array(Rc::new(RefCell::new(arr))));
    }
    let fid = require_open_stream(vm, first, "stream_get_meta_data")?;
    let mut arr = PhpArray::new();
    FILE_HANDLES.with(|h| {
        if let Some(fh) = h.borrow().get(&fid) {
            // For user streams, prepend wrapper_data (the wrapper object instance).
            if let Some(obj) = &fh.user_wrapper {
                arr.set(
                    ArrayKey::String(PhpString::from_bytes(b"wrapper_data")),
                    obj.clone(),
                );
            }
            // Order matches PHP's _php_stream_populate_meta_data():
            // timed_out, blocked, eof, [wrapper_data], wrapper_type, stream_type,
            // mode, unread_bytes, seekable, uri.
            // data: streams report their RFC 2397 meta data instead of
            // timed_out/blocked/eof.
            if let Some(dm) = &fh.data_meta {
                for (k, v) in dm {
                    arr.set(ArrayKey::String(PhpString::from_bytes(k)), v.clone());
                }
                let mut put =
                    |k: &[u8], v: Value| arr.set(ArrayKey::String(PhpString::from_bytes(k)), v);
                put(
                    b"wrapper_type",
                    Value::String(PhpString::from_string(fh.wrapper_type.clone())),
                );
                put(
                    b"stream_type",
                    Value::String(PhpString::from_string(fh.stream_type.clone())),
                );
                put(
                    b"mode",
                    Value::String(PhpString::from_string(fh.mode.clone())),
                );
                put(
                    b"unread_bytes",
                    Value::Long(fh.filter_read_ready.len() as i64),
                );
                put(b"seekable", Value::True);
                put(
                    b"uri",
                    Value::String(PhpString::from_string(fh.uri.clone())),
                );
                return;
            }
            // PHP_STREAM_OPTION_META_DATA_API: sockets report their own
            // state; descriptor-backed STDIO streams read O_NONBLOCK.
            let (timed_out, blocked) = match xp_socket::meta(fh) {
                Some(m) => m,
                None => {
                    let blocked =
                        if matches!(fh.special, SpecialStream::Memory | SpecialStream::Temp) {
                            true
                        } else {
                            use std::os::unix::io::AsRawFd;
                            let fl = unsafe { libc::fcntl(fh.file.as_raw_fd(), libc::F_GETFL, 0) };
                            fl < 0 || fl & libc::O_NONBLOCK == 0
                        };
                    (false, blocked)
                }
            };
            if let Some(m) = fh.crypto.as_ref().and_then(|c| c.meta()) {
                arr.set(
                    ArrayKey::String(PhpString::from_bytes(b"crypto")),
                    Value::Array(Rc::new(RefCell::new(m))),
                );
            }
            arr.set(
                ArrayKey::String(PhpString::from_bytes(b"timed_out")),
                if timed_out { Value::True } else { Value::False },
            );
            arr.set(
                ArrayKey::String(PhpString::from_bytes(b"blocked")),
                if blocked { Value::True } else { Value::False },
            );
            arr.set(
                ArrayKey::String(PhpString::from_bytes(b"eof")),
                if fh.eof { Value::True } else { Value::False },
            );
            if let Some(wd) = HTTP_WRAPPER_DATA.with(|w| w.borrow().get(&fid).cloned()) {
                arr.set(ArrayKey::String(PhpString::from_bytes(b"wrapper_data")), wd);
            }
            if !fh.wrapper_type.is_empty() {
                arr.set(
                    ArrayKey::String(PhpString::from_bytes(b"wrapper_type")),
                    Value::String(PhpString::from_string(fh.wrapper_type.clone())),
                );
            }
            arr.set(
                ArrayKey::String(PhpString::from_bytes(b"stream_type")),
                Value::String(PhpString::from_string(fh.stream_type.clone())),
            );
            arr.set(
                ArrayKey::String(PhpString::from_bytes(b"mode")),
                Value::String(PhpString::from_string(fh.mode.clone())),
            );
            arr.set(
                ArrayKey::String(PhpString::from_bytes(b"unread_bytes")),
                Value::Long(fh.filter_read_ready.len() as i64),
            );
            arr.set(
                ArrayKey::String(PhpString::from_bytes(b"seekable")),
                if fh.seekable {
                    Value::True
                } else {
                    Value::False
                },
            );
            if !fh.uri.is_empty() {
                arr.set(
                    ArrayKey::String(PhpString::from_bytes(b"uri")),
                    Value::String(PhpString::from_string(fh.uri.clone())),
                );
            }
        }
    });
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

fn stream_wrapper_register_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let scheme = args
        .first()
        .unwrap_or(&Value::Null)
        .to_php_string()
        .as_bytes()
        .to_vec();
    let class = args
        .get(1)
        .unwrap_or(&Value::Null)
        .to_php_string()
        .as_bytes()
        .to_vec();
    let flags = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    if scheme.is_empty() {
        vm.emit_warning("stream_wrapper_register(): Protocol must not be empty");
        return Ok(Value::False);
    }
    // Check if already registered (built-in or user). PHP returns false and emits
    // a warning if a wrapper for this protocol already exists.
    let scheme_lower: Vec<u8> = scheme.iter().map(|b| b.to_ascii_lowercase()).collect();
    let builtin = matches!(
        scheme_lower.as_slice(),
        b"file"
            | b"http"
            | b"https"
            | b"ftp"
            | b"ftps"
            | b"php"
            | b"data"
            | b"compress.zlib"
            | b"compress.bzip2"
            | b"phar"
            | b"glob"
    );
    let already_user = USER_WRAPPERS.with(|w| w.borrow().contains_key(&scheme_lower));
    if builtin || already_user {
        vm.emit_warning(&format!(
            "stream_wrapper_register(): Protocol {}:// is already defined.",
            goro_core::string::msg_str(&scheme)
        ));
        return Ok(Value::False);
    }
    USER_WRAPPERS.with(|w| {
        w.borrow_mut().insert(
            scheme_lower,
            UserWrapper {
                class_name: class,
                flags,
            },
        );
    });
    Ok(Value::True)
}

fn stream_wrapper_unregister_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let scheme = args
        .first()
        .unwrap_or(&Value::Null)
        .to_php_string()
        .as_bytes()
        .to_vec();
    let scheme_lower: Vec<u8> = scheme.iter().map(|b| b.to_ascii_lowercase()).collect();
    let was_present = USER_WRAPPERS.with(|w| w.borrow_mut().remove(&scheme_lower).is_some());
    if !was_present {
        // PHP also lets you "unregister" a built-in wrapper, which then needs to be
        // restored. For now we report false on unknown schemes.
        let builtin = matches!(
            scheme_lower.as_slice(),
            b"file"
                | b"http"
                | b"https"
                | b"ftp"
                | b"ftps"
                | b"php"
                | b"data"
                | b"compress.zlib"
                | b"compress.bzip2"
                | b"phar"
                | b"glob"
        );
        if !builtin {
            vm.emit_warning(&format!(
                "stream_wrapper_unregister(): Unable to unregister protocol {}://",
                goro_core::string::msg_str(&scheme)
            ));
            return Ok(Value::False);
        }
    }
    Ok(Value::True)
}

fn stream_wrapper_restore_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // No-op for built-ins (we don't allow unregistering them via stream_wrapper_unregister).
    Ok(Value::True)
}

fn stream_get_wrappers_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut arr = PhpArray::new();
    // Built-in wrappers (alphabetical).
    for name in &[
        b"compress.zlib" as &[u8],
        b"data",
        b"file",
        b"glob",
        b"http",
        b"https",
        b"php",
    ] {
        arr.push(Value::String(PhpString::from_bytes(name)));
    }
    // User-registered wrappers.
    USER_WRAPPERS.with(|w| {
        for scheme in w.borrow().keys() {
            arr.push(Value::String(PhpString::from_vec(scheme.clone())));
        }
    });
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

fn stream_get_filters_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut arr = PhpArray::new();
    for name in crate::stream_filters::list_filter_names() {
        arr.push(Value::String(PhpString::from_vec(name)));
    }
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

fn stream_get_transports_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut arr = PhpArray::new();
    for name in &[b"tcp" as &[u8], b"udp", b"unix", b"udg"] {
        arr.push(Value::String(PhpString::from_bytes(name)));
    }
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

/// One `ops->read()` worth of data for record reading: post-filter bytes
/// when read filters are attached. Returns None when no data could be
/// read (EOF, would block, timeout or error).
fn read_some(vm: &mut Vm, fid: i64, max: usize) -> Option<Vec<u8>> {
    if crate::stream_filters::chain_has_direction(fid, crate::stream_filters::STREAM_FILTER_READ) {
        let d = read_filtered(vm, fid, max);
        return if d.is_empty() { None } else { Some(d) };
    }
    // (callers check stream_fatal_error() for filter failures)
    FILE_HANDLES.with(|h| {
        let mut h = h.borrow_mut();
        let fh = h.get_mut(&fid)?;
        let mut buf = vec![0u8; max];
        match fh.read_raw(&mut buf, false) {
            Ok(0) => {
                fh.eof = true;
                None
            }
            Ok(n) => {
                buf.truncate(n);
                Some(buf)
            }
            Err(_) => None,
        }
    })
}

/// stream->fatal_error
fn stream_fatal_error(fid: i64) -> bool {
    FILE_HANDLES.with(|h| {
        h.borrow()
            .get(&fid)
            .map(|fh| fh.fatal_error)
            .unwrap_or(false)
    })
}

/// Put bytes back at the front of the stream's read buffer.
fn unread_bytes(fid: i64, data: &[u8]) {
    if data.is_empty() {
        return;
    }
    FILE_HANDLES.with(|h| {
        if let Some(fh) = h.borrow_mut().get_mut(&fid) {
            let mut v = data.to_vec();
            v.extend_from_slice(&fh.filter_read_ready);
            fh.filter_read_ready = v;
        }
    });
}

/// php_stream_get_record()
fn stream_get_record(vm: &mut Vm, fid: i64, maxlen: usize, delim: &[u8]) -> Option<Vec<u8>> {
    if maxlen == 0 {
        return None;
    }
    // Already buffered data first (php_stream readbuf).
    let mut buf: Vec<u8> = FILE_HANDLES.with(|h| {
        h.borrow_mut()
            .get_mut(&fid)
            .map(|fh| std::mem::take(&mut fh.filter_read_ready))
            .unwrap_or_default()
    });
    // Search for the delimiter in buf[from..] (within the first maxlen bytes).
    let find = |buf: &[u8], from: usize| -> Option<usize> {
        let hay = &buf[..buf.len().min(maxlen)];
        if delim.is_empty() || hay.len() < delim.len() || from >= hay.len() {
            return None;
        }
        hay[from..]
            .windows(delim.len())
            .position(|w| w == delim)
            .map(|p| p + from)
    };
    let mut found = find(&buf, 0);
    while found.is_none() && buf.len() < maxlen {
        let want = (maxlen - buf.len()).min(8192);
        let before = buf.len();
        match read_some(vm, fid, want) {
            Some(d) => buf.extend_from_slice(&d),
            None if stream_fatal_error(fid) => return None,
            None => break,
        }
        found = find(&buf, before.saturating_sub(delim.len().saturating_sub(1)));
    }
    let eof = FILE_HANDLES.with(|h| h.borrow().get(&fid).map(|fh| fh.eof).unwrap_or(true));
    let ret_len = if let Some(p) = found {
        p
    } else if delim.is_empty() && buf.len() >= maxlen {
        maxlen
    } else if buf.len() < maxlen && !eof {
        unread_bytes(fid, &buf);
        return None;
    } else if buf.is_empty() && eof {
        return None;
    } else {
        buf.len().min(maxlen)
    };
    let skip = if found.is_some() { delim.len() } else { 0 };
    unread_bytes(fid, &buf[ret_len + skip..]);
    buf.truncate(ret_len);
    Some(buf)
}

fn stream_get_line_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let arg = args.first().unwrap_or(&Value::Null);
    let fid = require_open_stream(vm, arg, "stream_get_line")?;
    let max_length = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    if max_length < 0 {
        return Err(throw_value_error_msg(
            vm,
            "stream_get_line(): Argument #2 ($length) must be greater than or equal to 0",
        ));
    }
    let max_length = if max_length == 0 {
        8192
    } else {
        max_length as usize
    };
    let ending = args
        .get(2)
        .map(|v| v.to_php_string())
        .unwrap_or_else(|| PhpString::from_bytes(b""));
    if let Some(m) = stream_mode_clean(fid) {
        if matches!(m.as_str(), "w" | "a" | "x" | "c") {
            let line = vm.current_line;
            vm.emit_notice_at(
                "stream_get_line(): Read of 8192 bytes failed with errno=9 Bad file descriptor",
                line,
            );
            return Ok(Value::False);
        }
    }
    Ok(
        match stream_get_record(vm, fid, max_length, ending.as_bytes()) {
            Some(b) => Value::String(PhpString::from_vec(b)),
            None => Value::False,
        },
    )
}

fn stream_set_chunk_size_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let arg = args.first().unwrap_or(&Value::Null);
    let _fid = require_open_stream(vm, arg, "stream_set_chunk_size")?;
    let size = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    if size <= 0 {
        let msg = "stream_set_chunk_size(): Argument #2 ($size) must be greater than 0".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    // We don't currently honour the chunk size internally. Return the default
    // 8192 so callers see a sane previous value.
    Ok(Value::Long(8192))
}
fn stream_socket_enable_crypto_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    xp_socket::stream_socket_enable_crypto(vm, args)
}
fn headers_list_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut arr = PhpArray::new();
    // sapi_cli_header_handler() keeps no header: the CLI's list stays empty.
    if !crate::info::is_web_sapi() {
        return Ok(Value::Array(Rc::new(RefCell::new(arr))));
    }
    SENT_HEADERS.with(|h| {
        for (k, v) in h.borrow().iter() {
            let mut s = Vec::with_capacity(k.len() + 2 + v.len());
            s.extend_from_slice(k);
            s.extend_from_slice(b": ");
            s.extend_from_slice(v);
            arr.push(Value::String(PhpString::from_vec(s)));
        }
    });
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

fn dir_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let path_str = path.to_string_lossy();
    let dir_path = std::path::Path::new(&*path_str);
    if !dir_path.is_dir() {
        vm.emit_warning(&format!(
            "dir({}): Failed to open directory: No such file or directory",
            path_str
        ));
        return Ok(Value::False);
    }
    if std::fs::read_dir(dir_path).is_err() {
        vm.emit_warning(&format!(
            "dir({}): Failed to open directory: Permission denied",
            path_str
        ));
        return Ok(Value::False);
    }
    let did = alloc_dir_handle(path_str.to_string());
    Ok(dir_class::new_directory(
        vm,
        Value::String(path.clone()),
        Value::resource(did),
    ))
}

fn popen_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    vm.flush_cli_output();
    let command = args.first().unwrap_or(&Value::Null).to_php_string();
    let mode = args
        .get(1)
        .map(|v| v.to_php_string())
        .unwrap_or_else(|| PhpString::from_bytes(b"r"));
    let cmd_str = command.to_string_lossy();
    let mode_str = mode.to_string_lossy();
    // Validate mode: PHP accepts only "r", "rb", "w", "wb".
    if !matches!(mode_str.as_str(), "r" | "rb" | "w" | "wb") {
        let msg = "popen(): Argument #2 ($mode) must be one of \"r\", \"rb\", \"w\", or \"wb\""
            .to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    use std::process::{Command, Stdio};
    // A child writing to our stdout: when the script's output is not
    // written straight to it, collect the child's and emit it at pclose().
    let capture_stdout = !mode_str.contains('r') && !vm.cli_direct_output;
    let child = if mode_str.contains('r') {
        Command::new("sh")
            .arg("-c")
            .arg(&*cmd_str)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
    } else {
        let mut c = Command::new("sh");
        c.arg("-c").arg(&*cmd_str).stdin(Stdio::piped());
        if capture_stdout {
            c.stdout(Stdio::piped());
        }
        c.spawn()
    };
    match child {
        Ok(mut child) => {
            let drain = if capture_stdout {
                child.stdout.take()
            } else {
                None
            }
            .map(|mut out| {
                let buf = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
                let b2 = buf.clone();
                let h = std::thread::spawn(move || {
                    use std::io::Read;
                    let mut chunk = [0u8; 8192];
                    loop {
                        match out.read(&mut chunk) {
                            Ok(0) | Err(_) => break,
                            Ok(n) => {
                                if let Ok(mut b) = b2.lock() {
                                    b.extend_from_slice(&chunk[..n]);
                                }
                            }
                        }
                    }
                });
                (h, buf)
            });
            use std::os::unix::io::{FromRawFd, IntoRawFd};
            // php_stream_fopen_from_pipe(): no wrapper, not seekable.
            let pipe_stream = |file: std::fs::File| {
                let fid = alloc_file_handle(file, &mode_str);
                FILE_HANDLES.with(|h| {
                    if let Some(fh) = h.borrow_mut().get_mut(&fid) {
                        fh.wrapper_type = String::new();
                        fh.seekable = false;
                        // php_stream_fopen_from_pipe() starts at position 0.
                        fh.pipe_pos = Some(0);
                    }
                });
                goro_core::value::register_resource(fid, b"stream");
                fid
            };
            let fid = if mode_str.contains('r') {
                child.stdout.take().map(|stdout| {
                    pipe_stream(unsafe { std::fs::File::from_raw_fd(stdout.into_raw_fd()) })
                })
            } else {
                child.stdin.take().map(|stdin| {
                    pipe_stream(unsafe { std::fs::File::from_raw_fd(stdin.into_raw_fd()) })
                })
            };
            match fid {
                Some(fid) => {
                    POPEN_CHILDREN.with(|p| p.borrow_mut().insert(fid, (child, drain)));
                    Ok(Value::resource(fid))
                }
                None => Ok(Value::False),
            }
        }
        Err(_) => Ok(Value::False),
    }
}

/// The output drain of a popen()ed child whose stdout is collected.
type PopenDrain = (
    std::thread::JoinHandle<()>,
    std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
);

thread_local! {
    /// popen() children, by stream id.
    static POPEN_CHILDREN: RefCell<StdHashMap<i64, (std::process::Child, Option<PopenDrain>)>> =
        RefCell::new(StdHashMap::new());
}

fn pclose_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let fid = args.first().unwrap_or(&Value::Null).to_long();
    // Closing our end of the pipe lets the child finish.
    FILE_HANDLES.with(|h| h.borrow_mut().remove(&fid));
    goro_core::value::close_resource(fid);
    let Some((mut child, drain)) = POPEN_CHILDREN.with(|p| p.borrow_mut().remove(&fid)) else {
        return Ok(Value::Long(-1));
    };
    let status = child.wait();
    if let Some((h, buf)) = drain {
        let _ = h.join();
        let out = buf.lock().map(|b| b.clone()).unwrap_or_default();
        vm.write_output(&out);
    }
    // FG(pclose_ret): the exit status.
    Ok(Value::Long(match status {
        Ok(st) => st.code().unwrap_or(-1) as i64,
        Err(_) => -1,
    }))
}

fn rewinddir_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let id = match resolve_dir_handle(vm, args, "rewinddir")? {
        Some(id) => id,
        None => return Ok(Value::Null),
    };
    let path = DIR_HANDLES.with(|dh| dh.borrow().get(&id).map(|h| h.path.clone()));
    if let Some(path) = path {
        if let Ok(names) = read_dir_entries(&path) {
            DIR_HANDLES.with(|dh| {
                if let Some(h) = dh.borrow_mut().get_mut(&id) {
                    h.remaining = names;
                }
            });
        }
    }
    Ok(Value::Null)
}

fn error_log_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let message = args.first().unwrap_or(&Value::Null).to_php_string();
    let message_type = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let destination = args.get(2).map(|v| v.to_php_string());
    // _php_error_log_ex()
    match message_type {
        1 => Ok(Value::False),
        2 => Err(throw_value_error_msg(
            vm,
            "TCP/IP option is not available for error logging",
        )),
        3 => {
            let dest = destination.map(|d| d.to_string_lossy()).unwrap_or_default();
            use std::io::Write;
            match std::fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(&dest)
            {
                Ok(mut f) => {
                    if f.write_all(message.as_bytes()).is_ok() {
                        Ok(Value::True)
                    } else {
                        Ok(Value::False)
                    }
                }
                Err(e) => {
                    let reason = match e.kind() {
                        std::io::ErrorKind::NotFound => "No such file or directory".to_string(),
                        std::io::ErrorKind::PermissionDenied => "Permission denied".to_string(),
                        _ => e.to_string(),
                    };
                    let line = vm.current_line;
                    vm.emit_warning_at(
                        &format!("error_log({}): Failed to open stream: {}", dest, reason),
                        line,
                    );
                    Ok(Value::False)
                }
            }
        }
        4 => {
            use std::io::Write;
            let mut m = message.as_bytes().to_vec();
            m.push(b'\n');
            let _ = std::io::stderr().write_all(&m);
            Ok(Value::True)
        }
        _ => {
            vm.log_err(&message.to_string_lossy());
            Ok(Value::True)
        }
    }
}

fn disk_space_impl(
    vm: &mut Vm,
    args: &[Value],
    fn_name: &str,
    total: bool,
    called_as: &str,
) -> Result<Value, VmError> {
    let path_val = args.first().cloned().unwrap_or(Value::Null);
    let path_str = path_val.to_php_string();
    let path_bytes = path_str.as_bytes();
    if path_bytes.is_empty() {
        let msg = "Path cannot be empty".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    if path_bytes.contains(&0u8) {
        let msg = format!(
            "{}(): Argument #1 ($directory) must not contain any null bytes",
            fn_name
        );
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    let path_string = String::from_utf8_lossy(path_bytes).to_string();
    let path = std::path::Path::new(&path_string);
    if !path.exists() {
        let line = vm.current_line;
        vm.emit_warning_at(&format!("{}(): No such file or directory", called_as), line);
        return Ok(Value::False);
    }
    #[cfg(unix)]
    {
        use std::ffi::CString;
        let c_path = match CString::new(path_bytes) {
            Ok(c) => c,
            Err(_) => return Ok(Value::False),
        };
        let mut buf: libc::statvfs = unsafe { std::mem::zeroed() };
        let r = unsafe { libc::statvfs(c_path.as_ptr(), &mut buf) };
        if r != 0 {
            let line = vm.current_line;
            vm.emit_warning_at(
                &format!("{}(): {}", called_as, std::io::Error::last_os_error()),
                line,
            );
            return Ok(Value::False);
        }
        let result = if total {
            buf.f_blocks as f64 * buf.f_frsize as f64
        } else {
            buf.f_bavail as f64 * buf.f_frsize as f64
        };
        Ok(Value::Double(result))
    }
    #[cfg(not(unix))]
    {
        Ok(Value::Double(if total {
            10737418240.0
        } else {
            1073741824.0
        }))
    }
}
fn disk_free_space_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    disk_space_impl(vm, args, "disk_free_space", false, "disk_free_space")
}
fn diskfreespace_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    disk_space_impl(vm, args, "diskfreespace", false, "diskfreespace")
}
fn disk_total_space_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    disk_space_impl(vm, args, "disk_total_space", true, "disk_total_space")
}
fn get_resources_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let ty = match args.first().map(|v| v.deref()) {
        None | Some(Value::Null) | Some(Value::Undef) => None,
        Some(Value::String(s)) => Some(s.as_bytes().to_vec()),
        Some(v @ (Value::Long(_) | Value::Double(_) | Value::True | Value::False)) => {
            Some(v.to_php_string().as_bytes().to_vec())
        }
        Some(v) => {
            let msg = format!(
                "get_resources(): Argument #1 ($type) must be of type ?string, {} given",
                Vm::value_type_name(&v)
            );
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
    };
    let mut arr = PhpArray::new();
    for (id, t) in goro_core::value::live_resources() {
        let keep = match (&ty, &t) {
            (None, _) => true,
            // "Unknown": the closed ones (type <= 0)
            (Some(want), None) => want.as_slice() == b"Unknown",
            (Some(want), Some(t)) => want == t,
        };
        if keep {
            arr.set(ArrayKey::Int(id), Value::resource(id));
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}
thread_local! {
    /// BG(syslog_device): openlog() keeps the ident pointer.
    static SYSLOG_IDENT: RefCell<Option<std::ffi::CString>> = const { RefCell::new(None) };
}

fn closelog_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    unsafe { libc::closelog() };
    SYSLOG_IDENT.with(|s| *s.borrow_mut() = None);
    Ok(Value::True)
}

fn openlog_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let ident = args
        .first()
        .map(|v| v.to_php_string())
        .unwrap_or_else(PhpString::empty);
    let option = args.get(1).map(|v| v.to_long()).unwrap_or(0) as libc::c_int;
    let facility = args.get(2).map(|v| v.to_long()).unwrap_or(0) as libc::c_int;
    let b = ident.as_bytes();
    let c = std::ffi::CString::new(
        b.iter()
            .take_while(|&&c| c != 0)
            .copied()
            .collect::<Vec<u8>>(),
    )
    .unwrap_or_default();
    SYSLOG_IDENT.with(|s| {
        let mut s = s.borrow_mut();
        *s = Some(c);
        let p = s.as_ref().map_or(std::ptr::null(), |c| c.as_ptr());
        unsafe { libc::openlog(p, option, facility) };
    });
    Ok(Value::True)
}

/// php_syslog_str(): each line is a separate message; bytes are filtered
/// per syslog.filter.
fn syslog_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let priority = args.first().map(|v| v.to_long()).unwrap_or(0) as libc::c_int;
    let message = args
        .get(1)
        .map(|v| v.to_php_string())
        .unwrap_or_else(PhpString::empty);
    let filter = vm
        .constants
        .get(b"syslog.filter".as_slice())
        .map(|v| v.to_php_string().to_string_lossy().to_ascii_lowercase())
        .unwrap_or_else(|| "no-ctrl".to_string());
    let send = |buf: &[u8]| {
        let c = std::ffi::CString::new(
            buf.iter()
                .take_while(|&&c| c != 0)
                .copied()
                .collect::<Vec<u8>>(),
        )
        .unwrap_or_default();
        unsafe { libc::syslog(priority, c"%s".as_ptr(), c.as_ptr()) };
    };
    if filter == "raw" {
        send(message.as_bytes());
        return Ok(Value::True);
    }
    let mut sbuf: Vec<u8> = Vec::new();
    for &c in message.as_bytes() {
        if (0x20..=0x7e).contains(&c) || (c >= 0x80 && filter != "ascii") {
            sbuf.push(c);
        } else if c == b'\n' {
            send(&sbuf);
            sbuf.clear();
        } else if c < 0x20 && filter == "all" {
            sbuf.push(c);
        } else {
            const X: &[u8; 16] = b"0123456789abcdef";
            sbuf.extend_from_slice(&[b'\\', b'x', X[(c >> 4) as usize], X[(c & 15) as usize]]);
        }
    }
    send(&sbuf);
    Ok(Value::True)
}

fn fsockopen_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    xp_socket::fsockopen(vm, args, false)
}

fn pfsockopen_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    xp_socket::fsockopen(vm, args, true)
}

fn register_tick_function_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(cb) = args.first() {
        validate_callback_arg(vm, "register_tick_function", cb)?;
        vm.tick_functions
            .push((cb.deref(), args[1..].to_vec(), false));
    }
    Ok(Value::True)
}
fn unregister_tick_function_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(cb) = args.first() {
        validate_callback_arg(vm, "unregister_tick_function", cb)?;
        // zend_llist_del_element(): the first matching entry
        if let Some(i) = vm
            .tick_functions
            .iter()
            .position(|e| goro_core::errdisplay::callables_equal(&e.0, cb))
        {
            if vm.tick_functions[i].2 {
                let msg =
                    "Registered tick function cannot be unregistered while it is being executed";
                let exc = vm.create_exception(b"Error", msg, vm.current_line);
                vm.current_exception = Some(exc);
                return Err(VmError {
                    message: format!("Uncaught Error: {}", msg),
                    line: vm.current_line,
                });
            }
            vm.tick_functions.remove(i);
        }
    }
    Ok(Value::Null)
}

fn stream_isatty_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // In CLI mode, always return false (not a TTY)
    Ok(Value::False)
}

thread_local! {
    /// ps_title: the title set by cli_set_process_title()
    static PROCESS_TITLE: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
}

/// set_ps_title(): overwrite the argv area (what ps shows) with `title`.
fn set_ps_title(title: &[u8]) -> bool {
    let Ok(stat) = std::fs::read_to_string("/proc/self/stat") else {
        return false;
    };
    // fields after the ")" of the comm field; arg_start/arg_end are fields 48/49
    let Some(rest) = stat.rfind(')').map(|p| &stat[p + 2..]) else {
        return false;
    };
    let f: Vec<&str> = rest.split_whitespace().collect();
    let (Some(start), Some(end)) = (
        f.get(45).and_then(|s| s.parse::<u64>().ok()),
        f.get(46).and_then(|s| s.parse::<u64>().ok()),
    ) else {
        return false;
    };
    if end <= start {
        return false;
    }
    let len = (end - start) as usize;
    let mut buf = vec![0u8; len];
    let n = title.len().min(len.saturating_sub(1));
    buf[..n].copy_from_slice(&title[..n]);
    use std::io::{Seek, SeekFrom, Write};
    let Ok(mut mem) = std::fs::OpenOptions::new()
        .write(true)
        .open("/proc/self/mem")
    else {
        return false;
    };
    mem.seek(SeekFrom::Start(start)).is_ok() && mem.write_all(&buf).is_ok()
}

fn cli_set_process_title_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let title = args
        .first()
        .map(|v| v.to_php_string().as_bytes().to_vec())
        .unwrap_or_default();
    if set_ps_title(&title) {
        PROCESS_TITLE.with(|t| *t.borrow_mut() = Some(title));
        Ok(Value::True)
    } else {
        vm.emit_warning("cli_set_process_title(): cli_set_process_title had an error: Not initialized correctly");
        Ok(Value::False)
    }
}

fn cli_get_process_title_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let t = PROCESS_TITLE
        .with(|t| t.borrow().clone())
        .unwrap_or_default();
    Ok(Value::String(PhpString::from_vec(t)))
}

fn ini_parse_quantity_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let (v, err) = goro_core::ini_quantity::parse_quantity(s.as_bytes(), true);
    if let Some(e) = err {
        vm.emit_warning(&e);
    }
    Ok(Value::Long(v as i64))
}

fn dl_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let filename = args.first().unwrap_or(&Value::Null).to_php_string();
    // PG(enable_dl) defaults to on
    let enabled = vm
        .constants
        .get(b"enable_dl".as_slice())
        .map(ini_parse_bool)
        .unwrap_or(true);
    let line = vm.current_line;
    if !enabled {
        vm.emit_warning_at("dl(): Dynamically loaded extensions aren't enabled", line);
        return Ok(Value::False);
    }
    const MAXPATHLEN: usize = 4096;
    if filename.len() >= MAXPATHLEN {
        vm.emit_warning_at(
            &format!(
                "dl(): Filename exceeds the maximum allowed length of {} characters",
                MAXPATHLEN
            ),
            line,
        );
        return Ok(Value::False);
    }
    // php_load_extension() with MODULE_TEMPORARY
    let name = filename.to_string_lossy();
    if name.contains('/') {
        vm.emit_warning_at(
            "dl(): Temporary module name should contain only filename",
            line,
        );
        return Ok(Value::False);
    }
    let dir = vm
        .constants
        .get(b"extension_dir".as_slice())
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_default();
    // A module goro has built in but loads on request: found in
    // extension_dir as the file name or as the extension name.
    if !dir.is_empty() {
        let sep = if dir.ends_with('/') { "" } else { "/" };
        for f in [name.to_string(), format!("{}.so", name)] {
            if !std::path::Path::new(&format!("{}{}{}", dir, sep, f)).is_file() {
                continue;
            }
            let stem = f.strip_prefix("php_").unwrap_or(&f);
            let stem = stem.split('.').next().unwrap_or(stem).to_ascii_lowercase();
            let Some(&(module, minit)) = vm
                .loadable_modules
                .iter()
                .find(|(m, _)| *m == stem.as_bytes())
            else {
                break;
            };
            if vm.loaded_extensions.contains(module) {
                // zend_error(E_CORE_WARNING)
                vm.display_error(
                    "Warning",
                    &format!("Module \"{}\" is already loaded", stem),
                    "Unknown",
                    0,
                );
                return Ok(Value::False);
            }
            minit(vm);
            return Ok(Value::True);
        }
    }
    // goro cannot load other shared extensions.
    let tried = |f: &str| {
        let p = format!("{}/{}", dir, f);
        let err = if std::path::Path::new(&p).exists() {
            "invalid ELF header"
        } else {
            "cannot open shared object file: No such file or directory"
        };
        format!("{} ({}: {})", p, p, err)
    };
    vm.emit_warning_at(
        &format!(
            "dl(): Unable to load dynamic library '{}' (tried: {}, {})",
            name,
            tried(&name),
            tried(&format!("{}.so", name))
        ),
        line,
    );
    Ok(Value::False)
}

fn header_register_callback_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let callback = args.first().cloned().unwrap_or(Value::Null);
    // Z_PARAM_FUNC
    let (ok, _, err) = check_callable(vm, &callback, false);
    if vm.current_exception.is_some() {
        return Err(VmError {
            message: "Uncaught exception".into(),
            line: vm.current_line,
        });
    }
    if !ok {
        return Err(throw_type_error_msg(
            vm,
            format!(
                "header_register_callback(): Argument #1 ($callback) must be a valid callback, {}",
                err.unwrap_or_default()
            ),
        ));
    }
    // A callback registered after the headers were sent is never used.
    vm.header_callback = if vm.headers_sent {
        None
    } else {
        Some(callback)
    };
    Ok(Value::True)
}

fn token_get_all_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let source = args.first().unwrap_or(&Value::Null).to_php_string();
    let flags = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    match goro_core::tokenizer::token_get_all(vm, source.as_bytes(), flags) {
        Some(v) => Ok(v),
        None => Err(VmError {
            message: vm.uncaught_exception_message(),
            line: vm.current_line,
        }),
    }
}

fn token_name_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        return Ok(Value::String(PhpString::from_bytes(b"UNKNOWN")));
    }
    let id = args[0].to_long();
    let name = goro_parser::token::token_id_to_name(id).unwrap_or("UNKNOWN");
    Ok(Value::String(PhpString::from_bytes(name.as_bytes())))
}

fn request_parse_body_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let value_error = |vm: &mut Vm, msg: &str| -> VmError {
        let exc = vm.create_exception(b"ValueError", msg, vm.current_line);
        vm.current_exception = Some(exc);
        VmError {
            message: format!("Uncaught ValueError: {}", msg),
            line: vm.current_line,
        }
    };
    // cache_request_parse_body_options()
    let mut opts = crate::request::ParseBodyOptions::default();
    if let Some(Value::Array(arr)) = args.first().map(|v| v.deref()) {
        let pairs: Vec<(ArrayKey, Value)> = arr
            .borrow()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for (k, v) in pairs {
            let key = match k {
                ArrayKey::Int(_) => {
                    return Err(value_error(vm, "Invalid integer key in $options argument"));
                }
                ArrayKey::String(s) => s,
            };
            if key.is_empty() {
                return Err(value_error(
                    vm,
                    "Invalid empty string key in $options argument",
                ));
            }
            let lower = key.as_bytes().to_ascii_lowercase();
            let slot = match lower.as_slice() {
                b"max_file_uploads" => &mut opts.max_file_uploads,
                b"max_input_vars" => &mut opts.max_input_vars,
                b"max_multipart_body_parts" => &mut opts.max_multipart_body_parts,
                b"post_max_size" => &mut opts.post_max_size,
                b"upload_max_filesize" => &mut opts.upload_max_filesize,
                _ => {
                    return Err(value_error(
                        vm,
                        &format!(
                            "Invalid key \"{}\" in $options argument",
                            key.to_msg_string()
                        ),
                    ));
                }
            };
            *slot = Some(match v.deref() {
                Value::String(s) => {
                    let (n, err) = goro_core::ini_quantity::parse_quantity(s.as_bytes(), true);
                    if let Some(e) = err {
                        vm.emit_warning(&e);
                    }
                    n as i64
                }
                Value::Long(n) => n,
                other => {
                    let tn = match other {
                        Value::Null | Value::Undef => "null".to_string(),
                        Value::True => "true".to_string(),
                        Value::False => "false".to_string(),
                        o => Vm::value_type_name(&o),
                    };
                    return Err(value_error(
                        vm,
                        &format!("Invalid {} value in $options argument", tn),
                    ));
                }
            });
        }
    }
    match crate::request::request_parse_body(vm, &opts) {
        Ok((post, files)) => {
            let mut pair = PhpArray::new();
            pair.push(Value::Array(post));
            pair.push(Value::Array(files));
            Ok(Value::Array(Rc::new(RefCell::new(pair))))
        }
        Err(msg) => {
            let exc = vm.create_exception(b"RequestParseBodyException", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            Err(VmError {
                message: format!("Uncaught RequestParseBodyException: {}", msg),
                line: vm.current_line,
            })
        }
    }
}

fn ignore_user_abort_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let old = vm
        .constants
        .get(b"ignore_user_abort".as_slice())
        .map(ini_parse_bool)
        .unwrap_or(false);
    if let Some(v) = args.first() {
        if !matches!(v, Value::Null) {
            let new = if v.is_truthy() { "1" } else { "0" };
            ini_remember(vm, b"ignore_user_abort");
            vm.constants.insert(
                b"ignore_user_abort".to_vec(),
                Value::String(PhpString::from_bytes(new.as_bytes())),
            );
        }
    }
    Ok(Value::Long(old as i64))
}

fn connection_aborted_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(0))
}

fn connection_status_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(0))
}

fn time_nanosleep_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let seconds = args.first().map(|v| v.to_long()).unwrap_or(0);
    let nanoseconds = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    if seconds < 0 {
        return Err(throw_value_error_in(
            vm,
            "time_nanosleep",
            args,
            "time_nanosleep(): Argument #1 ($seconds) must be greater than or equal to 0",
        ));
    }
    if nanoseconds < 0 {
        return Err(throw_value_error_in(
            vm,
            "time_nanosleep",
            args,
            "time_nanosleep(): Argument #2 ($nanoseconds) must be greater than or equal to 0",
        ));
    }
    // nanosleep() fails with EINVAL
    if nanoseconds > 999_999_999 {
        return Err(throw_value_error_in(
            vm,
            "time_nanosleep",
            args,
            "Nanoseconds was not in the range 0 to 999 999 999 or seconds was negative",
        ));
    }
    let req = libc::timespec {
        tv_sec: seconds as libc::time_t,
        tv_nsec: nanoseconds as libc::c_long,
    };
    let mut rem = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    if unsafe { libc::nanosleep(&req, &mut rem) } != 0 {
        if std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
            // Interrupted by a signal: what is left.
            let mut a = PhpArray::new();
            a.set(
                ArrayKey::String(PhpString::from_bytes(b"seconds")),
                Value::Long(rem.tv_sec),
            );
            a.set(
                ArrayKey::String(PhpString::from_bytes(b"nanoseconds")),
                Value::Long(rem.tv_nsec),
            );
            return Ok(Value::Array(Rc::new(RefCell::new(a))));
        }
        return Ok(Value::False);
    }
    Ok(Value::True)
}

fn time_sleep_until_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    const NS_PER_SEC: u64 = 1_000_000_000;
    let top = (u64::MAX / NS_PER_SEC) as f64;
    let target = args.first().map(|v| v.to_double()).unwrap_or(0.0);
    if !(target >= 0.0 && target <= top) {
        return Err(throw_value_error_in(
            vm,
            "time_sleep_until",
            args,
            &format!(
                "time_sleep_until(): Argument #1 ($timestamp) must be between 0 and {}",
                u64::MAX / NS_PER_SEC
            ),
        ));
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    // gettimeofday() resolution
    let current_ns = now.as_secs() * NS_PER_SEC + u64::from(now.subsec_micros()) * 1000;
    let target_ns = (target * NS_PER_SEC as f64) as u64;
    if target_ns < current_ns {
        let line = vm.current_line;
        vm.emit_warning_at(
            "time_sleep_until(): Argument #1 ($timestamp) must be greater than or equal to the current time",
            line,
        );
        return Ok(Value::False);
    }
    std::thread::sleep(std::time::Duration::from_nanos(target_ns - current_ns));
    Ok(Value::True)
}

fn set_file_buffer_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(0)) // success
}

fn stream_resolve_include_path_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let filename = args
        .first()
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_default();
    if filename.is_empty() {
        return Ok(Value::False);
    }
    // Absolute paths resolve directly.
    let p = std::path::Path::new(&*filename);
    if p.is_absolute() {
        return Ok(match std::fs::canonicalize(p) {
            Ok(cp) => Value::String(PhpString::from_string(cp.to_string_lossy().into_owned())),
            Err(_) => Value::False,
        });
    }
    // Walk the include_path. set_include_path stores into vm.constants[b"include_path"].
    let include_path = vm
        .constants
        .get(b"include_path".as_ref())
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_else(|| ".".to_string());
    let sep = if cfg!(target_os = "windows") {
        ';'
    } else {
        ':'
    };
    for entry in include_path.split(sep) {
        if entry.is_empty() {
            continue;
        }
        let candidate = if entry == "." {
            std::path::PathBuf::from(&*filename)
        } else {
            std::path::PathBuf::from(entry).join(&*filename)
        };
        if candidate.exists() {
            return Ok(match std::fs::canonicalize(&candidate) {
                Ok(cp) => Value::String(PhpString::from_string(cp.to_string_lossy().into_owned())),
                Err(_) => Value::String(PhpString::from_string(
                    candidate.to_string_lossy().into_owned(),
                )),
            });
        }
    }
    Ok(Value::False)
}

fn stream_supports_lock_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let arg = args.first().unwrap_or(&Value::Null);
    let fid = require_open_stream(vm, arg, "stream_supports_lock")?;
    // PHP_STREAM_OPTION_LOCKING: plain files (and descriptor streams);
    // memory/temp and data: streams have no lock support.
    let supports = FILE_HANDLES.with(|h| {
        h.borrow().get(&fid).is_some_and(|fh| {
            fh.user_wrapper.is_none()
                && fh.data_meta.is_none()
                && !matches!(fh.special, SpecialStream::Memory | SpecialStream::Temp)
        })
    });
    Ok(if supports { Value::True } else { Value::False })
}
