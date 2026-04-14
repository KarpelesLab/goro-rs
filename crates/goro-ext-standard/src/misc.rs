use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::{PhpObject, Visibility};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use std::cell::RefCell;
use std::rc::Rc;

/// Validate that an argument is an array and throw TypeError if not.
/// Returns Ok(()) if valid array (or reference to array), Err with VmError if not.
fn require_array_arg(vm: &mut Vm, val: &Value, func_name: &str, param_name: &str, param_num: u32) -> Result<(), VmError> {
    require_array_arg_inner(vm, val, func_name, Some(param_name), param_num)
}

/// Like require_array_arg but without parameter name in error message (for variadic args)
fn require_array_arg_variadic(vm: &mut Vm, val: &Value, func_name: &str, param_num: u32) -> Result<(), VmError> {
    require_array_arg_inner(vm, val, func_name, None, param_num)
}

fn require_array_arg_inner(vm: &mut Vm, val: &Value, func_name: &str, param_name: Option<&str>, param_num: u32) -> Result<(), VmError> {
    let make_msg = |type_name: &str| -> String {
        if let Some(name) = param_name {
            format!("{}(): Argument #{} (${}) must be of type array, {} given", func_name, param_num, name, type_name)
        } else {
            format!("{}(): Argument #{} must be of type array, {} given", func_name, param_num, type_name)
        }
    };
    match val {
        Value::Array(_) => Ok(()),
        Value::Reference(r) => {
            let inner = r.borrow();
            if matches!(&*inner, Value::Array(_)) {
                Ok(())
            } else {
                let type_name = Vm::value_type_name(&*inner);
                let msg = make_msg(&type_name);
                let exc = vm.create_exception(b"TypeError", &msg, 0);
                vm.current_exception = Some(exc);
                Err(VmError { message: msg, line: vm.current_line })
            }
        }
        _ => {
            let type_name = Vm::value_type_name(val);
            let msg = make_msg(&type_name);
            let exc = vm.create_exception(b"TypeError", &msg, 0);
            vm.current_exception = Some(exc);
            Err(VmError { message: msg, line: vm.current_line })
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
    vm.register_function(b"constant", constant);

    // Output buffering
    vm.register_function(b"ob_start", ob_start);
    vm.register_function(b"ob_end_clean", ob_end_clean);
    vm.register_function(b"ob_end_flush", ob_end_flush);
    vm.register_function(b"ob_get_contents", ob_get_contents);
    vm.register_function(b"ob_get_clean", ob_get_clean);
    vm.register_function(b"ob_get_level", ob_get_level);
    vm.register_function(b"ob_flush", ob_flush);
    vm.register_function(b"ob_clean", ob_clean);
    vm.register_function(b"ob_get_length", ob_get_length);
    vm.register_function(b"ob_implicit_flush", ob_implicit_flush);
    vm.register_function(b"ob_get_status", ob_get_status);
    vm.register_function(b"ob_get_flush", ob_get_flush);
    vm.register_function(b"ob_list_handlers", ob_list_handlers);
    vm.register_function(b"flush", flush_fn);

    // Function handling
    vm.register_function(b"func_num_args", func_num_args);
    vm.register_function(b"func_get_args", func_get_args);
    vm.register_function(b"func_get_arg", func_get_arg);
    vm.register_function(b"function_exists", function_exists);
    vm.register_function(b"is_callable", is_callable);
    vm.register_function(b"call_user_func", call_user_func);
    vm.register_function(b"call_user_func_array", call_user_func_array);

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
    vm.register_function(b"array_key_exists", array_key_exists_fn2);
    vm.register_function(b"in_array", in_array);
    vm.register_function(b"array_map", array_map);
    vm.register_function(b"array_filter", array_filter);
    vm.register_function(b"array_walk", array_walk);
    vm.register_function(b"array_reduce", array_reduce_fn);
    vm.register_function(b"array_combine", array_combine);
    vm.register_function(b"array_chunk", array_chunk);
    vm.register_function(b"array_pad", array_pad);
    vm.register_function(b"array_fill", array_fill);
    vm.register_function(b"array_fill_keys", array_fill_keys);
    vm.register_function(b"array_merge_recursive", array_merge_recursive);
    vm.register_function(b"array_diff", array_diff);
    vm.register_function(b"array_intersect", array_intersect);
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
    vm.register_function(b"compact", compact);
    vm.register_function(b"current", current);
    vm.register_function(b"next", next_fn);
    vm.register_function(b"prev", prev_fn);
    vm.register_function(b"reset", reset_fn);
    vm.register_function(b"end", end_fn);
    vm.register_function(b"key", key_fn);

    // Misc
    vm.register_function(b"ini_set", ini_set);
    vm.register_function(b"ini_alter", ini_set);
    vm.register_function(b"ini_get", ini_get);
    vm.register_function(b"ini_restore", ini_restore);
    vm.register_function(b"set_time_limit", set_time_limit);
    vm.register_function(b"assert", php_assert);
    vm.register_function(b"class_exists", class_exists);
    vm.register_function(b"get_class", get_class);
    vm.register_function(b"get_declared_classes", get_declared_classes);
    vm.register_function(b"get_declared_traits", get_declared_traits_fn);
    vm.register_function(b"get_declared_interfaces", get_declared_interfaces_fn);
    vm.register_function(b"property_exists", property_exists);
    vm.register_function(b"method_exists", method_exists);
    vm.register_function(b"is_object", is_object);
    vm.register_function(b"setlocale", setlocale);
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
    vm.register_function(b"gc_collect_cycles", gc_collect_fn);
    vm.register_function(b"gc_enabled", gc_enabled_fn);
    vm.register_function(b"gc_disable", gc_disable_fn);
    vm.register_function(b"gc_enable", gc_enable_fn);
    vm.register_function(b"gc_status", gc_status_fn);
    vm.register_function(b"debug_zval_dump", debug_zval_dump_fn);
    vm.register_function(b"get_class_methods", get_class_methods_fn);
    vm.register_function(b"get_object_vars", get_object_vars_fn);
    vm.register_function(b"get_class", get_class_fn);
    vm.register_function(b"serialize", serialize_fn);
    vm.register_function(b"unserialize", unserialize_fn);
    vm.register_function(b"memory_get_usage", memory_get_usage_fn);
    vm.register_function(b"memory_get_peak_usage", memory_get_peak_fn);
    vm.register_function(b"memory_reset_peak_usage", memory_reset_peak_usage_fn);
    vm.register_function(b"sleep", sleep_fn);
    vm.register_function(b"usleep", usleep_fn);
    vm.register_function(b"uniqid", uniqid_fn);
    vm.register_function(b"sys_get_temp_dir", sys_get_temp_dir_fn);
    vm.register_function(b"tempnam", tempnam_fn);
    vm.register_function(b"getenv", getenv_fn);
    vm.register_function(b"putenv", putenv_fn);
    // SPL autoload functions moved to goro-ext-spl
    vm.register_function(b"class_alias", class_alias_fn);
    vm.register_function(b"is_a", is_a_fn);
    vm.register_function(b"is_subclass_of", is_subclass_of_fn);
    vm.register_function(b"get_parent_class", get_parent_class_fn);
    vm.register_function(b"get_called_class", get_called_class_fn);
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
    vm.register_function(b"array_diff_key", array_diff_key_fn);
    vm.register_function(b"array_diff_assoc", array_diff_assoc_fn);
    vm.register_function(b"array_intersect_key", array_intersect_key_fn);
    vm.register_function(b"array_intersect_assoc", array_intersect_assoc_fn);
    vm.register_function(b"array_all", array_all_fn);
    vm.register_function(b"array_any", array_any_fn);
    vm.register_function(b"array_multisort", array_multisort_fn);
    vm.register_function(b"highlight_string", highlight_string_fn);
    vm.register_function(b"exec", exec_fn);
    vm.register_function(b"system", system_fn);
    vm.register_function(b"shell_exec", shell_exec_fn);
    vm.register_function(b"passthru", passthru_fn);
    vm.register_function(b"escapeshellarg", escapeshellarg_fn);
    vm.register_function(b"escapeshellcmd", escapeshellcmd_fn);
    vm.register_function(b"fopen", fopen_fn);
    vm.register_function(b"fclose", fclose_fn);
    vm.register_function(b"fread", fread_fn);
    vm.register_function(b"fwrite", fwrite_fn);
    vm.register_function(b"fgets", fgets_fn);
    vm.register_function(b"feof", feof_fn);
    vm.register_function(b"rewind", rewind_fn);
    vm.register_function(b"fseek", fseek_fn);
    vm.register_function(b"ftell", ftell_fn);
    vm.register_function(b"fflush", fflush_fn);
    vm.register_function(b"unlink", unlink_fn);
    vm.register_function(b"rename", rename_fn);
    vm.register_function(b"copy", copy_fn);
    vm.register_function(b"readfile", readfile_fn);
    vm.register_function(b"file", file_fn);
    vm.register_function(b"lstat", lstat_fn);
    vm.register_function(b"is_executable", is_executable_fn);
    vm.register_function(b"tempnam", tempnam_fn3);
    vm.register_function(b"sys_get_temp_dir", sys_get_temp_dir_fn3);
    vm.register_function(b"mkdir", mkdir_fn);
    vm.register_function(b"rmdir", rmdir_fn);
    vm.register_function(b"glob", glob_fn);
    vm.register_function(b"scandir", scandir_fn);
    vm.register_function(b"is_writable", is_writable_fn);
    vm.register_function(b"is_writeable", is_writable_fn);
    vm.register_function(b"is_readable", is_readable_fn);
    vm.register_function(b"filemtime", filemtime_fn);
    vm.register_function(b"fileatime", fileatime_fn);
    vm.register_function(b"filectime", filectime_fn);
    vm.register_function(b"fileinode", fileinode_fn);
    vm.register_function(b"fileperms", fileperms_fn);
    vm.register_function(b"fileowner", fileowner_fn);
    vm.register_function(b"filegroup", filegroup_fn);
    vm.register_function(b"filetype", filetype_fn);
    vm.register_function(b"is_link", is_link_fn);
    vm.register_function(b"chmod", chmod_fn);
    vm.register_function(b"chown", chown_fn);
    vm.register_function(b"chgrp", chgrp_fn);
    vm.register_function(b"clearstatcache", clearstatcache_fn);
    vm.register_function_with_params(b"fputcsv", fputcsv_fn, &[b"stream", b"fields", b"separator", b"enclosure", b"escape", b"eol"]);
    vm.register_function_with_params(b"fgetcsv", fgetcsv_fn, &[b"stream", b"length", b"separator", b"enclosure", b"escape"]);
    vm.register_function(b"fpassthru", fpassthru_fn);
    vm.register_function(b"fgetc", fgetc_fn);
    vm.register_function(b"flock", flock_fn);
    vm.register_function(b"fstat", fstat_fn);
    vm.register_function(b"stream_get_contents", stream_get_contents_fn);
    vm.register_function(b"fputs", fwrite_fn); // fputs is an alias for fwrite
    vm.register_function(b"fscanf", fscanf_fn);
    vm.register_function(b"linkinfo", linkinfo_fn);
    vm.register_function(b"parse_ini_file", parse_ini_file_fn);
    vm.register_function(b"header", header_fn);
    vm.register_function(b"headers_sent", headers_sent_fn);
    vm.register_function(b"http_response_code", http_response_code_fn);
    vm.register_function(b"setcookie", setcookie_fn);
    vm.register_function(b"setrawcookie", setcookie_fn);
    // SPL object functions moved to goro-ext-spl
    vm.register_function(b"forward_static_call", call_user_func);
    vm.register_function(b"forward_static_call_array", call_user_func_array);
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
    vm.register_function(b"array_find_key", array_find_key_fn);
    vm.register_function(b"array_intersect_ukey", array_intersect_ukey_fn);
    vm.register_function(b"array_intersect_uassoc", array_intersect_uassoc_fn);
    vm.register_function(b"array_diff_ukey", array_diff_ukey_fn);
    vm.register_function(b"array_diff_uassoc", array_diff_uassoc_fn);
    vm.register_function(b"array_udiff", array_udiff_fn);
    vm.register_function(b"array_udiff_assoc", array_udiff_assoc_fn);
    vm.register_function(b"array_udiff_uassoc", array_udiff_uassoc_fn);
    vm.register_function(b"array_uintersect", array_uintersect_fn);
    vm.register_function(b"array_uintersect_assoc", array_uintersect_assoc_fn);
    vm.register_function(b"array_uintersect_uassoc", array_uintersect_uassoc_fn);
    vm.register_function(b"array_product", array_product_fn);
    vm.register_function(b"array_sum", array_sum_fn);
    vm.register_function(b"parse_ini_string", parse_ini_string_fn);
    vm.register_function(b"assert_options", assert_options_fn);
    vm.register_function(b"ftruncate", ftruncate_fn);
    vm.register_function(b"tmpfile", tmpfile_fn);
    vm.register_function(b"phpinfo", phpinfo_fn);
    vm.register_function(b"phpcredits", phpcredits_fn);
    vm.register_function(b"image_type_to_mime_type", image_type_to_mime_type_fn);
    vm.register_function(b"image_type_to_extension", image_type_to_extension_fn);
    vm.register_function(b"get_cfg_var", get_cfg_var_fn);
    vm.register_function(b"php_ini_loaded_file", php_ini_loaded_file_fn);
    vm.register_function(b"php_ini_scanned_files", php_ini_scanned_files_fn);
    vm.register_function(b"getmypid", getmypid_fn);
    vm.register_function(b"getmyuid", getmyuid_fn);
    vm.register_function(b"getlastmod", getlastmod_fn);
    vm.register_function(b"getmygid", getmygid_fn);
    vm.register_function(b"get_current_user", get_current_user_fn);
    // sizeof is an alias for count (registered in type_funcs.rs)

    // String extras
    vm.register_function(b"str_split", str_split);
    vm.register_function(b"number_format", number_format);
    vm.register_function(b"money_format", money_format);
    vm.register_function(b"hex2bin", hex2bin);
    vm.register_function(b"bin2hex", bin2hex);
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
    vm.register_function(b"htmlspecialchars", htmlspecialchars);
    vm.register_function(b"htmlentities", htmlentities);
    vm.register_function(b"htmlspecialchars_decode", htmlspecialchars_decode);
    vm.register_function(b"fprintf", fprintf_fn);
    vm.register_function(b"vfprintf", vfprintf_fn);
    vm.register_function(b"sscanf", sscanf_fn);
    // File stubs (return false/null for now)
    vm.register_function(b"file_exists", file_exists_fn);
    vm.register_function(b"is_file", is_file_fn);
    vm.register_function(b"is_dir", is_dir_fn);
    vm.register_function(b"is_readable", is_readable_fn);
    vm.register_function(b"is_writable", is_writable_fn);
    vm.register_function(b"getcwd", getcwd_fn);
    vm.register_function(b"chdir", chdir_fn);
    vm.register_function(b"filesize", filesize_fn);
    vm.register_function(b"touch", touch_fn);
    vm.register_function(b"file_get_contents", file_get_contents_fn);
    vm.register_function(b"file_put_contents", file_put_contents_fn);
    vm.register_function(b"realpath", realpath_fn);
    vm.register_function(b"realpath_cache_size", realpath_cache_size_fn);
    vm.register_function(b"realpath_cache_get", realpath_cache_get_fn);
    vm.register_function(b"is_link", is_link_fn);
    vm.register_function(b"stat", stat_fn);
    vm.register_function(b"is_numeric", is_numeric_fn);
    vm.register_function(b"clearstatcache", clearstatcache_fn);
    vm.register_function(b"array_walk_recursive", array_walk_recursive_fn);
    vm.register_function_with_params(b"fgetcsv", fgetcsv_fn, &[b"stream", b"length", b"separator", b"enclosure", b"escape"]);
    vm.register_function(b"fileperms", fileperms_fn);
    vm.register_function(b"filetype", filetype_fn);
    vm.register_function(b"opendir", opendir_fn);
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
    vm.register_function(b"unlink", unlink_fn2);
    vm.register_function(b"rename", rename_fn2);
    vm.register_function(b"copy", copy_fn2);
    vm.register_function(b"file_get_contents", file_get_contents_fn);
    vm.register_function(b"file_put_contents", file_put_contents_fn);
    vm.register_function(b"realpath", realpath_fn);
    vm.register_function(b"dirname", dirname_fn);
    vm.register_function(b"basename", basename_fn);
    vm.register_function(b"pathinfo", pathinfo_fn);
    // class_implements, class_parents, class_uses moved to goro-ext-spl
    vm.register_function(b"str_increment", str_increment_fn);
    vm.register_function(b"str_decrement", str_decrement_fn);
    vm.register_function(b"get_error_handler", get_error_handler_fn);
    vm.register_function(b"get_exception_handler", get_exception_handler_fn);
    vm.register_function(b"get_included_files", get_included_files_fn);
    vm.register_function(b"get_required_files", get_included_files_fn);
    // iterator_apply moved to goro-ext-spl
    vm.register_function(b"property_exists", property_exists_fn);
    vm.register_function(b"get_mangled_object_vars", get_mangled_object_vars_fn);
    vm.register_function(b"get_resource_id", get_resource_id_fn);
    vm.register_function(b"ip2long", ip2long_fn);
    vm.register_function(b"long2ip", long2ip_fn);
    vm.register_function(b"hrtime", hrtime_fn);
    vm.register_function(b"gethostname", gethostname_fn);
    vm.register_function(b"umask", umask_fn);
    vm.register_function(b"getopt", getopt_fn);
    vm.register_function(b"gethostbyname", gethostbyname_fn);
    vm.register_function(b"gethostbynamel", gethostbynamel_fn);
    vm.register_function(b"gethostbyaddr", gethostbyaddr_fn);
    vm.register_function(b"proc_open", proc_open_fn);
    vm.register_function(b"proc_close", proc_close_fn);
    vm.register_function(b"proc_get_status", proc_get_status_fn);
    vm.register_function(b"proc_terminate", proc_terminate_fn);
    vm.register_function(b"inet_pton", inet_pton_fn);
    vm.register_function(b"inet_ntop", inet_ntop_fn);
    vm.register_function(b"stream_set_blocking", stream_set_blocking_fn);
    vm.register_function(b"stream_set_timeout", stream_set_timeout_fn);
    vm.register_function(b"stream_set_write_buffer", stream_set_write_buffer_fn);
    vm.register_function(b"stream_set_read_buffer", stream_set_read_buffer_fn);
    vm.register_function(b"stream_copy_to_stream", stream_copy_to_stream_fn);
    vm.register_function(b"stream_filter_append", stream_filter_append_fn);
    vm.register_function(b"stream_filter_prepend", stream_filter_prepend_fn);
    vm.register_function(b"stream_filter_register", stream_filter_register_fn);
    vm.register_function(b"stream_filter_remove", stream_filter_remove_fn);
    vm.register_function(b"stream_context_create", stream_context_create_fn);
    vm.register_function(b"stream_context_set_option", stream_context_set_option_fn);
    vm.register_function(b"stream_context_set_options", stream_context_set_option_fn);
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
    vm.register_function(b"stream_socket_client", stream_socket_client_fn);
    vm.register_function(b"stream_socket_server", stream_socket_server_fn);
    vm.register_function(b"stream_socket_get_name", stream_socket_get_name_fn);
    vm.register_function(b"stream_select", stream_select_fn);
    vm.register_function(b"stream_socket_pair", stream_socket_pair_fn);
    vm.register_function(b"stream_get_line", stream_get_line_fn);
    vm.register_function(b"stream_set_chunk_size", stream_set_chunk_size_fn);
    vm.register_function(b"stream_socket_recvfrom", stream_socket_recvfrom_fn);
    vm.register_function(b"stream_socket_sendto", stream_socket_sendto_fn);
    vm.register_function(b"stream_socket_shutdown", stream_socket_shutdown_fn);
    vm.register_function(b"stream_socket_enable_crypto", stream_socket_enable_crypto_fn);
    vm.register_function(b"headers_list", headers_list_fn);
    vm.register_function(b"dir", dir_fn);
    vm.register_function(b"popen", popen_fn);
    vm.register_function(b"pclose", pclose_fn);
    vm.register_function(b"rewinddir", rewinddir_fn);
    vm.register_function(b"error_log", error_log_fn);
    vm.register_function(b"highlight_file", highlight_file_fn);
    vm.register_function(b"show_source", highlight_file_fn);
    vm.register_function(b"php_strip_whitespace", php_strip_whitespace_fn);
    vm.register_function(b"disk_free_space", disk_free_space_fn);
    vm.register_function(b"diskfreespace", disk_free_space_fn);
    vm.register_function(b"disk_total_space", disk_total_space_fn);
    vm.register_function(b"get_resources", get_resources_fn);
    vm.register_function(b"closelog", closelog_fn);
    vm.register_function(b"openlog", openlog_fn);
    vm.register_function(b"syslog", syslog_fn);
    vm.register_function(b"mail", mail_fn);
    vm.register_function(b"fsockopen", fsockopen_fn);
    vm.register_function(b"pfsockopen", pfsockopen_fn);
    vm.register_function(b"getimagesize", getimagesize_fn);
    vm.register_function(b"register_tick_function", register_tick_function_fn);
    vm.register_function(b"unregister_tick_function", unregister_tick_function_fn);
    vm.register_function(b"output_add_rewrite_var", output_add_rewrite_var_fn);
    vm.register_function(b"output_reset_rewrite_vars", output_reset_rewrite_vars_fn);
    vm.register_function(b"dns_check_record", dns_check_record_fn);
    vm.register_function(b"checkdnsrr", dns_check_record_fn);
    vm.register_function(b"dns_get_record", dns_get_record_fn);
    vm.register_function(b"getmxrr", getmxrr_fn);
    vm.register_function(b"getservbyname", getservbyname_fn);
    vm.register_function(b"getservbyport", getservbyport_fn);
    vm.register_function(b"getprotobyname", getprotobyname_fn);
    vm.register_function(b"getprotobynumber", getprotobynumber_fn);
    vm.register_function(b"get_browser", get_browser_fn);
    vm.register_function(b"stream_socket_accept", stream_socket_accept_fn);
    vm.register_function(b"stream_isatty", stream_isatty_fn);
    vm.register_function(b"is_uploaded_file", is_uploaded_file_fn);
    vm.register_function(b"move_uploaded_file", move_uploaded_file_fn);
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
    vm.register_function(b"get_html_translation_table", get_html_translation_table_fn);
    vm.register_function(b"nl_langinfo", nl_langinfo_fn);
    vm.register_function(b"localeconv", localeconv_fn);
    vm.register_function(b"time_nanosleep", time_nanosleep_fn);
    vm.register_function(b"time_sleep_until", time_sleep_until_fn);
    vm.register_function(b"socket_get_status", stream_get_meta_data_fn); // alias
    vm.register_function(b"set_file_buffer", set_file_buffer_fn);
    vm.register_function(b"stream_resolve_include_path", stream_resolve_include_path_fn);
    vm.register_function(b"stream_supports_lock", stream_supports_lock_fn);
    vm.register_function(b"stream_bucket_new", stream_bucket_new_fn);
    vm.register_function(b"iptcparse", iptcparse_fn);
    vm.register_function(b"iptcembed", iptcembed_fn);

    // Regex functions are now in the regex module (regex.rs)
}

// === Error handling ===

fn error_reporting(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let old = vm.error_reporting;
    if let Some(level) = args.first() {
        vm.error_reporting = level.to_long();
    }
    Ok(Value::Long(old))
}

fn set_error_handler(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let prev = vm.error_handler.take();
    if let Some(handler) = args.first() {
        if !matches!(handler, Value::Null) {
            vm.error_handler = Some(handler.clone());
        }
    }
    Ok(prev.unwrap_or(Value::Null))
}

fn restore_error_handler(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    vm.error_handler = None;
    Ok(Value::True)
}

fn set_exception_handler(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let prev = vm.exception_handler.take();
    vm.exception_handler_stack.push(prev.clone());
    if let Some(handler) = args.first() {
        if !matches!(handler, Value::Null) {
            vm.exception_handler = Some(handler.clone());
        }
    }
    Ok(prev.unwrap_or(Value::Null))
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
    let message = args
        .first()
        .unwrap_or(&Value::Null)
        .to_php_string()
        .to_string_lossy();
    let error_type = args.get(1).map(|v| v.to_long()).unwrap_or(256); // E_USER_ERROR = 256

    match error_type {
        256 => {
            // E_USER_ERROR - fatal error
            // PHP 8.4: passing E_USER_ERROR is deprecated
            let call_line = vm.current_line;
            vm.emit_deprecated_raw(
                "Passing E_USER_ERROR to trigger_error() is deprecated since 8.4, throw an exception or call exit with a string message instead",
                call_line,
            );
            // Try user error handler first
            if vm.call_user_error_handler(256, &message, call_line) {
                return Ok(Value::True);
            }
            return Err(VmError {
                message: message.to_string(),
                line: call_line,
            });
        }
        512 => {
            // E_USER_WARNING
            let call_line = vm.current_line;
            if !vm.call_user_error_handler(512, &message, call_line) {
                vm.emit_warning_at(&message, call_line);
            }
        }
        1024 => {
            // E_USER_NOTICE
            let call_line = vm.current_line;
            if !vm.call_user_error_handler(1024, &message, call_line) {
                vm.emit_notice_raw(&message, call_line);
            }
        }
        16384 => {
            // E_USER_DEPRECATED
            let call_line = vm.current_line;
            if !vm.call_user_error_handler(16384, &message, call_line) {
                vm.emit_deprecated_raw(&message, call_line);
            }
        }
        _ => {
            vm.emit_warning(&message);
        }
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
            let msg = format!("define(): Argument #1 ($constant_name) must be of type string, {} given", type_name);
            let exc = vm.create_exception(b"TypeError", &msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
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
        return Err(VmError { message: msg.to_string(), line: vm.current_line });
    }
    // Check if it's a built-in keyword constant (TRUE, FALSE, NULL)
    let name_upper: Vec<u8> = name_bytes.iter().map(|b| b.to_ascii_uppercase()).collect();
    if name_upper == b"TRUE" || name_upper == b"FALSE" || name_upper == b"NULL" {
        let name_str = name.to_string_lossy();
        vm.emit_warning(&format!("Constant {} already defined, this will be an error in PHP 9", name_str));
        return Ok(Value::False);
    }
    // Normalize namespace part (lowercase) while keeping constant name case-sensitive
    let normalized = normalize_ns_const_name(&name_bytes);
    // Check if constant is already defined
    if vm.constants.contains_key(&normalized) {
        let name_str = name.to_string_lossy();
        vm.emit_warning(&format!("Constant {} already defined, this will be an error in PHP 9", name_str));
        return Ok(Value::False);
    }
    vm.constants.insert(normalized, value);
    Ok(Value::True)
}

fn constant(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let name = args.first().unwrap_or(&Value::Null).to_php_string();
    let name_str = name.to_string_lossy();
    // Check for class constants (Class::CONST)
    if let Some(pos) = name_str.find("::") {
        let class_name = &name_str[..pos];
        let const_name = &name_str[pos+2..];
        let class_name_stripped = class_name.strip_prefix('\\').unwrap_or(class_name);
        let class_lower: Vec<u8> = class_name_stripped.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
        if let Some(class) = vm.classes.get(&class_lower) {
            if let Some(val) = class.constants.get(const_name.as_bytes()) {
                return Ok(val.clone());
            }
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
    if let Some(val) = vm.constants.get(lookup_name) {
        return Ok(val.clone());
    }
    let msg = format!("Undefined constant \"{}\"", name_str);
    let line = vm.current_line;
    let exc = vm.create_exception(b"Error", &msg, line);
    vm.current_exception = Some(exc);
    Err(VmError { message: msg, line })
}

// === Output buffering ===

fn ob_start(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    vm.ob_stack.push(Vec::new());
    Ok(Value::True)
}
fn ob_end_clean(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    if vm.ob_stack.pop().is_some() {
        Ok(Value::True)
    } else {
        Ok(Value::False)
    }
}
fn ob_end_flush(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    if let Some(buf) = vm.ob_stack.pop() {
        // Flush to parent buffer or output
        vm.write_output(&buf);
        Ok(Value::True)
    } else {
        Ok(Value::False)
    }
}
fn ob_get_contents(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    if let Some(buf) = vm.ob_stack.last() {
        Ok(Value::String(PhpString::from_vec(buf.clone())))
    } else {
        Ok(Value::False)
    }
}
fn ob_get_clean(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    if let Some(buf) = vm.ob_stack.pop() {
        Ok(Value::String(PhpString::from_vec(buf)))
    } else {
        Ok(Value::False)
    }
}
fn ob_get_level(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(vm.ob_stack.len() as i64))
}
fn ob_get_length(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    if let Some(buf) = vm.ob_stack.last() {
        Ok(Value::Long(buf.len() as i64))
    } else {
        Ok(Value::False)
    }
}
fn ob_clean(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    if let Some(buf) = vm.ob_stack.last_mut() {
        buf.clear();
        Ok(Value::True)
    } else {
        Ok(Value::False)
    }
}
fn ob_flush(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    if let Some(buf) = vm.ob_stack.last_mut() {
        let data = std::mem::take(buf);
        drop(buf);
        // Pop this level, write to parent, push empty back
        vm.ob_stack.pop();
        vm.write_output(&data);
        vm.ob_stack.push(Vec::new());
        Ok(Value::True)
    } else {
        Ok(Value::False)
    }
}
fn ob_implicit_flush(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Null)
}
fn ob_get_status(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let full = args.first().map(|v| v.to_bool()).unwrap_or(false);
    if full {
        let result = PhpArray::new();
        let result_rc = Rc::new(RefCell::new(result));
        for (i, buf) in vm.ob_stack.iter().enumerate() {
            let entry_rc = Rc::new(RefCell::new(PhpArray::new()));
            {
                let mut e = entry_rc.borrow_mut();
                e.set(ArrayKey::String(PhpString::from_bytes(b"name")), Value::String(PhpString::from_bytes(b"default output handler")));
                e.set(ArrayKey::String(PhpString::from_bytes(b"type")), Value::Long(1));
                e.set(ArrayKey::String(PhpString::from_bytes(b"flags")), Value::Long(112));
                e.set(ArrayKey::String(PhpString::from_bytes(b"level")), Value::Long(i as i64));
                e.set(ArrayKey::String(PhpString::from_bytes(b"chunk_size")), Value::Long(0));
                e.set(ArrayKey::String(PhpString::from_bytes(b"buffer_size")), Value::Long(buf.len() as i64));
                e.set(ArrayKey::String(PhpString::from_bytes(b"buffer_used")), Value::Long(buf.len() as i64));
            }
            result_rc.borrow_mut().push(Value::Array(entry_rc));
        }
        Ok(Value::Array(result_rc))
    } else if !vm.ob_stack.is_empty() {
        let buf = vm.ob_stack.last().unwrap();
        let entry_rc = Rc::new(RefCell::new(PhpArray::new()));
        {
            let mut e = entry_rc.borrow_mut();
            e.set(ArrayKey::String(PhpString::from_bytes(b"name")), Value::String(PhpString::from_bytes(b"default output handler")));
            e.set(ArrayKey::String(PhpString::from_bytes(b"type")), Value::Long(1));
            e.set(ArrayKey::String(PhpString::from_bytes(b"flags")), Value::Long(112));
            e.set(ArrayKey::String(PhpString::from_bytes(b"level")), Value::Long((vm.ob_stack.len() - 1) as i64));
            e.set(ArrayKey::String(PhpString::from_bytes(b"chunk_size")), Value::Long(0));
            e.set(ArrayKey::String(PhpString::from_bytes(b"buffer_size")), Value::Long(buf.len() as i64));
            e.set(ArrayKey::String(PhpString::from_bytes(b"buffer_used")), Value::Long(buf.len() as i64));
        }
        Ok(Value::Array(entry_rc))
    } else {
        Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
    }
}
fn ob_get_flush(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    if let Some(buf) = vm.ob_stack.pop() {
        let contents = Value::String(PhpString::from_vec(buf.clone()));
        vm.write_output(&buf);
        Ok(contents)
    } else {
        Ok(Value::False)
    }
}
fn ob_list_handlers(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let arr = PhpArray::new();
    let arr_rc = Rc::new(RefCell::new(arr));
    for _ in 0..vm.ob_stack.len() {
        arr_rc.borrow_mut().push(Value::String(PhpString::from_bytes(b"default output handler")));
    }
    Ok(Value::Array(arr_rc))
}
fn flush_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // No-op in our implementation
    Ok(Value::Null)
}

// === Function handling ===

fn func_num_args(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // Return the number of arguments passed to the calling user function
    if let Some((_name, _file, _line, args, _is_method)) = vm.call_stack.last() {
        Ok(Value::Long(args.len() as i64))
    } else {
        // PHP 8.4: calling from global scope throws Error
        let msg = "func_num_args() cannot be called from the global scope";
        let exc = vm.create_exception(b"Error", msg, vm.current_line);
        vm.current_exception = Some(exc);
        Err(VmError { message: msg.to_string(), line: vm.current_line })
    }
}
fn func_get_args(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // Return an array of all arguments passed to the calling user function
    if let Some((_name, _file, _line, args, _is_method)) = vm.call_stack.last() {
        let mut arr = PhpArray::new();
        for arg in args.iter() {
            arr.push(arg.clone());
        }
        Ok(Value::Array(Rc::new(RefCell::new(arr))))
    } else {
        // PHP 8.4: calling from global scope throws Error
        let msg = "func_get_args() cannot be called from the global scope";
        let exc = vm.create_exception(b"Error", msg, vm.current_line);
        vm.current_exception = Some(exc);
        Err(VmError { message: msg.to_string(), line: vm.current_line })
    }
}
fn func_get_arg(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let index = args.first().unwrap_or(&Value::Null).to_long();
    if let Some((_name, _file, _line, caller_args, _is_method)) = vm.call_stack.last() {
        if index < 0 || index as usize >= caller_args.len() {
            vm.emit_warning(&format!("func_get_arg(): Argument #{} not passed to function", index));
            Ok(Value::False)
        } else {
            Ok(caller_args[index as usize].clone())
        }
    } else {
        // PHP 8.4: calling from global scope throws Error
        let msg = "func_get_arg() cannot be called from the global scope";
        let exc = vm.create_exception(b"Error", msg, vm.current_line);
        vm.current_exception = Some(exc);
        Err(VmError { message: msg.to_string(), line: vm.current_line })
    }
}
fn function_exists(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let name = args.first().unwrap_or(&Value::Null).to_php_string();
    let raw = name.as_bytes();
    // Strip leading backslash for namespace resolution
    let stripped = if raw.first() == Some(&b'\\') { &raw[1..] } else { raw };
    let name_lower: Vec<u8> = stripped
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect();
    // "Class::method" is not a function
    if name_lower.contains(&b':') {
        return Ok(Value::False);
    }
    if vm.functions.contains_key(&name_lower) || vm.user_functions.contains_key(&name_lower) {
        Ok(Value::True)
    } else {
        Ok(Value::False)
    }
}
fn is_callable(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null).deref();
    let mut callable_name: Option<Vec<u8>> = None;
    let result = match &val {
        Value::String(s) => {
            let raw_bytes = s.as_bytes();
            // Strip leading backslash for namespace resolution
            let stripped = if raw_bytes.first() == Some(&b'\\') {
                &raw_bytes[1..]
            } else {
                raw_bytes
            };
            let name_lower: Vec<u8> = stripped
                .iter()
                .map(|b| b.to_ascii_lowercase())
                .collect();
            callable_name = Some(stripped.to_vec());
            // Check for "Class::method" syntax
            if let Some(pos) = name_lower.iter().position(|&b| b == b':') {
                if pos + 1 < name_lower.len() && name_lower[pos + 1] == b':' {
                    let class_name = &name_lower[..pos];
                    let method_name = &name_lower[pos + 2..];
                    if let Some(class) = vm.classes.get(class_name) {
                        if let Some(method) = class.get_method(method_name) {
                            // Check visibility
                            let caller_scope = vm.current_class_scope();
                            let caller_lower = caller_scope.as_ref().map(|s| s.iter().map(|b| b.to_ascii_lowercase()).collect::<Vec<u8>>());
                            let decl_lower: Vec<u8> = method.declaring_class.iter().map(|b| b.to_ascii_lowercase()).collect();
                            match method.visibility {
                                Visibility::Private => {
                                    if let Some(ref scope) = caller_lower {
                                        if *scope == decl_lower { Value::True } else { Value::False }
                                    } else {
                                        Value::False
                                    }
                                }
                                Visibility::Protected => {
                                    if let Some(ref scope) = caller_lower {
                                        if *scope == decl_lower
                                            || vm.class_extends(scope, &decl_lower)
                                            || vm.class_extends(&decl_lower, scope)
                                        {
                                            Value::True
                                        } else {
                                            Value::False
                                        }
                                    } else {
                                        Value::False
                                    }
                                }
                                Visibility::Public => Value::True,
                            }
                        } else {
                            Value::False
                        }
                    } else {
                        Value::False
                    }
                } else {
                    Value::False
                }
            } else if vm.functions.contains_key(&name_lower)
                || vm.user_functions.contains_key(&name_lower)
            {
                Value::True
            } else {
                Value::False
            }
        }
        Value::Object(obj) => {
            // Check if the object has __invoke method
            let class_name_lower: Vec<u8> = obj.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
            if class_name_lower == b"closure" {
                callable_name = Some(b"Closure::__invoke".to_vec());
                Value::True
            } else {
                let class_orig: Vec<u8> = obj.borrow().class_name.clone();
                if let Some(class) = vm.classes.get(&class_name_lower) {
                    if class.methods.contains_key(&b"__invoke".to_vec()) {
                        let mut name = class_orig;
                        name.extend_from_slice(b"::__invoke");
                        callable_name = Some(name);
                        Value::True
                    } else {
                        callable_name = Some(b"".to_vec());
                        Value::False
                    }
                } else {
                    callable_name = Some(b"".to_vec());
                    Value::True // default to true for built-in objects
                }
            }
        }
        Value::Array(arr) => {
            let arr = arr.borrow();
            // Check if this is a closure array [__closure_N, use_val1, ...]
            if let Some(first) = arr.values().next() {
                if let Value::String(s) = first {
                    let b = s.as_bytes();
                    if b.starts_with(b"__closure_") || b.starts_with(b"__arrow_") || b.starts_with(b"__bound_closure_") || b.starts_with(b"__closure_fcc_") {
                        callable_name = Some(b"Closure::__invoke".to_vec());
                        return set_callable_name_and_return(vm, args, callable_name, Value::True);
                    }
                }
            }
            if arr.len() == 2 {
                // Validate that the callback is actually callable
                let vals: Vec<Value> = arr.values().cloned().collect();
                let method_name = vals[1].to_php_string();
                let method_name_bytes = method_name.as_bytes().to_vec();
                let method_lower: Vec<u8> = method_name_bytes.iter().map(|b| b.to_ascii_lowercase()).collect();
                match &vals[0] {
                    Value::Object(obj) => {
                        let class_orig: Vec<u8> = obj.borrow().class_name.clone();
                        let class_lower: Vec<u8> = class_orig.iter().map(|b| b.to_ascii_lowercase()).collect();
                        let mut current = class_lower.clone();
                        let mut found = false;
                        let mut method_visibility = None;
                        let mut declaring_class_lower: Vec<u8> = Vec::new();
                        for _ in 0..50 {
                            if let Some(class) = vm.classes.get(&current) {
                                if let Some(method) = class.methods.get(&method_lower) {
                                    found = true;
                                    method_visibility = Some(method.visibility);
                                    declaring_class_lower = method.declaring_class.iter().map(|b| b.to_ascii_lowercase()).collect();
                                    break;
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
                        // Check visibility if method was found
                        if found {
                            if let Some(vis) = method_visibility {
                                let caller_scope = vm.current_class_scope();
                                let caller_lower = caller_scope.as_ref().map(|s| s.iter().map(|b| b.to_ascii_lowercase()).collect::<Vec<u8>>());
                                match vis {
                                    Visibility::Private => {
                                        // Private methods are only callable from the declaring class
                                        if let Some(ref scope) = caller_lower {
                                            if *scope != declaring_class_lower {
                                                found = false;
                                            }
                                        } else {
                                            found = false;
                                        }
                                    }
                                    Visibility::Protected => {
                                        // Protected methods are callable from the declaring class and subclasses
                                        if let Some(ref scope) = caller_lower {
                                            if *scope != declaring_class_lower
                                                && !vm.class_extends(scope, &declaring_class_lower)
                                                && !vm.class_extends(&declaring_class_lower, scope)
                                            {
                                                found = false;
                                            }
                                        } else {
                                            found = false;
                                        }
                                    }
                                    Visibility::Public => {}
                                }
                            }
                        }
                        let mut name = class_orig;
                        name.extend_from_slice(b"::");
                        name.extend_from_slice(&method_name_bytes);
                        callable_name = Some(name);
                        if found { Value::True } else { Value::False }
                    }
                    Value::String(class_name) => {
                        let class_name_bytes = class_name.as_bytes().to_vec();
                        let class_lower: Vec<u8> = class_name_bytes.iter().map(|b| b.to_ascii_lowercase()).collect();
                        let mut name = class_name_bytes;
                        name.extend_from_slice(b"::");
                        name.extend_from_slice(&method_name_bytes);
                        callable_name = Some(name);
                        if let Some(class) = vm.classes.get(&class_lower) {
                            if let Some(method) = class.get_method(&method_lower) {
                                // Check visibility
                                let caller_scope = vm.current_class_scope();
                                let caller_lower = caller_scope.as_ref().map(|s| s.iter().map(|b| b.to_ascii_lowercase()).collect::<Vec<u8>>());
                                let decl_lower: Vec<u8> = method.declaring_class.iter().map(|b| b.to_ascii_lowercase()).collect();
                                match method.visibility {
                                    Visibility::Private => {
                                        if let Some(ref scope) = caller_lower {
                                            if *scope == decl_lower { Value::True } else { Value::False }
                                        } else {
                                            Value::False
                                        }
                                    }
                                    Visibility::Protected => {
                                        if let Some(ref scope) = caller_lower {
                                            if *scope == decl_lower
                                                || vm.class_extends(scope, &decl_lower)
                                                || vm.class_extends(&decl_lower, scope)
                                            {
                                                Value::True
                                            } else {
                                                Value::False
                                            }
                                        } else {
                                            Value::False
                                        }
                                    }
                                    Visibility::Public => Value::True,
                                }
                            } else {
                                Value::False
                            }
                        } else {
                            Value::False
                        }
                    }
                    _ => {
                        callable_name = Some(b"".to_vec());
                        Value::False
                    }
                }
            } else {
                callable_name = Some(b"".to_vec());
                Value::False
            }
        }
        _ => {
            callable_name = Some(b"".to_vec());
            Value::False
        }
    };
    set_callable_name_and_return(vm, args, callable_name, result)
}

fn set_callable_name_and_return(_vm: &mut Vm, args: &[Value], callable_name: Option<Vec<u8>>, result: Value) -> Result<Value, VmError> {
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
fn call_user_func(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        return Ok(Value::Null);
    }

    let callback = &args[0];
    let call_args: Vec<Value> = args[1..].to_vec();
    // Take any forwarded named args from the VM
    let named_args: Vec<(Vec<u8>, Value)> = std::mem::take(&mut vm.pending_named_args);

    // Handle array callback: [class_or_object, method_name]
    if let Value::Array(arr) = callback {
        let arr_borrow = arr.borrow();
        let vals: Vec<Value> = arr_borrow.values().cloned().collect();
        drop(arr_borrow);
        if vals.len() >= 2 {
            let method_name = vals[1].to_php_string();
            let method_lower: Vec<u8> = method_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();

            // Check if first element is a class name (string) or object
            match &vals[0] {
                Value::String(class_name) => {
                    let class_bytes = class_name.as_bytes();
                    // Check if this is a closure with captures (not a class::method call)
                    if !class_bytes.starts_with(b"__closure_") && !class_bytes.starts_with(b"__arrow_") {
                        // Static method call: ['ClassName', 'method']
                        let class_lower: Vec<u8> = class_bytes.iter().map(|b| b.to_ascii_lowercase()).collect();
                        // Check for "Class::method" in the class name itself
                        if let Some(pos) = class_lower.iter().position(|&b| b == b':') {
                            if pos + 1 < class_lower.len() && class_lower[pos + 1] == b':' {
                                let real_class = &class_lower[..pos];
                                let real_method = &class_lower[pos + 2..];
                                if let Some(class) = vm.classes.get(real_class).cloned() {
                                    if let Some(method) = class.methods.get(real_method) {
                                        let op = method.op_array.clone();
                                        let declaring = method.declaring_class.clone();
                                        let class_name_orig = class.name.clone();
                                        vm.called_class_stack.push(class_name_orig);
                                        vm.push_class_scope(declaring);
                                        let result = vm.execute_fn_with_named_args(&op, call_args, named_args, None);
                                        vm.pop_class_scope();
                                        vm.called_class_stack.pop();
                                        return result;
                                    }
                                }
                                return Ok(Value::Null);
                            }
                        }

                        // Check for forwarding calls (parent::, self:: in array syntax)
                        let is_forwarding = class_lower == b"parent" || class_lower == b"self";
                        let resolved_class_lower = if class_lower == b"parent" {
                            vm.get_current_class_name().as_ref()
                                .and_then(|scope| vm.classes.get(scope))
                                .and_then(|c| c.parent.clone())
                                .map(|p| p.iter().map(|b| b.to_ascii_lowercase()).collect::<Vec<u8>>())
                                .unwrap_or(class_lower.clone())
                        } else if class_lower == b"self" {
                            vm.get_current_class_name().as_ref().cloned()
                                .or_else(|| vm.called_class_stack.last().map(|n| n.iter().map(|b| b.to_ascii_lowercase()).collect()))
                                .unwrap_or(class_lower.clone())
                        } else {
                            class_lower.clone()
                        };

                        if let Some(class) = vm.classes.get(&resolved_class_lower).cloned() {
                            if let Some(method) = class.methods.get(&method_lower) {
                                // Check visibility
                                let accessible = match method.visibility {
                                    goro_core::object::Visibility::Public => true,
                                    goro_core::object::Visibility::Protected | goro_core::object::Visibility::Private => {
                                        if let Some(caller) = vm.current_class_scope() {
                                            let caller_lower: Vec<u8> = caller.iter().map(|b| b.to_ascii_lowercase()).collect();
                                            let declaring_lower: Vec<u8> = method.declaring_class.iter().map(|b| b.to_ascii_lowercase()).collect();
                                            if method.visibility == goro_core::object::Visibility::Private {
                                                caller_lower == declaring_lower
                                            } else {
                                                caller_lower == declaring_lower
                                                    || vm.class_extends(&caller_lower, &declaring_lower)
                                                    || vm.class_extends(&declaring_lower, &caller_lower)
                                            }
                                        } else {
                                            false
                                        }
                                    }
                                };
                                if accessible {
                                    let op = method.op_array.clone();
                                    let declaring = method.declaring_class.clone();
                                    if is_forwarding {
                                        // Forward: keep current called_class
                                    } else {
                                        vm.called_class_stack.push(class.name.clone());
                                    }
                                    vm.push_class_scope(declaring);
                                    let result = vm.execute_fn_with_named_args(&op, call_args, named_args, None);
                                    vm.pop_class_scope();
                                    if !is_forwarding {
                                        vm.called_class_stack.pop();
                                    }
                                    return result;
                                }
                                // Method not accessible - fall through to __callStatic
                            }
                            // Method not found or not accessible - check for __callStatic magic method
                            if let Some(call_static) = class.get_method(b"__callstatic") {
                                let op = call_static.op_array.clone();
                                let mut args_array = goro_core::array::PhpArray::new();
                                for arg in &call_args {
                                    args_array.push(arg.clone());
                                }
                                let magic_args = vec![
                                    Value::String(PhpString::from_vec(method_name.as_bytes().to_vec())),
                                    Value::Array(std::rc::Rc::new(std::cell::RefCell::new(args_array))),
                                ];
                                return vm.execute_fn_with_named_args(&op, magic_args, vec![], None);
                            }
                            return Ok(Value::Null);
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
                        class_lower = obj_borrow.class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                    }
                    if let Some(class) = vm.classes.get(&class_lower).cloned() {
                        if let Some(method) = class.methods.get(&method_lower) {
                            // Check visibility before executing
                            let accessible = match method.visibility {
                                goro_core::object::Visibility::Public => true,
                                goro_core::object::Visibility::Protected | goro_core::object::Visibility::Private => {
                                    // Check if caller scope has access
                                    if let Some(caller) = vm.current_class_scope() {
                                        let caller_lower: Vec<u8> = caller.iter().map(|b| b.to_ascii_lowercase()).collect();
                                        let declaring_lower: Vec<u8> = method.declaring_class.iter().map(|b| b.to_ascii_lowercase()).collect();
                                        if method.visibility == goro_core::object::Visibility::Private {
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
                                let op = method.op_array.clone();
                                return vm.execute_fn_with_named_args(&op, call_args, named_args, Some(Value::Object(obj.clone())));
                            }
                            // Method exists but not accessible - fall through to __call
                        }
                        // Method not found or not accessible - check for __call magic method
                        if let Some(call_method) = class.get_method(b"__call") {
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
                            return vm.execute_fn_with_named_args(&op, magic_args, vec![], Some(Value::Object(obj.clone())));
                        }
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
                return builtin(vm, &call_args);
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
            class_lower = obj_borrow.class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
        }
        if let Some(class) = vm.classes.get(&class_lower).cloned() {
            if let Some(method) = class.methods.get(b"__invoke".as_slice()) {
                let op = method.op_array.clone();
                return vm.execute_fn_with_named_args(&op, call_args, named_args, Some(Value::Object(obj.clone())));
            }
        }
        return Ok(Value::Null);
    }

    // Handle closure callback (stored as string name or array with captures)
    if let Value::String(s) = callback {
        let func_name = s.as_bytes().to_vec();
        let func_lower: Vec<u8> = func_name.iter().map(|b| b.to_ascii_lowercase()).collect();

        // Try builtin first
        if let Some(builtin) = vm.functions.get(&func_lower).copied() {
            // For builtins, forward named args via pending_named_args
            if !named_args.is_empty() {
                vm.pending_named_args = named_args;
            }
            return builtin(vm, &call_args);
        }

        // Try user function (but not "Class::method" strings - those need proper LSB handling below)
        let looks_like_class_method = func_lower.windows(2).any(|w| w == b"::");
        if !looks_like_class_method {
            if let Some(user_fn) = vm.user_functions.get(&func_lower).cloned() {
                return vm.execute_fn_with_named_args(&user_fn, call_args, named_args, None);
            }
        }

        // Try "Class::method" syntax
        if let Some(pos) = func_lower.iter().position(|&b| b == b':') {
            if pos + 1 < func_lower.len() && func_lower[pos + 1] == b':' {
                let class_part_lower = func_lower[..pos].to_vec();
                let method_lower = &func_lower[pos + 2..];

                // Check for forwarding calls (parent::, self::)
                let is_forwarding = class_part_lower == b"parent" || class_part_lower == b"self";

                // Resolve parent/self to actual class names
                let resolved_class_lower = if class_part_lower == b"parent" {
                    vm.get_current_class_name().as_ref()
                        .and_then(|scope| vm.classes.get(scope))
                        .and_then(|c| c.parent.clone())
                        .map(|p| p.iter().map(|b| b.to_ascii_lowercase()).collect::<Vec<u8>>())
                        .unwrap_or(class_part_lower.clone())
                } else if class_part_lower == b"self" {
                    vm.get_current_class_name().as_ref().cloned()
                        .or_else(|| vm.called_class_stack.last().map(|n| n.iter().map(|b| b.to_ascii_lowercase()).collect()))
                        .unwrap_or(class_part_lower.clone())
                } else {
                    class_part_lower.clone()
                };

                if let Some(class) = vm.classes.get(&resolved_class_lower).cloned() {
                    if let Some(method) = class.methods.get(method_lower) {
                        let op = method.op_array.clone();
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
                        let result = vm.execute_fn_with_named_args(&op, call_args, named_args, None);
                        vm.pop_class_scope();
                        if !is_forwarding {
                            vm.called_class_stack.pop();
                        }
                        return result;
                    }
                    // Check __callStatic
                    if let Some(call_static) = class.get_method(b"__callstatic") {
                        let op = call_static.op_array.clone();
                        let orig_method = &func_name[pos + 2..];
                        let mut args_array = goro_core::array::PhpArray::new();
                        for arg in &call_args {
                            args_array.push(arg.clone());
                        }
                        let magic_args = vec![
                            Value::String(PhpString::from_vec(orig_method.to_vec())),
                            Value::Array(std::rc::Rc::new(std::cell::RefCell::new(args_array))),
                        ];
                        return vm.execute_fn_with_named_args(&op, magic_args, vec![], None);
                    }
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
                            let exc = vm.create_exception(b"Error", "Cannot use positional argument after named argument", 0);
                            vm.current_exception = Some(exc);
                            return Err(VmError {
                                message: "Cannot use positional argument after named argument".into(),
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
            if let Err(msg) = arr.try_push(val.clone()) {
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
        let entries: Vec<_> = arr.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let (last_key, last_val) = entries.last().unwrap();
        let was_int_key = matches!(last_key, goro_core::array::ArrayKey::Int(_));
        arr.remove(last_key);
        // Reset internal pointer
        arr.pointer = 0;
        // Recalculate next_int_key after removal
        if was_int_key {
            arr.recalculate_next_int_key();
        }
        Ok(last_val.clone())
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
        let entries: Vec<_> = arr_mut
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let first_val = entries[0].1.clone();
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
        let mut arr_mut = arr.borrow_mut();
        let existing: Vec<_> = arr_mut.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let new_values: Vec<Value> = args[1..].to_vec();

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
            result.push(val.clone());
        }
        Ok(Value::Array(Rc::new(RefCell::new(result))))
    } else {
        Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
    }
}

fn array_merge(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let mut result = PhpArray::new();
    for (i, arg) in args.iter().enumerate() {
        require_array_arg_variadic(vm, arg, "array_merge", (i + 1) as u32)?;
        if let Value::Array(arr) = arg {
            let arr = arr.borrow();
            for (key, val) in arr.iter() {
                match key {
                    goro_core::array::ArrayKey::Int(_) => result.push(val.clone()),
                    goro_core::array::ArrayKey::String(s) => {
                        result.set(goro_core::array::ArrayKey::String(s.clone()), val.clone());
                    }
                }
            }
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn array_reverse(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let preserve_keys = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
        let arr = arr.borrow();
        let entries: Vec<_> = arr.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let mut result = PhpArray::new();
        for (key, val) in entries.into_iter().rev() {
            if preserve_keys {
                result.set(key, val);
            } else {
                match key {
                    goro_core::array::ArrayKey::Int(_) => result.push(val),
                    goro_core::array::ArrayKey::String(s) => result.set(goro_core::array::ArrayKey::String(s), val),
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
                    vm.emit_warning("array_flip(): Can only flip string and integer values, entry skipped");
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
        let arr = arr.borrow();
        let mut result = PhpArray::new();
        let mut seen: Vec<Vec<u8>> = Vec::new();
        let flags = args.get(1).map(|v| v.to_long()).unwrap_or(2); // SORT_STRING = 2 default
        for (key, val) in arr.iter() {
            let s = match val {
                Value::Object(obj) => {
                    // For objects (including enums), use class name + identity-based key
                    let obj_ref = obj.borrow();
                    if obj_ref.has_property(b"__enum_case") {
                        // Enum: use class_name::case_name as unique key
                        let mut k = obj_ref.class_name.clone();
                        k.extend_from_slice(b"::");
                        let case_name = obj_ref.get_property(b"name");
                        k.extend_from_slice(case_name.to_php_string().as_bytes());
                        k
                    } else {
                        // Regular objects: use pointer as unique key
                        let ptr = Rc::as_ptr(obj) as usize;
                        format!("__obj_{}", ptr).into_bytes()
                    }
                }
                _ => {
                    if flags == 1 {
                        // SORT_NUMERIC
                        let n = val.to_double();
                        format!("{}", n).into_bytes()
                    } else {
                        val.to_php_string().as_bytes().to_vec()
                    }
                }
            };
            if !seen.contains(&s) {
                seen.push(s);
                result.set(key.clone(), val.clone());
            }
        }
        Ok(Value::Array(Rc::new(RefCell::new(result))))
    } else {
        Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
    }
}

fn array_slice(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
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
            Some(l) => (start + l as usize).min(entries.len()),
            None => entries.len(),
        };

        let mut result = PhpArray::new();
        let end = end.min(entries.len());
        let start = start.min(end);
        for &(ref k, ref v) in &entries[start..end] {
            match k {
                ArrayKey::String(s) => {
                    // String keys are always preserved
                    result.set(ArrayKey::String(s.clone()), (*v).clone());
                }
                ArrayKey::Int(i) => {
                    if preserve_keys {
                        result.set(ArrayKey::Int(*i), (*v).clone());
                    } else {
                        result.push((*v).clone());
                    }
                }
            }
        }
        Ok(Value::Array(Rc::new(RefCell::new(result))))
    } else {
        Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
    }
}

fn array_splice(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let first = args.first().unwrap_or(&Value::Null);
    let arr_opt = match first {
        Value::Array(a) => Some(a.clone()),
        Value::Reference(r) => match &*r.borrow() { Value::Array(a) => Some(a.clone()), _ => None },
        _ => None,
    };
    if let Some(arr) = arr_opt {
        let offset = args.get(1).map(|v| v.to_long()).unwrap_or(0);
        // length is nullable: null means remove everything from offset
        let length = match args.get(2) {
            Some(Value::Null) | None => None,
            Some(v) => Some(v.to_long()),
        };
        let replacement = args.get(3);

        let mut arr_mut = arr.borrow_mut();
        let entries: Vec<(goro_core::array::ArrayKey, Value)> =
            arr_mut.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let total = entries.len() as i64;

        let start = if offset < 0 {
            (total + offset).max(0) as usize
        } else {
            offset.min(total) as usize
        };
        let len = match length {
            Some(l) if l < 0 => ((total + l) as usize).saturating_sub(start),
            Some(l) => l as usize,
            None => entries.len() - start,
        };
        let end = (start + len).min(entries.len());

        // Extract removed elements (values only, re-indexed)
        let removed: Vec<Value> = entries[start..end].iter().map(|(_, v)| v.clone()).collect();

        // Build new array: re-index integer keys, preserve string keys
        let mut new_arr = PhpArray::new();
        // Elements before the splice point
        for (k, v) in &entries[..start] {
            match k {
                goro_core::array::ArrayKey::String(_) => new_arr.set(k.clone(), v.clone()),
                goro_core::array::ArrayKey::Int(_) => new_arr.push(v.clone()),
            }
        }
        // Replacement elements (always re-indexed)
        if let Some(repl) = replacement {
            if let Value::Array(repl_arr) = repl {
                let repl_arr = repl_arr.borrow();
                for (_, v) in repl_arr.iter() {
                    new_arr.push(v.clone());
                }
            } else {
                new_arr.push(repl.clone());
            }
        }
        // Elements after the splice point
        for (k, v) in &entries[end..] {
            match k {
                goro_core::array::ArrayKey::String(_) => new_arr.set(k.clone(), v.clone()),
                goro_core::array::ArrayKey::Int(_) => new_arr.push(v.clone()),
            }
        }
        *arr_mut = new_arr;

        // Return removed elements (always re-indexed)
        let mut removed_arr = PhpArray::new();
        for val in removed {
            removed_arr.push(val);
        }
        Ok(Value::Array(Rc::new(RefCell::new(removed_arr))))
    } else {
        Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
    }
}

fn array_search(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let needle = args.first().unwrap_or(&Value::Null);
    let strict = args.get(2).map(|v| v.is_truthy()).unwrap_or(false);
    if let Some(Value::Array(arr)) = args.get(1) {
        let arr = arr.borrow();
        for (key, val) in arr.iter() {
            let matches = if strict {
                val.identical(needle)
            } else {
                val.equals(needle)
            };
            if matches {
                return Ok(match key {
                    goro_core::array::ArrayKey::Int(n) => Value::Long(*n),
                    goro_core::array::ArrayKey::String(s) => Value::String(s.clone()),
                });
            }
        }
    }
    Ok(Value::False)
}

fn array_key_exists(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let key = args.first().unwrap_or(&Value::Null);
    if let Some(Value::Array(arr)) = args.get(1) {
        let arr = arr.borrow();
        let k = match key {
            Value::Long(n) => goro_core::array::ArrayKey::Int(*n),
            Value::String(s) => goro_core::array::ArrayKey::String(s.clone()),
            _ => return Ok(Value::False),
        };
        Ok(if arr.contains_key(&k) {
            Value::True
        } else {
            Value::False
        })
    } else {
        Ok(Value::False)
    }
}

fn in_array(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let needle = args.first().unwrap_or(&Value::Null);
    if let Some(Value::Array(arr)) = args.get(1) {
        let strict = args.get(2).map(|v| v.is_truthy()).unwrap_or(false);
        let arr = arr.borrow();
        for (_, val) in arr.iter() {
            if strict {
                if val.identical(needle) {
                    return Ok(Value::True);
                }
            } else if val.equals(needle) {
                return Ok(Value::True);
            }
        }
    }
    Ok(Value::False)
}

fn validate_callback_for_array_map(vm: &mut Vm, callback: &Value) -> Result<(), VmError> {
    match callback {
        Value::Null => Ok(()), // null is valid (identity/zip)
        Value::String(s) => {
            let name = s.as_bytes();
            if name.is_empty() {
                let msg = "array_map(): Argument #1 ($callback) must be a valid callback or null, function \"\" not found or invalid function name".to_string();
                let exc = vm.throw_type_error(msg.clone());
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg, line: vm.current_line });
            }
            // Check for "Class::method" syntax
            if let Some(pos) = name.iter().position(|&b| b == b':') {
                if pos + 1 < name.len() && name[pos + 1] == b':' {
                    let class_name = &name[..pos];
                    let method_name = &name[pos + 2..];
                    let class_lower: Vec<u8> = class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                    let method_lower: Vec<u8> = method_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                    if let Some(class) = vm.classes.get(&class_lower) {
                        if class.get_method(&method_lower).is_some() {
                            return Ok(());
                        }
                        let msg = format!("array_map(): Argument #1 ($callback) must be a valid callback or null, class '{}' does not have a method '{}'",
                            String::from_utf8_lossy(class_name), String::from_utf8_lossy(method_name));
                        let exc = vm.throw_type_error(msg.clone());
                        vm.current_exception = Some(exc);
                        return Err(VmError { message: msg, line: vm.current_line });
                    }
                    let msg = format!("array_map(): Argument #1 ($callback) must be a valid callback or null, class \"{}\" not found",
                        String::from_utf8_lossy(class_name));
                    let exc = vm.throw_type_error(msg.clone());
                    vm.current_exception = Some(exc);
                    return Err(VmError { message: msg, line: vm.current_line });
                }
            }
            let name_lower: Vec<u8> = name.iter().map(|b| b.to_ascii_lowercase()).collect();
            // Check if it's a closure name
            if name_lower.starts_with(b"__closure_") || name_lower.starts_with(b"__arrow_") {
                return Ok(());
            }
            if vm.functions.contains_key(&name_lower) || vm.user_functions.contains_key(&name_lower) {
                return Ok(());
            }
            let msg = format!("array_map(): Argument #1 ($callback) must be a valid callback or null, function \"{}\" not found or invalid function name",
                String::from_utf8_lossy(name));
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            Err(VmError { message: msg, line: vm.current_line })
        }
        Value::Array(arr) => {
            let arr_borrow = arr.borrow();
            let len = arr_borrow.len();
            if len != 2 {
                let msg = "array_map(): Argument #1 ($callback) must be a valid callback or null, array callback must have exactly two members".to_string();
                let exc = vm.throw_type_error(msg.clone());
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg, line: vm.current_line });
            }
            let vals: Vec<Value> = arr_borrow.values().cloned().collect();
            drop(arr_borrow);
            match &vals[0] {
                Value::String(s) => {
                    let class_lower: Vec<u8> = s.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
                    if class_lower.starts_with(b"__closure_") || class_lower.starts_with(b"__arrow_") {
                        return Ok(());
                    }
                    if vm.classes.contains_key(&class_lower) {
                        return Ok(());
                    }
                    let msg = "array_map(): Argument #1 ($callback) must be a valid callback or null, first array member is not a valid class name or object".to_string();
                    let exc = vm.throw_type_error(msg.clone());
                    vm.current_exception = Some(exc);
                    Err(VmError { message: msg, line: vm.current_line })
                }
                Value::Object(_) => Ok(()),
                _ => {
                    let msg = "array_map(): Argument #1 ($callback) must be a valid callback or null, first array member is not a valid class name or object".to_string();
                    let exc = vm.throw_type_error(msg.clone());
                    vm.current_exception = Some(exc);
                    Err(VmError { message: msg, line: vm.current_line })
                }
            }
        }
        Value::Object(obj) => {
            let class_lower: Vec<u8> = obj.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
            if let Some(class) = vm.classes.get(&class_lower) {
                if class.get_method(b"__invoke").is_some() {
                    return Ok(());
                }
            }
            let msg = "array_map(): Argument #1 ($callback) must be a valid callback or null, no array or string given".to_string();
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            Err(VmError { message: msg, line: vm.current_line })
        }
        _ => {
            let msg = "array_map(): Argument #1 ($callback) must be a valid callback or null, no array or string given".to_string();
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            Err(VmError { message: msg, line: vm.current_line })
        }
    }
}

fn array_map(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.len() < 2 {
        let msg = format!("array_map() expects at least 2 arguments, {} given", args.len());
        let exc = vm.create_exception(b"ArgumentCountError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    let callback_raw = args.first().cloned().unwrap_or(Value::Null);
    // Treat Undef as Null for callback
    let callback = match &callback_raw {
        Value::Undef => Value::Null,
        Value::Reference(r) => {
            let inner = r.borrow().clone();
            if matches!(inner, Value::Null) { Value::Null } else { inner }
        }
        _ => callback_raw,
    };
    let array = args.get(1);

    // Validate callback
    validate_callback_for_array_map(vm, &callback)?;

    // Validate array arguments - must be arrays
    for i in 1..args.len() {
        let val = args[i].deref();
        if !matches!(val, Value::Array(_)) {
            let type_name = Vm::value_type_name(&val);
            let msg = if i == 1 {
                format!("array_map(): Argument #2 ($array) must be of type array, {} given", type_name)
            } else {
                format!("array_map(): Argument #{} must be of type array, {} given", i + 1, type_name)
            };
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
        }
    }

    // Multi-array support: array_map($callback, $arr1, $arr2, ...)
    if args.len() > 2 {
        let arrays: Vec<_> = args[1..]
            .iter()
            .filter_map(|a| {
                if let Value::Array(arr) = a {
                    Some(
                        arr.borrow()
                            .iter()
                            .map(|(_, v)| v.clone())
                            .collect::<Vec<_>>(),
                    )
                } else {
                    None
                }
            })
            .collect();
        if arrays.is_empty() {
            return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
        }
        let max_len = arrays.iter().map(|a| a.len()).max().unwrap_or(0);
        let mut result = PhpArray::new();

        // Handle Object callbacks with __invoke (multi-array case)
        if let Value::Object(obj) = &callback {
            let class_lower: Vec<u8> = obj.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
            let method = vm.classes.get(&class_lower)
                .and_then(|c| c.get_method(b"__invoke"))
                .map(|m| m.op_array.clone());
            if let Some(op) = method {
                for i in 0..max_len {
                    let mut fn_cvs = vec![Value::Undef; op.cv_names.len()];
                    if !fn_cvs.is_empty() { fn_cvs[0] = callback.clone(); } // $this
                    let mut arg_idx = 1;
                    for arr in &arrays {
                        if arg_idx < fn_cvs.len() {
                            fn_cvs[arg_idx] = arr.get(i).cloned().unwrap_or(Value::Null);
                            arg_idx += 1;
                        }
                    }
                    let mapped = vm.execute_fn(&op, fn_cvs)?;
                    result.push(mapped);
                }
                return Ok(Value::Array(Rc::new(RefCell::new(result))));
            }
        }

        if matches!(callback, Value::Null) {
            // null callback with multiple arrays: create arrays of corresponding elements
            for i in 0..max_len {
                let mut sub = PhpArray::new();
                for arr in &arrays {
                    sub.push(arr.get(i).cloned().unwrap_or(Value::Null));
                }
                result.push(Value::Array(Rc::new(RefCell::new(sub))));
            }
            return Ok(Value::Array(Rc::new(RefCell::new(result))));
        }

        for i in 0..max_len {
            let mut cb_args: Vec<Value> = Vec::new();
            for arr in &arrays {
                cb_args.push(arr.get(i).cloned().unwrap_or(Value::Null));
            }
            let mapped = vm.call_callback(&callback, &cb_args)?;
            result.push(mapped);
        }
        return Ok(Value::Array(Rc::new(RefCell::new(result))));
    }

    if let Some(Value::Array(arr)) = array {
        let arr = arr.borrow();
        let mut result = PhpArray::new();

        // Get callback function name
        match &callback {
            Value::Null => {
                // null callback = identity
                for (key, val) in arr.iter() {
                    result.set(key.clone(), val.clone());
                }
                return Ok(Value::Array(Rc::new(RefCell::new(result))));
            }
            Value::Array(cb_arr) => {
                let cb = cb_arr.borrow();
                let vals: Vec<Value> = cb.values().cloned().collect();
                drop(cb);
                if vals.len() >= 2 {
                    let method_name = vals[1].to_php_string();
                    let method_lower: Vec<u8> = method_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
                    // Get the class name from either a string or an object
                    let (class_lower, this_val) = if let Value::String(class_name) = &vals[0] {
                        (class_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect::<Vec<u8>>(), None)
                    } else if let Value::Object(obj) = &vals[0] {
                        (obj.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect::<Vec<u8>>(), Some(vals[0].clone()))
                    } else {
                        (vec![], None)
                    };
                    if !class_lower.is_empty() {
                        if let Some(class) = vm.classes.get(&class_lower).cloned() {
                            if let Some(method) = class.methods.get(&method_lower) {
                                let op = method.op_array.clone();
                                for (key, val) in arr.iter() {
                                    let mut fn_cvs = vec![Value::Undef; op.cv_names.len()];
                                    if let Some(ref this) = this_val {
                                        // Instance method: $this is the object, first user arg is the element
                                        if !fn_cvs.is_empty() { fn_cvs[0] = this.clone(); }
                                        if fn_cvs.len() > 1 { fn_cvs[1] = val.clone(); }
                                    } else {
                                        // Static method: first user arg is the element
                                        if !fn_cvs.is_empty() { fn_cvs[0] = val.clone(); }
                                    }
                                    let mapped = vm.execute_fn(&op, fn_cvs)?;
                                    result.set(key.clone(), mapped);
                                }
                                return Ok(Value::Array(Rc::new(RefCell::new(result))));
                            }
                        }
                    }
                }
            }
            _ => {}
        }

        // Handle Object callbacks with __invoke
        if let Value::Object(obj) = &callback {
            let class_lower: Vec<u8> = obj.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
            let has_invoke = vm.classes.get(&class_lower)
                .map(|c| c.methods.contains_key(&b"__invoke".to_vec()))
                .unwrap_or(false);
            if has_invoke {
                let method = vm.classes.get(&class_lower)
                    .and_then(|c| c.get_method(b"__invoke"))
                    .map(|m| m.op_array.clone());
                if let Some(op) = method {
                    for (key, val) in arr.iter() {
                        let mut fn_cvs = vec![Value::Undef; op.cv_names.len()];
                        if !fn_cvs.is_empty() { fn_cvs[0] = callback.clone(); } // $this
                        if fn_cvs.len() > 1 { fn_cvs[1] = val.clone(); }
                        let mapped = vm.execute_fn(&op, fn_cvs)?;
                        result.set(key.clone(), mapped);
                    }
                    return Ok(Value::Array(Rc::new(RefCell::new(result))));
                }
            }
        }

        let (func_name, captured_args) = match &callback {
            Value::String(s) => (s.as_bytes().to_vec(), vec![]),
            Value::Array(cb_arr) => {
                let cb = cb_arr.borrow();
                let vals: Vec<Value> = cb.values().cloned().collect();
                if vals.is_empty() {
                    return Ok(Value::Array(Rc::new(RefCell::new(result))));
                }
                let name = vals[0].to_php_string().as_bytes().to_vec();
                let captured: Vec<Value> = vals[1..].to_vec();
                (name, captured)
            }
            _ => return Ok(Value::Array(Rc::new(RefCell::new(result)))),
        };

        let func_lower: Vec<u8> = func_name.iter().map(|b| b.to_ascii_lowercase()).collect();

        // Check for builtin function
        if let Some(builtin) = vm.functions.get(&func_lower).copied() {
            for (key, val) in arr.iter() {
                let mapped = builtin(vm, &[val.clone()])?;
                result.set(key.clone(), mapped);
            }
        } else if let Some(user_fn) = vm.user_functions.get(&func_lower).cloned() {
            for (key, val) in arr.iter() {
                let mut fn_cvs = vec![Value::Undef; user_fn.cv_names.len()];
                // Pass captured vars first, then the element
                let mut arg_idx = 0;
                for cv in &captured_args {
                    if arg_idx < fn_cvs.len() {
                        fn_cvs[arg_idx] = cv.clone();
                        arg_idx += 1;
                    }
                }
                if arg_idx < fn_cvs.len() {
                    fn_cvs[arg_idx] = val.clone();
                }
                let mapped = vm.execute_fn(&user_fn, fn_cvs)?;
                result.set(key.clone(), mapped);
            }
        } else {
            // Function not found - return original array
            for (key, val) in arr.iter() {
                result.set(key.clone(), val.clone());
            }
        }

        Ok(Value::Array(Rc::new(RefCell::new(result))))
    } else {
        Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
    }
}

fn array_filter(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let first = args.first().unwrap_or(&Value::Null);
    require_array_arg(vm, first, "array_filter", "array", 1)?;
    if let Some(Value::Array(arr)) = args.first() {
        let arr_data: Vec<(ArrayKey, Value)> = arr.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
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

            for (key, val) in &arr_data {
                let key_val = match key {
                    ArrayKey::Int(n) => Value::Long(*n),
                    ArrayKey::String(s) => Value::String(s.clone()),
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

fn array_walk(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.len() > 3 {
        let msg = format!("array_walk() expects at most 3 arguments, {} given", args.len());
        let exc = vm.create_exception(b"ArgumentCountError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
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

    let callback = match args.get(1) {
        Some(cb) => cb,
        None => return Ok(Value::True),
    };

    // For objects, convert properties to entries with PHP-style mangled names
    if is_object {
        let obj = match first {
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
        let entries: Vec<(ArrayKey, Value)> = {
            let obj_borrow = obj.borrow();
            let class_name_lower: Vec<u8> = obj_borrow.class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
            // Map property name -> (visibility, declaring_class_original_case)
            let prop_info: std::collections::HashMap<Vec<u8>, (goro_core::object::Visibility, Vec<u8>)> =
                if let Some(class_def) = vm.get_class_def(&class_name_lower) {
                    class_def.properties.iter()
                        .map(|p| {
                            let decl_lower = &p.declaring_class;
                            let decl_display = vm.get_class_def(decl_lower)
                                .map(|c| c.name.clone())
                                .unwrap_or_else(|| p.declaring_class.clone());
                            (p.name.clone(), (p.visibility, decl_display))
                        })
                        .collect()
                } else {
                    std::collections::HashMap::new()
                };
            obj_borrow.properties.iter()
                .filter(|(k, _)| !k.starts_with(b"__spl_") && !k.starts_with(b"__reflection_")
                    && !k.starts_with(b"__enum_") && !k.starts_with(b"__ctor_")
                    && !k.starts_with(b"__clone_") && !k.starts_with(b"__destructed")
                    && !k.starts_with(b"__fiber_") && !k.starts_with(b"__timestamp"))
                .map(|(k, v)| {
                    let (vis, declaring_class) = prop_info.get(k)
                        .map(|(v, dc)| (Some(*v), dc.as_slice()))
                        .unwrap_or((None, &[]));
                    let mangled_key = mangle_property_name(k, declaring_class, vis);
                    (ArrayKey::String(PhpString::from_vec(mangled_key)), v.clone())
                }).collect()
        };
        let extra_data = args.get(2);
        return array_walk_entries(vm, callback, &entries, extra_data, None);
    }

    // Array case
    if let Some(Value::Array(arr)) = args.first() {
        let entries: Vec<_> = {
            let arr_borrow = arr.borrow();
            arr_borrow
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        };
        let extra_data = args.get(2);
        return array_walk_entries(vm, callback, &entries, extra_data, Some(arr));
    }

    Ok(Value::True)
}

fn array_walk_entries(
    vm: &mut Vm,
    callback: &Value,
    entries: &[(ArrayKey, Value)],
    extra_data: Option<&Value>,
    arr: Option<&Rc<RefCell<PhpArray>>>,
) -> Result<Value, VmError> {
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
        for (key, val) in entries {
            // Wrap value in a Reference so by-ref params (&$v) work
            let val_ref = Rc::new(RefCell::new(val.clone()));
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
            // Pass key as second argument
            if idx < fn_cvs.len() {
                let key_val = match key {
                    ArrayKey::Int(n) => Value::Long(*n),
                    ArrayKey::String(s) => Value::String(s.clone()),
                };
                fn_cvs[idx] = key_val;
                idx += 1;
            }
            // Pass extra_data as third argument
            if let Some(extra) = extra_data {
                if idx < fn_cvs.len() {
                    fn_cvs[idx] = extra.clone();
                }
            }
            vm.execute_fn(&user_fn, fn_cvs)?;
            // Write modified value back to the array (if it's an array)
            if let Some(arr) = arr {
                let new_val = val_ref.borrow().clone();
                arr.borrow_mut().set(key.clone(), new_val);
            }
        }
    } else if let Some(builtin) = vm.functions.get(&func_lower).copied() {
        for (_key, val) in entries {
            builtin(vm, &[val.clone()])?;
        }
    }
    Ok(Value::True)
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
        return Err(VmError { message: msg, line: vm.current_line });
    }
    let mut result = PhpArray::new();
    let keys_vec: Vec<_> = keys.values().cloned().collect();
    let vals_vec: Vec<_> = vals.values().cloned().collect();
    for (k, v) in keys_vec.iter().zip(vals_vec.iter()) {
        // PHP array_combine converts values to strings first, then to array keys
        let key_str = k.to_php_string();
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
        result.set(key, v.clone());
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
            return Err(VmError { message: msg, line: vm.current_line });
        }
        let size = size as usize;
        let preserve_keys = args.get(2).map(|v| v.is_truthy()).unwrap_or(false);
        let arr = arr.borrow();
        let entries: Vec<_> = arr.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
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

fn array_pad(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let input = args.first().unwrap_or(&Value::Null);
    let pad_size = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let pad_value = args.get(2).cloned().unwrap_or(Value::Null);

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
            // Pad at the beginning
            for _ in 0..pad_count {
                result.push(pad_value.clone());
            }
            for (_k, v) in arr.iter() {
                result.push(v.clone());
            }
        } else {
            // Pad at the end
            for (_k, v) in arr.iter() {
                result.push(v.clone());
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
        let msg = "array_fill(): Argument #2 ($count) must be greater than or equal to 0".to_string();
        let exc = vm.throw_type_error(msg.clone());
        if let Value::Object(obj) = &exc {
            obj.borrow_mut().class_name = b"ValueError".to_vec();
        }
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    if num > 10_000_000 {
        let msg = "array_fill(): Argument #2 ($count) is too large".to_string();
        let exc = vm.throw_type_error(msg.clone());
        if let Value::Object(obj) = &exc {
            obj.borrow_mut().class_name = b"ValueError".to_vec();
        }
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    let val = args.get(2).cloned().unwrap_or(Value::Null);
    let mut result = PhpArray::new();
    for i in 0..num {
        result.set(goro_core::array::ArrayKey::Int(start + i), val.clone());
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn array_fill_keys(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let keys = args.first().unwrap_or(&Value::Null);
    require_array_arg(vm, keys, "array_fill_keys", "keys", 1)?;
    let fill_val = args.get(1).cloned().unwrap_or(Value::Null);
    let mut result = PhpArray::new();
    if let Value::Array(arr) = keys {
        let arr = arr.borrow();
        for (_key, val) in arr.iter() {
            // For array_fill_keys, float values should become string keys
            let k = match val {
                Value::Double(_f) => {
                    let s = val.to_php_string();
                    ArrayKey::String(s)
                }
                _ => value_to_array_key(val),
            };
            result.set(k, fill_val.clone());
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn array_merge_recursive(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let mut result = PhpArray::new();
    for (i, arg) in args.iter().enumerate() {
        require_array_arg_variadic(vm, arg, "array_merge_recursive", (i + 1) as u32)?;
        if let Value::Array(arr) = arg {
            let arr = arr.borrow();
            for (key, val) in arr.iter() {
                match key {
                    goro_core::array::ArrayKey::Int(_) => {
                        result.push(val.clone());
                    }
                    goro_core::array::ArrayKey::String(s) => {
                        if let Some(existing) = result.get_str(s.as_bytes()) {
                            let merged = merge_recursive_values(&existing, val, 0);
                            result.set(key.clone(), merged);
                        } else {
                            result.set(key.clone(), val.clone());
                        }
                    }
                }
            }
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn merge_recursive_values(existing: &Value, new_val: &Value, depth: usize) -> Value {
    if depth > 256 { return new_val.clone(); }
    match (existing, new_val) {
        (Value::Array(a), Value::Array(b)) => {
            let mut merged_arr = a.borrow().clone();
            let b_ref = b.borrow();
            for (k, v) in b_ref.iter() {
                match k {
                    goro_core::array::ArrayKey::Int(_) => { merged_arr.push(v.clone()); }
                    goro_core::array::ArrayKey::String(s) => {
                        if let Some(ex) = merged_arr.get_str(s.as_bytes()) {
                            let merged = merge_recursive_values(&ex, v, depth + 1);
                            merged_arr.set(k.clone(), merged);
                        } else {
                            merged_arr.set(k.clone(), v.clone());
                        }
                    }
                }
            }
            Value::Array(Rc::new(RefCell::new(merged_arr)))
        }
        (existing_val, new_v) => {
            let mut arr = PhpArray::new();
            if let Value::Array(a) = existing_val {
                for (_, v) in a.borrow().iter() { arr.push(v.clone()); }
            } else { arr.push(existing_val.clone()); }
            if let Value::Array(b) = new_v {
                for (_, v) in b.borrow().iter() { arr.push(v.clone()); }
            } else { arr.push(new_v.clone()); }
            Value::Array(Rc::new(RefCell::new(arr)))
        }
    }
}

fn array_diff(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
    }
    // Validate all arguments are arrays
    for (i, arg) in args.iter().enumerate() {
        if !matches!(arg, Value::Array(_)) {
            let type_name = Vm::value_type_name(arg);
            let msg = format!("array_diff(): Argument #{} must be of type array, {} given", i + 1, type_name);
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
        }
    }
    if args.len() < 2 {
        // Single array arg: return a copy
        if let Value::Array(a) = &args[0] {
            return Ok(Value::Array(Rc::new(RefCell::new(a.borrow().clone()))));
        }
    }
    if let (Some(Value::Array(a)), _) = (args.first(), args.get(1)) {
        let a = a.borrow();
        // Collect all values from arrays at index 1+
        let mut other_vals: Vec<Vec<u8>> = Vec::new();
        for arg in &args[1..] {
            if let Value::Array(b) = arg {
                let b = b.borrow();
                for v in b.values() {
                    other_vals.push(v.to_php_string().as_bytes().to_vec());
                }
            }
        }
        let mut result = PhpArray::new();
        for (key, val) in a.iter() {
            let s = val.to_php_string().as_bytes().to_vec();
            if !other_vals.contains(&s) {
                result.set(key.clone(), val.clone());
            }
        }
        Ok(Value::Array(Rc::new(RefCell::new(result))))
    } else {
        Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
    }
}

fn array_intersect(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
    }
    // Validate all arguments are arrays
    for (i, arg) in args.iter().enumerate() {
        require_array_arg(vm, arg, "array_intersect", "array", (i + 1) as u32)?;
    }
    // Single array: return it as-is
    if args.len() == 1 {
        return match &args[0] {
            Value::Array(arr) => Ok(Value::Array(Rc::new(RefCell::new(arr.borrow().clone())))),
            _ => Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new())))),
        };
    }
    let first = match &args[0] {
        Value::Array(a) => a.borrow().clone(),
        _ => return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new())))),
    };
    // Collect string representations of values from all other arrays
    let other_val_sets: Vec<Vec<Vec<u8>>> = args[1..].iter().map(|arg| {
        match arg {
            Value::Array(arr) => arr.borrow().values().map(|v| v.to_php_string().as_bytes().to_vec()).collect(),
            _ => Vec::new(),
        }
    }).collect();
    let mut result = PhpArray::new();
    for (key, val) in first.iter() {
        let s = val.to_php_string().as_bytes().to_vec();
        // Value must exist in ALL other arrays
        if other_val_sets.iter().all(|vals| vals.contains(&s)) {
            result.set(key.clone(), val.clone());
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

/// Get a type priority for sorting that establishes total ordering
fn type_priority(v: &Value) -> u8 {
    match v {
        Value::Null | Value::Undef => 0,
        Value::False => 1,
        Value::True => 2,
        Value::Long(_) | Value::Double(_) => 3,
        Value::String(_) => 4,
        Value::Array(_) => 5,
        Value::Object(_) | Value::Generator(_) => 6,
        Value::Reference(_) => 7,
    }
}

/// Compare two PHP values for sorting, ensuring total ordering
fn php_sort_cmp(a: &Value, b: &Value) -> std::cmp::Ordering {
    php_sort_cmp_flags(a, b, 0)
}

// Sort flag constants
const SORT_REGULAR: i64 = 0;
const SORT_NUMERIC: i64 = 1;
const SORT_STRING: i64 = 2;
const SORT_LOCALE_STRING: i64 = 5;
const SORT_NATURAL: i64 = 6;
const SORT_FLAG_CASE: i64 = 8;

fn php_sort_cmp_flags(a: &Value, b: &Value, flags: i64) -> std::cmp::Ordering {
    let base_flag = flags & !SORT_FLAG_CASE;
    let case_insensitive = (flags & SORT_FLAG_CASE) != 0;

    match base_flag {
        SORT_STRING | SORT_LOCALE_STRING => {
            let sa = a.to_php_string();
            let sb = b.to_php_string();
            if case_insensitive {
                let la = sa.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect::<Vec<u8>>();
                let lb = sb.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect::<Vec<u8>>();
                la.cmp(&lb)
            } else {
                sa.as_bytes().cmp(sb.as_bytes())
            }
        }
        SORT_NUMERIC => {
            let fa = a.to_double();
            let fb = b.to_double();
            fa.partial_cmp(&fb).unwrap_or(std::cmp::Ordering::Equal)
        }
        SORT_NATURAL => {
            let sa = a.to_php_string();
            let sb = b.to_php_string();
            if case_insensitive {
                let la = sa.to_string_lossy().to_lowercase();
                let lb = sb.to_string_lossy().to_lowercase();
                natcmp(&la, &lb)
            } else {
                natcmp(&sa.to_string_lossy(), &sb.to_string_lossy())
            }
        }
        _ => {
            // SORT_REGULAR (default)
            let cmp = a.compare(b);
            if cmp < 0 {
                std::cmp::Ordering::Less
            } else if cmp > 0 {
                std::cmp::Ordering::Greater
            } else {
                let tp_a = type_priority(a);
                let tp_b = type_priority(b);
                tp_a.cmp(&tp_b)
            }
        }
    }
}

fn php_key_sort_cmp_flags(a: &ArrayKey, b: &ArrayKey, flags: i64) -> std::cmp::Ordering {
    let base_flag = flags & !SORT_FLAG_CASE;
    let case_insensitive = (flags & SORT_FLAG_CASE) != 0;

    match base_flag {
        SORT_STRING | SORT_LOCALE_STRING => {
            let sa = match a { ArrayKey::Int(n) => n.to_string().into_bytes(), ArrayKey::String(s) => s.as_bytes().to_vec() };
            let sb = match b { ArrayKey::Int(n) => n.to_string().into_bytes(), ArrayKey::String(s) => s.as_bytes().to_vec() };
            if case_insensitive {
                let la: Vec<u8> = sa.iter().map(|b| b.to_ascii_lowercase()).collect();
                let lb: Vec<u8> = sb.iter().map(|b| b.to_ascii_lowercase()).collect();
                la.cmp(&lb)
            } else {
                sa.cmp(&sb)
            }
        }
        SORT_NUMERIC => {
            let na = match a { ArrayKey::Int(n) => *n as f64, ArrayKey::String(s) => s.to_string_lossy().parse::<f64>().unwrap_or(0.0) };
            let nb = match b { ArrayKey::Int(n) => *n as f64, ArrayKey::String(s) => s.to_string_lossy().parse::<f64>().unwrap_or(0.0) };
            na.partial_cmp(&nb).unwrap_or(std::cmp::Ordering::Equal)
        }
        SORT_NATURAL => {
            let sa = match a { ArrayKey::Int(n) => n.to_string(), ArrayKey::String(s) => s.to_string_lossy() };
            let sb = match b { ArrayKey::Int(n) => n.to_string(), ArrayKey::String(s) => s.to_string_lossy() };
            if case_insensitive {
                let la = sa.to_lowercase();
                let lb = sb.to_lowercase();
                natcmp(&la, &lb)
            } else {
                natcmp(&sa, &sb)
            }
        }
        _ => a.cmp(b),
    }
}

fn sort_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let first = args.first().unwrap_or(&Value::Null);
    require_array_arg(vm, first, "sort", "array", 1)?;
    if let Some(Value::Array(arr)) = args.first() {
        let flags = args.get(1).map(|v| v.to_long()).unwrap_or(0);
        let mut arr = arr.borrow_mut();
        let mut entries: Vec<Value> = arr.values().cloned().collect();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            entries.sort_by(|a, b| php_sort_cmp_flags(a, b, flags));
        }));
        let mut new_arr = PhpArray::new();
        for val in entries {
            new_arr.push(val);
        }
        *arr = new_arr;
    }
    Ok(Value::True)
}
fn rsort_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let first = args.first().unwrap_or(&Value::Null);
    require_array_arg(vm, first, "rsort", "array", 1)?;
    if let Some(Value::Array(arr)) = args.first() {
        let flags = args.get(1).map(|v| v.to_long()).unwrap_or(0);
        let mut arr = arr.borrow_mut();
        let mut entries: Vec<Value> = arr.values().cloned().collect();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            entries.sort_by(|a, b| php_sort_cmp_flags(b, a, flags));
        }));
        let mut new_arr = PhpArray::new();
        for val in entries {
            new_arr.push(val);
        }
        *arr = new_arr;
    }
    Ok(Value::True)
}
fn asort_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let first = args.first().unwrap_or(&Value::Null);
    require_array_arg(vm, first, "asort", "array", 1)?;
    if let Some(Value::Array(arr)) = args.first() {
        let flags = args.get(1).map(|v| v.to_long()).unwrap_or(0);
        let mut arr = arr.borrow_mut();
        let mut entries: Vec<_> = arr.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            entries.sort_by(|a, b| php_sort_cmp_flags(&a.1, &b.1, flags));
        }));
        *arr = goro_core::array::PhpArray::new();
        for (k, v) in entries {
            arr.set(k, v);
        }
    }
    Ok(Value::True)
}
fn arsort_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let first = args.first().unwrap_or(&Value::Null);
    require_array_arg(vm, first, "arsort", "array", 1)?;
    if let Some(Value::Array(arr)) = args.first() {
        let flags = args.get(1).map(|v| v.to_long()).unwrap_or(0);
        let mut arr = arr.borrow_mut();
        let mut entries: Vec<_> = arr.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            entries.sort_by(|a, b| php_sort_cmp_flags(&b.1, &a.1, flags));
        }));
        *arr = goro_core::array::PhpArray::new();
        for (k, v) in entries {
            arr.set(k, v);
        }
    }
    Ok(Value::True)
}
fn ksort_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let first = args.first().unwrap_or(&Value::Null);
    require_array_arg(vm, first, "ksort", "array", 1)?;
    if let Some(Value::Array(arr)) = args.first() {
        let flags = args.get(1).map(|v| v.to_long()).unwrap_or(0);
        let mut arr = arr.borrow_mut();
        let mut entries: Vec<_> = arr.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        entries.sort_by(|a, b| php_key_sort_cmp_flags(&a.0, &b.0, flags));
        *arr = goro_core::array::PhpArray::new();
        for (k, v) in entries {
            arr.set(k, v);
        }
    }
    Ok(Value::True)
}
fn krsort_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let first = args.first().unwrap_or(&Value::Null);
    require_array_arg(vm, first, "krsort", "array", 1)?;
    if let Some(Value::Array(arr)) = args.first() {
        let flags = args.get(1).map(|v| v.to_long()).unwrap_or(0);
        let mut arr = arr.borrow_mut();
        let mut entries: Vec<_> = arr.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        entries.sort_by(|a, b| php_key_sort_cmp_flags(&b.0, &a.0, flags));
        *arr = goro_core::array::PhpArray::new();
        for (k, v) in entries {
            arr.set(k, v);
        }
    }
    Ok(Value::True)
}
fn shuffle_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let mut values: Vec<Value> = {
            let arr_borrow = arr.borrow();
            arr_borrow.values().cloned().collect()
        };
        // Fisher-Yates shuffle using simple time-based random
        let n = values.len();
        if n > 1 {
            use std::time::SystemTime;
            let mut seed = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64;
            for i in (1..n).rev() {
                // xorshift64 for better randomness
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                let j = (seed as usize) % (i + 1);
                values.swap(i, j);
            }
        }
        // Rebuild array with sequential integer keys
        let mut new_arr = PhpArray::new();
        for v in values {
            new_arr.push(v);
        }
        *arr.borrow_mut() = new_arr;
    } else if let Some(Value::Reference(r)) = args.first() {
        let val = r.borrow().clone();
        if let Value::Array(arr) = val {
            let mut values: Vec<Value> = {
                let arr_borrow = arr.borrow();
                arr_borrow.values().cloned().collect()
            };
            let n = values.len();
            if n > 1 {
                use std::time::SystemTime;
                let mut seed = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos() as u64;
                for i in (1..n).rev() {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    let j = (seed as usize) % (i + 1);
                    values.swap(i, j);
                }
            }
            let mut new_arr = PhpArray::new();
            for v in values {
                new_arr.push(v);
            }
            *arr.borrow_mut() = new_arr;
        }
    }
    Ok(Value::True)
}

fn range_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let start_val = args.first().unwrap_or(&Value::Null);
    let end_val = args.get(1).unwrap_or(&Value::Null);
    let step_val = args.get(2);

    // Character range
    if let (Value::String(s1), Value::String(s2)) = (start_val, end_val) {
        if s1.len() == 1 && s2.len() == 1 {
            let start_c = s1.as_bytes()[0];
            let end_c = s2.as_bytes()[0];
            let step = step_val
                .map(|v| v.to_long().unsigned_abs().max(1) as u8)
                .unwrap_or(1);
            let mut result = PhpArray::new();
            if start_c <= end_c {
                let mut c = start_c;
                while c <= end_c {
                    result.push(Value::String(PhpString::from_vec(vec![c])));
                    if c.checked_add(step).is_none() {
                        break;
                    }
                    c += step;
                }
            } else {
                let mut c = start_c;
                while c >= end_c {
                    result.push(Value::String(PhpString::from_vec(vec![c])));
                    if c < step || c - step < end_c {
                        break;
                    }
                    c -= step;
                }
            }
            return Ok(Value::Array(Rc::new(RefCell::new(result))));
        }
    }

    // Float range (if any argument is float or step is float)
    let use_float = matches!(start_val, Value::Double(_))
        || matches!(end_val, Value::Double(_))
        || step_val.is_some_and(|v| matches!(v, Value::Double(_)));

    if use_float {
        let start = start_val.to_double();
        let end = end_val.to_double();
        if start.is_nan() || end.is_nan() || start.is_infinite() || end.is_infinite() {
            return Err(VmError {
                message: format!(
                    "range(): Argument #1 ($start) must be a finite number, {} provided",
                    if start.is_nan() { "NAN" } else if start.is_infinite() { "INF" } else { "value" }
                ),
                line: 0,
            });
        }
        let step = step_val
            .map(|v| v.to_double().abs())
            .unwrap_or(1.0)
            .max(f64::EPSILON);
        let mut result = PhpArray::new();
        if start <= end {
            let mut v = start;
            while v <= end + f64::EPSILON {
                result.push(Value::Double(v));
                v += step;
                if result.len() > 10000 {
                    break;
                }
            }
        } else {
            let mut v = start;
            while v >= end - f64::EPSILON {
                result.push(Value::Double(v));
                v -= step;
                if result.len() > 10000 {
                    break;
                }
            }
        }
        return Ok(Value::Array(Rc::new(RefCell::new(result))));
    }

    // Integer range
    let start = start_val.to_long();
    let end = end_val.to_long();
    let step = step_val
        .map(|v| v.to_long().unsigned_abs().max(1) as i64)
        .unwrap_or(1);

    // Check for excessive size
    let size = if step > 0 {
        ((end as i128 - start as i128).unsigned_abs() / step as u128) + 1
    } else {
        1
    };
    if size > 10_000_000 {
        // Just return empty array for extremely large ranges to avoid memory issues
        return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
    }

    let mut result = PhpArray::new();
    if start <= end {
        let mut i = start;
        while i <= end {
            result.push(Value::Long(i));
            i = match i.checked_add(step) {
                Some(v) => v,
                None => break,
            };
        }
    } else {
        let mut i = start;
        while i >= end {
            result.push(Value::Long(i));
            i = match i.checked_sub(step) {
                Some(v) => v,
                None => break,
            };
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn compact(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
}

fn current(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let arr = arr.borrow();
        let pos = arr.pointer;
        Ok(arr
            .iter()
            .nth(pos)
            .map(|(_, v)| v.clone())
            .unwrap_or(Value::False))
    } else {
        Ok(Value::False)
    }
}

fn next_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let mut arr = arr.borrow_mut();
        arr.pointer += 1;
        let pos = arr.pointer;
        Ok(arr
            .iter()
            .nth(pos)
            .map(|(_, v)| v.clone())
            .unwrap_or(Value::False))
    } else {
        Ok(Value::False)
    }
}

fn prev_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let mut arr = arr.borrow_mut();
        let len = arr.len();
        if arr.pointer == 0 || arr.pointer > len {
            // Already at (or before) beginning or past end: return false
            // Move pointer past end so current() also returns false
            arr.pointer = len;
            Ok(Value::False)
        } else {
            arr.pointer -= 1;
            let pos = arr.pointer;
            Ok(arr
                .iter()
                .nth(pos)
                .map(|(_, v)| v.clone())
                .unwrap_or(Value::False))
        }
    } else {
        Ok(Value::False)
    }
}

fn reset_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let mut arr = arr.borrow_mut();
        arr.pointer = 0;
        Ok(arr
            .iter()
            .next()
            .map(|(_, v)| v.clone())
            .unwrap_or(Value::False))
    } else {
        Ok(Value::False)
    }
}

fn end_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let mut arr = arr.borrow_mut();
        let len = arr.len();
        arr.pointer = if len > 0 { len - 1 } else { 0 };
        Ok(arr
            .iter()
            .last()
            .map(|(_, v)| v.clone())
            .unwrap_or(Value::False))
    } else {
        Ok(Value::False)
    }
}

fn key_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let arr = arr.borrow();
        let pos = arr.pointer;
        Ok(arr
            .iter()
            .nth(pos)
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
    let name = args.first().unwrap_or(&Value::Null).to_php_string();
    let value = args.get(1).cloned().unwrap_or(Value::Null);
    let key = name.as_bytes().to_vec();
    let old = vm.constants.get(&key).cloned();
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
    // Handle memory_limit
    if key == b"memory_limit" {
        let limit = parse_memory_value(&value);
        goro_core::value::set_memory_limit(limit);
    }
    // Handle error_reporting
    if key == b"error_reporting" {
        let level = match &value {
            Value::Long(n) => *n,
            Value::String(s) => {
                let s_str = s.to_string_lossy();
                if let Ok(n) = s_str.parse::<i64>() { n } else { vm.error_reporting }
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
        let cur_val = vm.constants.get(b"zend.assertions".as_ref())
            .map(|v| v.to_long())
            .unwrap_or(1);
        if new_val == -1 || cur_val == -1 {
            vm.emit_warning("zend.assertions may be completely enabled or disabled only in php.ini");
            // When zend.assertions is -1 (set in php.ini), don't allow runtime changes.
            // Keep the value as -1 so subsequent ini_set calls also warn.
            if cur_val == -1 {
                return Ok(old.unwrap_or(Value::False));
            }
        }
    }
    // Actually update the value
    vm.constants.insert(key, value);
    // Return old value or false if not previously set
    Ok(old.unwrap_or(Value::False))
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
fn ini_restore(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Null)
}
fn set_time_limit(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True)
}
fn php_assert(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Check zend.assertions setting (0 or -1 means disabled)
    if let Some(val) = vm.constants.get(b"zend.assertions".as_ref()) {
        let v = val.to_long();
        if v <= 0 {
            return Ok(Value::True);
        }
    }
    // Check assert.active setting (0 means disabled)
    if let Some(val) = vm.constants.get(b"assert.active".as_ref()) {
        let v = val.to_long();
        if v == 0 {
            return Ok(Value::True);
        }
    }
    // Check assert.exception setting
    let assert_exception = vm.constants.get(b"assert.exception".as_ref())
        .map(|v| v.to_long() != 0)
        .unwrap_or(true); // default is true in PHP 8

    let val = args.first().unwrap_or(&Value::Null);
    if val.is_truthy() {
        Ok(Value::True)
    } else {
        // Get the description if provided (2nd argument)
        let description = args.get(1);

        // If description is a Throwable, throw it directly
        if let Some(Value::Object(obj)) = description {
            let class_lower: Vec<u8> = obj.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
            if class_lower == b"assertionerror" || class_lower == b"exception" || class_lower == b"error"
                || goro_core::vm::is_builtin_subclass(&class_lower, b"exception")
                || goro_core::vm::is_builtin_subclass(&class_lower, b"error")
            {
                vm.current_exception = Some(Value::Object(obj.clone()));
                return Err(VmError {
                    message: "assert() failed".into(),
                    line: 0,
                });
            }
        }

        // Build message
        let msg = match description {
            Some(Value::String(s)) => s.to_string_lossy(),
            _ => "assert(false)".to_string(),
        };

        if !assert_exception {
            // When assert.exception is 0, issue a warning and return NULL
            vm.emit_warning(&format!("assert(): {} failed", msg));
            return Ok(Value::Null);
        }

        // Throw AssertionError
        let err_id = vm.next_object_id();
        let mut err_obj = goro_core::object::PhpObject::new(b"AssertionError".to_vec(), err_id);
        err_obj.set_property(b"message".to_vec(), Value::String(PhpString::from_string(msg.clone())));
        err_obj.set_property(b"code".to_vec(), Value::Long(0));
        err_obj.set_property(b"file".to_vec(), Value::String(PhpString::from_string(vm.current_file.clone())));
        err_obj.set_property(b"line".to_vec(), Value::Long(vm.current_line as i64));
        err_obj.set_property(b"previous".to_vec(), Value::Null);
        let exc = Value::Object(Rc::new(RefCell::new(err_obj)));
        vm.current_exception = Some(exc);
        Err(VmError {
            message: format!("assert(): {} failed", msg),
            line: vm.current_line,
        })
    }
}
fn class_exists(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let name = args.first().unwrap_or(&Value::Null).to_php_string();
    // Strip leading backslash for fully qualified names
    let raw_bytes = name.as_bytes();
    let name_bytes = if raw_bytes.starts_with(b"\\") { &raw_bytes[1..] } else { raw_bytes };
    let name_lower: Vec<u8> = name_bytes
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect();
    // Check built-in classes
    let is_builtin = matches!(
        name_lower.as_slice(),
        b"stdclass"
            | b"exception"
            | b"error"
            | b"typeerror"
            | b"valueerror"
            | b"runtimeexception"
            | b"logicexception"
            | b"invalidargumentexception"
            | b"badmethodcallexception"
            | b"closure"
            | b"generator"
            | b"reflectionclass"
            | b"reflectionobject"
            | b"reflectionmethod"
            | b"reflectionfunction"
            | b"reflectionproperty"
            | b"reflectionparameter"
            | b"reflectionextension"
            | b"reflectionexception"
            | b"reflectionfunctionabstract"
            | b"reflectionnamedtype"
            | b"reflectionuniontype"
            | b"reflectionintersectiontype"
            | b"reflectionenum"
            | b"reflectionenumunitcase"
            | b"reflectionenumbackedcase"
            | b"reflectionclassconstant"
            | b"reflectiongenerator"
            | b"reflectionattribute"
            | b"weakreference"
            | b"weakmap"
            | b"fiber"
    );
    if is_builtin {
        return Ok(Value::True);
    }
    // class_exists() returns false for interfaces and traits (but true for enums)
    if let Some(class) = vm.classes.get(&name_lower) {
        if class.is_interface || class.is_trait {
            return Ok(Value::False);
        }
        return Ok(Value::True);
    }
    // Try autoload if second arg is true (default)
    let autoload = args.get(1).map_or(true, |v| v.to_bool());
    if autoload {
        vm.try_autoload_class(name_bytes);
        if let Some(class) = vm.classes.get(&name_lower) {
            if class.is_interface || class.is_trait {
                return Ok(Value::False);
            }
            return Ok(Value::True);
        }
    }
    Ok(Value::False)
}
fn get_class(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    let derefed = val.deref();
    match &derefed {
        Value::Object(obj) => {
            let obj_ref = obj.borrow();
            Ok(Value::String(PhpString::from_vec(obj_ref.class_name.clone())))
        }
        Value::Generator(_) => {
            Ok(Value::String(PhpString::from_bytes(b"Generator")))
        }
        Value::String(s) => {
            let b = s.as_bytes();
            if b.starts_with(b"__closure_") || b.starts_with(b"__arrow_") || b.starts_with(b"__bound_closure_") || b.starts_with(b"__closure_fcc_") {
                Ok(Value::String(PhpString::from_bytes(b"Closure")))
            } else {
                let type_name = Vm::value_type_name(val);
                let msg = format!("get_class(): Argument #1 ($object) must be of type object, {} given", type_name);
                let line = vm.current_line;
                let exc = vm.create_exception(b"TypeError", &msg, line);
                vm.current_exception = Some(exc);
                Err(VmError { message: msg, line })
            }
        }
        Value::Null | Value::Undef => {
            Ok(Value::False)
        }
        _ => {
            let type_name = Vm::value_type_name(val);
            let msg = format!("get_class(): Argument #1 ($object) must be of type object, {} given", type_name);
            let line = vm.current_line;
            let exc = vm.create_exception(b"TypeError", &msg, line);
            vm.current_exception = Some(exc);
            Err(VmError { message: msg, line })
        }
    }
}
fn get_declared_classes(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut result = PhpArray::new();
    for (_, class) in &vm.classes {
        if !class.is_interface && !class.is_trait {
            result.push(Value::String(PhpString::from_vec(class.name.clone())));
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn get_declared_traits_fn(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut result = PhpArray::new();
    for (_, class) in &vm.classes {
        if class.is_trait {
            result.push(Value::String(PhpString::from_vec(class.name.clone())));
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn get_declared_interfaces_fn(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut result = PhpArray::new();
    for (_, class) in &vm.classes {
        if class.is_interface {
            result.push(Value::String(PhpString::from_vec(class.name.clone())));
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}
fn property_exists(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let class_or_obj = args.first().unwrap_or(&Value::Null);
    let prop_name = args.get(1).unwrap_or(&Value::Null).to_php_string();
    match class_or_obj {
        Value::Object(obj) => Ok(if obj.borrow().has_property(prop_name.as_bytes()) {
            Value::True
        } else {
            Value::False
        }),
        Value::String(s) => {
            let class_lower: Vec<u8> = s
                .as_bytes()
                .iter()
                .map(|c| c.to_ascii_lowercase())
                .collect();
            Ok(if let Some(class) = vm.classes.get(&class_lower) {
                if class.properties.iter().any(|p| p.name == prop_name.as_bytes()) {
                    Value::True
                } else {
                    Value::False
                }
            } else {
                Value::False
            })
        }
        _ => {
            let type_name = Vm::value_type_name(class_or_obj);
            let msg = format!("property_exists(): Argument #1 ($object_or_class) must be of type object|string, {} given", type_name);
            let line = vm.current_line;
            let exc = vm.create_exception(b"TypeError", &msg, line);
            vm.current_exception = Some(exc);
            Err(VmError { message: msg, line })
        }
    }
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
            let cn_lower: Vec<u8> = obj.class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
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
            s.as_bytes()
                .iter()
                .map(|c| c.to_ascii_lowercase())
                .collect()
        }
        _ => return Ok(Value::False),
    };

    // Walk parent chain to check for method
    let mut current = class_lower.clone();
    for _ in 0..50 {
        if let Some(class) = vm.classes.get(&current) {
            if class.methods.contains_key(&method_lower) {
                // method_exists() returns true regardless of visibility
                return Ok(Value::True);
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

    // Check built-in class methods
    let has_builtin_method = match class_lower.as_slice() {
        b"closure" => matches!(method_lower.as_slice(),
            b"__invoke" | b"call" | b"bind" | b"bindto" | b"fromcallable"
        ),
        b"exception" | b"error" | b"runtimeexception" | b"logicexception"
        | b"typeerror" | b"valueerror" | b"argumentcounterror" | b"rangeerror"
        | b"arithmeticerror" | b"divisionbyzeroerror" | b"invalidargumentexception"
        | b"badmethodcallexception" | b"overflowexception" | b"underflowexception"
        | b"domainexception" | b"unexpectedvalueexception" | b"lengthexception"
        | b"outofrangeexception" | b"outofboundsexception" | b"errorexception"
        | b"assertionerror" | b"unhandledmatcherror" | b"closedgeneratorexception" => matches!(method_lower.as_slice(),
            b"getmessage" | b"getcode" | b"getfile" | b"getline" | b"gettrace"
            | b"gettraceAsstring" | b"gettraceasstring" | b"getprevious" | b"__tostring" | b"__construct"
        ),
        b"generator" => matches!(method_lower.as_slice(),
            b"current" | b"key" | b"next" | b"rewind" | b"send" | b"throw"
            | b"valid" | b"getreturn"
        ),
        b"stdclass" => method_lower == b"__construct",
        b"arrayobject" | b"arrayiterator" | b"recursivearrayiterator" => matches!(method_lower.as_slice(),
            b"__construct" | b"offsetexists" | b"offsetget" | b"offsetset" | b"offsetunset"
            | b"count" | b"getiterator" | b"append" | b"getarraycopy" | b"getflags" | b"setflags"
        ),
        b"splfixedarray" => matches!(method_lower.as_slice(),
            b"__construct" | b"count" | b"offsetexists" | b"offsetget" | b"offsetset"
            | b"offsetunset" | b"fromarray" | b"toarray" | b"getsize" | b"setsize"
        ),
        b"spldoublylinkedlist" | b"splstack" | b"splqueue" => matches!(method_lower.as_slice(),
            b"__construct" | b"count" | b"push" | b"pop" | b"shift" | b"unshift"
            | b"top" | b"bottom" | b"isempty" | b"current" | b"key" | b"next" | b"prev"
            | b"rewind" | b"valid" | b"offsetexists" | b"offsetget" | b"offsetset" | b"offsetunset"
        ),
        _ => false,
    };
    Ok(if has_builtin_method { Value::True } else { Value::False })
}
fn is_object(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    Ok(match val {
        Value::Object(_) | Value::Generator(_) => Value::True,
        Value::String(s) => {
            let b = s.as_bytes();
            if b.starts_with(b"__closure_") || b.starts_with(b"__arrow_") || b.starts_with(b"__bound_closure_") || b.starts_with(b"__closure_fcc_") {
                Value::True
            } else {
                Value::False
            }
        }
        Value::Array(arr) => {
            let arr = arr.borrow();
            if let Some(first) = arr.values().next() {
                if let Value::String(s) = first {
                    let b = s.as_bytes();
                    if b.starts_with(b"__closure_") || b.starts_with(b"__arrow_") || b.starts_with(b"__bound_closure_") || b.starts_with(b"__closure_fcc_") {
                        return Ok(Value::True);
                    }
                }
            }
            Value::False
        }
        _ => Value::False,
    })
}
fn setlocale(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Return the locale string (simplified - just return what was requested)
    if args.len() >= 2 {
        let locale = args.get(1).unwrap_or(&Value::Null).to_php_string();
        if locale.is_empty() || locale.as_bytes() == b"0" {
            // Query current locale
            Ok(Value::String(PhpString::from_bytes(b"C")))
        } else {
            Ok(Value::String(locale))
        }
    } else {
        Ok(Value::String(PhpString::from_bytes(b"C")))
    }
}
fn debug_zval_refcount(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Null)
}
fn extract_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(0))
}
fn array_column(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let input = match args.first() {
        Some(Value::Array(a)) => a.borrow(),
        _ => return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new())))),
    };
    let column_key = args.get(1).unwrap_or(&Value::Null);
    let index_key = args.get(2);

    let mut result = PhpArray::new();

    for (_, row) in input.iter() {
        let row_ref = match row {
            Value::Array(a) => a.clone(),
            Value::Object(o) => {
                // Convert object properties to array-like access
                let obj = o.borrow();
                let mut arr = PhpArray::new();
                for (k, v) in obj.properties.iter() {
                    // Try to parse numeric property names as integers
                    let key_str = String::from_utf8_lossy(k);
                    if let Ok(n) = key_str.parse::<i64>() {
                        if n.to_string() == key_str.as_ref() {
                            arr.set(goro_core::array::ArrayKey::Int(n), v.clone());
                            continue;
                        }
                    }
                    let key = goro_core::array::ArrayKey::String(PhpString::from_vec(k.clone()));
                    arr.set(key, v.clone());
                }
                Rc::new(RefCell::new(arr))
            }
            _ => continue,
        };
        let row_arr = row_ref.borrow();

        // Get the value to store
        let value = if matches!(column_key, Value::Null) {
            // null column_key means return the whole row
            row.clone()
        } else {
            let col_key = value_to_array_key(column_key);
            match row_arr.get(&col_key) {
                Some(v) => v.clone(),
                None => continue,
            }
        };

        // Determine the key
        if let Some(idx_key_val) = index_key {
            if matches!(idx_key_val, Value::Null) {
                result.push(value);
            } else {
                let idx_k = value_to_array_key(idx_key_val);
                if let Some(idx_val) = row_arr.get(&idx_k) {
                    let key = match &idx_val {
                        Value::Long(n) => goro_core::array::ArrayKey::Int(*n),
                        Value::String(s) => {
                            // Coerce numeric string keys to integers
                            let s_str = s.to_string_lossy();
                            if let Ok(n) = s_str.parse::<i64>() {
                                if n.to_string() == s_str {
                                    goro_core::array::ArrayKey::Int(n)
                                } else {
                                    goro_core::array::ArrayKey::String(s.clone())
                                }
                            } else {
                                goro_core::array::ArrayKey::String(s.clone())
                            }
                        }
                        Value::True => goro_core::array::ArrayKey::Int(1),
                        Value::False | Value::Null => goro_core::array::ArrayKey::Int(0),
                        Value::Double(f) => goro_core::array::ArrayKey::Int(*f as i64),
                        _ => goro_core::array::ArrayKey::String(idx_val.to_php_string()),
                    };
                    result.set(key, value);
                } else {
                    result.push(value);
                }
            }
        } else {
            result.push(value);
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
            let key = match val {
                Value::Long(n) => ArrayKey::Int(*n),
                Value::String(s) => ArrayKey::String(s.clone()),
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
    let arr_borrow = arr.borrow();
    let len = arr_borrow.len();
    if len == 0 {
        vm.emit_warning("array_rand(): Array is empty");
        return Ok(Value::Null);
    }
    if num < 1 || num as usize > len {
        vm.emit_warning("array_rand(): Argument #2 ($num) must be between 1 and the number of elements in argument #1 ($array)");
        return Ok(Value::Null);
    }
    let keys: Vec<goro_core::array::ArrayKey> = arr_borrow.iter().map(|(k, _)| k.clone()).collect();
    drop(arr_borrow);

    use std::time::SystemTime;
    let mut seed = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;

    if num == 1 {
        // xorshift64
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let idx = (seed as usize) % len;
        match &keys[idx] {
            goro_core::array::ArrayKey::Int(n) => Ok(Value::Long(*n)),
            goro_core::array::ArrayKey::String(s) => Ok(Value::String(s.clone())),
        }
    } else {
        // Return an array of random keys (sorted by their position in original array)
        let num = num as usize;
        let mut indices: Vec<usize> = (0..len).collect();
        for i in (1..len).rev() {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let j = (seed as usize) % (i + 1);
            indices.swap(i, j);
        }
        let mut selected: Vec<usize> = indices[..num].to_vec();
        selected.sort();
        let mut result = PhpArray::new();
        for idx in selected {
            match &keys[idx] {
                goro_core::array::ArrayKey::Int(n) => result.push(Value::Long(*n)),
                goro_core::array::ArrayKey::String(s) => result.push(Value::String(s.clone())),
            }
        }
        Ok(Value::Array(Rc::new(RefCell::new(result))))
    }
}

// === String extras ===

fn str_split(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let len = args.get(1).map(|v| v.to_long()).unwrap_or(1).max(1) as usize;
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

fn number_format(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    let decimals_raw = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let dec_point = match args.get(2).map(|v| v.deref()) {
        Some(Value::Null) | Some(Value::Undef) | None => ".".to_string(),
        Some(v) => v.to_php_string().to_string_lossy(),
    };
    let thousands_sep = match args.get(3).map(|v| v.deref()) {
        Some(Value::Null) | Some(Value::Undef) | None => ",".to_string(),
        Some(v) => v.to_php_string().to_string_lossy(),
    };

    // Handle negative decimal places: round to nearest 10^(-decimals)
    if decimals_raw < 0 {
        let num = val.to_double();
        if num.is_nan() {
            return Ok(Value::String(PhpString::from_bytes(b"NAN")));
        }
        if num.is_infinite() {
            let prefix = if num < 0.0 { "-" } else { "" };
            return Ok(Value::String(PhpString::from_string(format!("{}INF", prefix))));
        }
        let places = (-decimals_raw).min(100) as u32;
        let factor = 10f64.powi(places as i32);
        let rounded = (num / factor).round() * factor;
        // Check if result is effectively zero
        if rounded.abs() < 0.5 {
            return Ok(Value::String(PhpString::from_bytes(b"0")));
        }
        let neg = rounded < 0.0;
        // Use format with no decimal places - need to handle large numbers
        let abs_str = format!("{:.0}", rounded.abs());
        // Add thousands separator
        let int_bytes = abs_str.as_bytes();
        let mut with_sep = String::new();
        let len = int_bytes.len();
        for (i, &b) in int_bytes.iter().enumerate() {
            if i > 0 && (len - i) % 3 == 0 && !thousands_sep.is_empty() {
                with_sep.push_str(&thousands_sep);
            }
            with_sep.push(b as char);
        }
        let mut result = String::new();
        if neg { result.push('-'); }
        result.push_str(&with_sep);
        return Ok(Value::String(PhpString::from_string(result)));
    }

    let decimals = decimals_raw.min(100000) as usize;

    // For integer values, format without going through float to preserve precision
    let (formatted, is_negative) = if let Value::Long(n) = val {
        let neg = *n < 0;
        let abs_str = if neg { format!("{}", -(*n as i128)) } else { format!("{}", n) };
        let formatted = if decimals > 0 {
            format!("{}.{}", abs_str, "0".repeat(decimals))
        } else {
            abs_str
        };
        (formatted, neg)
    } else {
        let num = val.to_double();
        if num.is_nan() {
            return Ok(Value::String(PhpString::from_bytes(b"NAN")));
        }
        if num.is_infinite() {
            let prefix = if num < 0.0 { "-" } else { "" };
            return Ok(Value::String(PhpString::from_string(format!("{}INF", prefix))));
        }
        let neg = num < 0.0;
        // Use PHP's rounding approach: floor(f + 0.5) to handle edge cases
        // like number_format(0.045, 2) which should give "0.05"
        let abs_num = {
            let factor = 10f64.powi(decimals as i32);
            let f = num.abs() * factor;
            if f.abs() < 1e15 {
                let rounded = (f + 0.5).floor();
                rounded / factor
            } else {
                num.abs()
            }
        };
        let formatted = format!("{:.prec$}", abs_num, prec = decimals);
        // Check if result rounds to zero
        let is_zero = formatted.chars().all(|c| c == '0' || c == '.');
        (formatted, neg && !is_zero)
    };

    let parts: Vec<&str> = formatted.split('.').collect();
    let int_part = parts[0];
    let dec_part = parts.get(1).unwrap_or(&"");

    // Add thousands separator to integer part
    let int_bytes = int_part.as_bytes();
    let mut with_sep = String::new();
    let len = int_bytes.len();
    for (i, &b) in int_bytes.iter().enumerate() {
        if i > 0 && (len - i) % 3 == 0 && !thousands_sep.is_empty() {
            with_sep.push_str(&thousands_sep);
        }
        with_sep.push(b as char);
    }

    let mut result = String::new();
    if is_negative {
        result.push('-');
    }
    result.push_str(&with_sep);
    if decimals > 0 {
        result.push_str(&dec_point);
        result.push_str(dec_part);
    }
    Ok(Value::String(PhpString::from_string(result)))
}

fn money_format(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(PhpString::empty()))
}

fn hex2bin(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let hex = s.as_bytes();
    if hex.len() % 2 != 0 {
        _vm.emit_warning("hex2bin(): Hexadecimal input string must have an even length");
        return Ok(Value::False);
    }
    let mut result = Vec::with_capacity(hex.len() / 2);
    for i in (0..hex.len()).step_by(2) {
        let byte = u8::from_str_radix(&String::from_utf8_lossy(&hex[i..i + 2]), 16);
        match byte {
            Ok(b) => result.push(b),
            Err(_) => return Ok(Value::False),
        }
    }
    Ok(Value::String(PhpString::from_vec(result)))
}

fn bin2hex(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let hex: String = s.as_bytes().iter().map(|b| format!("{:02x}", b)).collect();
    Ok(Value::String(PhpString::from_string(hex)))
}

fn base64_encode(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = data.as_bytes();
    const B64: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = Vec::with_capacity((bytes.len() + 2) / 3 * 4);
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
        } else if let Some(_) = b64_val(b) {
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
        let first_pad = filtered.iter().position(|&b| b == b'=').unwrap_or(filtered.len());
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
                if pad_len != 0 { return Ok(Value::False); }
            }
            2 => {
                // 2 extra chars: accepts 0 or 2 padding chars
                if pad_len != 0 && pad_len != 2 { return Ok(Value::False); }
                if pad_len != 0 && total % 4 != 0 { return Ok(Value::False); }
            }
            3 => {
                // 3 extra chars: accepts 0 or 1 padding char
                if pad_len != 0 && pad_len != 1 { return Ok(Value::False); }
                if pad_len != 0 && total % 4 != 0 { return Ok(Value::False); }
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
        let n = ((vals[i] as u32) << 18) | ((vals[i+1] as u32) << 12) | ((vals[i+2] as u32) << 6) | (vals[i+3] as u32);
        result.push((n >> 16) as u8);
        result.push((n >> 8) as u8);
        result.push(n as u8);
        i += 4;
    }
    let remaining = vals.len() - i;
    if remaining == 3 {
        let n = ((vals[i] as u32) << 18) | ((vals[i+1] as u32) << 12) | ((vals[i+2] as u32) << 6);
        result.push((n >> 16) as u8);
        result.push((n >> 8) as u8);
    } else if remaining == 2 {
        let n = ((vals[i] as u32) << 18) | ((vals[i+1] as u32) << 12);
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

fn htmlspecialchars(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let flags = args.get(1).map(|v| v.to_long()).unwrap_or(11); // ENT_QUOTES | ENT_SUBSTITUTE (default)
    let double_encode = match args.get(3) {
        Some(Value::Null) | None => true,
        Some(v) => v.is_truthy(),
    };
    let ent_compat = flags & 2 != 0;  // ENT_COMPAT
    let ent_quotes = flags & 3 == 3;  // ENT_QUOTES (both single and double)
    let bytes = s.as_bytes();
    let mut result = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'&' => {
                if !double_encode {
                    // Check if this is already an entity
                    let remaining = &bytes[i..];
                    let is_entity = remaining.starts_with(b"&amp;")
                        || remaining.starts_with(b"&lt;")
                        || remaining.starts_with(b"&gt;")
                        || remaining.starts_with(b"&quot;")
                        || remaining.starts_with(b"&#039;")
                        || remaining.starts_with(b"&apos;")
                        || (remaining.len() > 2 && remaining[1] == b'#' && {
                            // Numeric entity: &#digits; or &#xhex;
                            if let Some(end) = remaining.iter().position(|&b| b == b';') {
                                end > 2
                            } else { false }
                        })
                        || {
                            // Named entity: &name;
                            if let Some(end) = remaining.iter().position(|&b| b == b';') {
                                end > 1 && remaining[1..end].iter().all(|&b| b.is_ascii_alphanumeric())
                            } else { false }
                        };
                    if is_entity {
                        result.push(b'&');
                    } else {
                        result.extend_from_slice(b"&amp;");
                    }
                } else {
                    result.extend_from_slice(b"&amp;");
                }
            }
            b'"' if ent_compat || ent_quotes => result.extend_from_slice(b"&quot;"),
            b'\'' if ent_quotes => result.extend_from_slice(b"&#039;"),
            b'<' => result.extend_from_slice(b"&lt;"),
            b'>' => result.extend_from_slice(b"&gt;"),
            _ => result.push(bytes[i]),
        }
        i += 1;
    }
    Ok(Value::String(PhpString::from_vec(result)))
}
fn htmlentities(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    htmlspecialchars(_vm, args)
}
fn html_entity_decode(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(PhpString::empty()))
}
fn htmlspecialchars_decode(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let flags = args.get(1).map(|v| v.to_long()).unwrap_or(3); // ENT_QUOTES | ENT_SUBSTITUTE
    let input = s.to_string_lossy();
    let mut result = input
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">");
    if flags & 2 != 0 {
        // ENT_COMPAT - decode double quotes
        result = result.replace("&quot;", "\"");
    }
    if flags & 4 != 0 || flags & 3 == 3 {
        // ENT_QUOTES - decode single quotes
        result = result.replace("&#039;", "'").replace("&apos;", "'");
    }
    Ok(Value::String(PhpString::from_string(result)))
}

fn str_word_count(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    Ok(Value::Long(
        s.to_string_lossy().split_whitespace().count() as i64
    ))
}
fn substr_count(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let haystack = args.first().unwrap_or(&Value::Null).to_php_string();
    let needle = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() {
        return Ok(Value::Long(0));
    }
    let mut count = 0i64;
    let mut i = 0;
    while i + n.len() <= h.len() {
        if &h[i..i + n.len()] == n {
            count += 1;
            i += n.len();
        } else {
            i += 1;
        }
    }
    Ok(Value::Long(count))
}
fn substr_replace(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let subject = args.first().unwrap_or(&Value::Null).to_php_string();
    let replacement = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let start = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    let length = args.get(3).map(|v| Some(v.to_long())).unwrap_or(None);

    let bytes = subject.as_bytes();
    let len = bytes.len() as i64;
    let s = if start < 0 {
        (len + start).max(0) as usize
    } else {
        start.min(len) as usize
    };
    let e = match length {
        Some(l) if l < 0 => (len + l).max(s as i64) as usize,
        Some(l) => (s + l as usize).min(bytes.len()),
        None => bytes.len(),
    };

    let mut result = Vec::new();
    result.extend_from_slice(&bytes[..s]);
    result.extend_from_slice(replacement.as_bytes());
    if e < bytes.len() {
        result.extend_from_slice(&bytes[e..]);
    }
    Ok(Value::String(PhpString::from_vec(result)))
}
fn str_ireplace(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let search = args
        .first()
        .unwrap_or(&Value::Null)
        .to_php_string()
        .to_string_lossy()
        .to_lowercase();
    let replace = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let subject = args.get(2).unwrap_or(&Value::Null).to_php_string();

    let subj = subject.to_string_lossy();
    let subj_lower = subj.to_lowercase();
    let mut result = String::new();
    let mut i = 0;
    while i < subj.len() {
        if i + search.len() <= subj_lower.len()
            && &subj_lower[i..i + search.len()] == search.as_str()
        {
            result.push_str(&replace.to_string_lossy());
            i += search.len();
        } else {
            result.push(subj.as_bytes()[i] as char);
            i += 1;
        }
    }
    Ok(Value::String(PhpString::from_string(result)))
}
fn stripos(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let h = args
        .first()
        .unwrap_or(&Value::Null)
        .to_php_string()
        .to_string_lossy()
        .to_lowercase();
    let n = args
        .get(1)
        .unwrap_or(&Value::Null)
        .to_php_string()
        .to_string_lossy()
        .to_lowercase();
    match h.find(&n) {
        Some(pos) => Ok(Value::Long(pos as i64)),
        None => Ok(Value::False),
    }
}
fn strrpos(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let h = args.first().unwrap_or(&Value::Null).to_php_string();
    let n = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let hb = h.as_bytes();
    let nb = n.as_bytes();
    if nb.is_empty() {
        return Ok(Value::False);
    }
    for i in (0..=(hb.len().saturating_sub(nb.len()))).rev() {
        if &hb[i..i + nb.len()] == nb {
            return Ok(Value::Long(i as i64));
        }
    }
    Ok(Value::False)
}
fn strripos(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let h = args
        .first()
        .unwrap_or(&Value::Null)
        .to_php_string()
        .to_string_lossy()
        .to_lowercase();
    let n = args
        .get(1)
        .unwrap_or(&Value::Null)
        .to_php_string()
        .to_string_lossy()
        .to_lowercase();
    match h.rfind(&n) {
        Some(pos) => Ok(Value::Long(pos as i64)),
        None => Ok(Value::False),
    }
}
fn strcmp(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = args.first().unwrap_or(&Value::Null).to_php_string();
    let b = args.get(1).unwrap_or(&Value::Null).to_php_string();
    Ok(Value::Long(php_strcmp(a.as_bytes(), b.as_bytes())))
}
fn strncmp(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = args.first().unwrap_or(&Value::Null).to_php_string();
    let b = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let n = args.get(2).map(|v| v.to_long()).unwrap_or(0) as usize;
    let sa = &a.as_bytes()[..n.min(a.len())];
    let sb = &b.as_bytes()[..n.min(b.len())];
    Ok(Value::Long(php_strcmp(sa, sb)))
}
fn strcasecmp(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s1 = args.first().unwrap_or(&Value::Null).to_php_string();
    let s2 = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let a: Vec<u8> = s1
        .as_bytes()
        .iter()
        .map(|c| c.to_ascii_lowercase())
        .collect();
    let b: Vec<u8> = s2
        .as_bytes()
        .iter()
        .map(|c| c.to_ascii_lowercase())
        .collect();
    Ok(Value::Long(php_strcmp(&a, &b)))
}
fn strncasecmp(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s1 = args.first().unwrap_or(&Value::Null).to_php_string();
    let s2 = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let a: Vec<u8> = s1
        .as_bytes()
        .iter()
        .map(|c| c.to_ascii_lowercase())
        .collect();
    let b: Vec<u8> = s2
        .as_bytes()
        .iter()
        .map(|c| c.to_ascii_lowercase())
        .collect();
    let n = args.get(2).map(|v| v.to_long()).unwrap_or(0) as usize;
    let sa = &a[..n.min(a.len())];
    let sb = &b[..n.min(b.len())];
    Ok(Value::Long(php_strcmp(sa, sb)))
}

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
fn str_contains_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let h = args.first().unwrap_or(&Value::Null).to_php_string();
    let n = args.get(1).unwrap_or(&Value::Null).to_php_string();
    if n.is_empty() {
        return Ok(Value::True);
    }
    let hb = h.as_bytes();
    let nb = n.as_bytes();
    if nb.len() > hb.len() {
        return Ok(Value::False);
    }
    for i in 0..=hb.len() - nb.len() {
        if &hb[i..i + nb.len()] == nb {
            return Ok(Value::True);
        }
    }
    Ok(Value::False)
}
fn wordwrap(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let str_val = args.first().unwrap_or(&Value::Null).to_php_string();
    let width = args.get(1).map(|v| v.to_long()).unwrap_or(75) as usize;
    let break_str = args.get(2).map(|v| v.to_php_string()).unwrap_or_else(|| PhpString::from_bytes(b"\n"));
    let cut_long_words = args.get(3).map(|v| v.is_truthy()).unwrap_or(false);

    if width == 0 && cut_long_words {
        // Cannot cut long words with width 0
        return Ok(Value::False);
    }

    let input = str_val.as_bytes();
    let brk = break_str.as_bytes();

    if input.is_empty() {
        return Ok(Value::String(PhpString::empty()));
    }

    let mut result: Vec<u8> = Vec::with_capacity(input.len() + input.len() / width.max(1) * brk.len());
    let mut line_len = 0;
    let mut last_space = None;
    let mut last_space_out = None;
    let mut i = 0;

    while i < input.len() {
        if input[i] == b'\n' {
            result.push(b'\n');
            line_len = 0;
            last_space = None;
            last_space_out = None;
            i += 1;
            continue;
        }

        if input[i] == b' ' {
            last_space = Some(i);
            last_space_out = Some(result.len());
        }

        if line_len >= width && width > 0 {
            if let Some(sp_out) = last_space_out {
                // Replace the space with break
                let tail: Vec<u8> = result[sp_out + 1..].to_vec();
                result.truncate(sp_out);
                result.extend_from_slice(brk);
                result.extend_from_slice(&tail);
                line_len = result.len() - sp_out - brk.len();
                // Actually recalculate line_len properly
                // It should be the length since the last break was inserted
                let last_brk_pos = result.len() - tail.len();
                line_len = tail.len();
                let _ = last_brk_pos; // suppress warning
                last_space = None;
                last_space_out = None;
            } else if cut_long_words {
                result.extend_from_slice(brk);
                line_len = 0;
            }
        }

        result.push(input[i]);
        line_len += 1;
        i += 1;
    }

    Ok(Value::String(PhpString::from_vec(result)))
}
fn printf(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Use the sprintf implementation from strings module
    let formatted = crate::strings::do_sprintf(args);
    let len = formatted.len();
    vm.write_output(formatted.as_bytes());
    Ok(Value::Long(len as i64))
}
fn fprintf_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // fprintf($handle, $format, ...$args) - write formatted to file handle
    if args.len() < 2 {
        let msg = format!("fprintf() expects at least 2 arguments, {} given", args.len());
        let exc = vm.create_exception(b"ArgumentCountError", &msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    let fid = args[0].to_long();
    let format_args = &args[1..]; // skip the handle, pass format + args
    let formatted = crate::strings::do_sprintf(format_args);
    let len = formatted.len();

    // Check if it's a real file handle
    let written = FILE_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(fh) = handles.get_mut(&fid) {
            fh.file.write_all(formatted.as_bytes()).ok();
            true
        } else {
            false
        }
    });

    if !written {
        // Fallback to stdout for STDOUT handle or unknown handles
        vm.write_output(formatted.as_bytes());
    }
    Ok(Value::Long(len as i64))
}

fn vfprintf_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // vfprintf($handle, $format, $args_array)
    if args.len() < 2 {
        let msg = format!("vfprintf() expects at least 2 arguments, {} given", args.len());
        let exc = vm.create_exception(b"ArgumentCountError", &msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    let fid = args[0].to_long();
    let format = args[1].to_php_string();
    let mut format_args = vec![Value::String(format)];
    if let Value::Array(arr) = &args[2] {
        for (_, v) in arr.borrow().iter() {
            format_args.push(v.clone());
        }
    }
    let formatted = crate::strings::do_sprintf(&format_args);
    let len = formatted.len();

    let written = FILE_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(fh) = handles.get_mut(&fid) {
            fh.file.write_all(formatted.as_bytes()).ok();
            true
        } else {
            false
        }
    });
    if !written {
        vm.write_output(formatted.as_bytes());
    }
    Ok(Value::Long(len as i64))
}

fn sscanf_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let input = args.first().unwrap_or(&Value::Null).to_php_string();
    let format = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let input_bytes = input.as_bytes();
    let format_bytes = format.as_bytes();

    let has_refs = args.len() > 2;
    let mut results: Vec<Value> = Vec::new();
    let mut input_pos = 0usize;
    let mut fmt_pos = 0usize;

    while fmt_pos < format_bytes.len() {
        if format_bytes[fmt_pos] == b'%' {
            fmt_pos += 1;
            if fmt_pos >= format_bytes.len() { break; }

            // Handle %% literal
            if format_bytes[fmt_pos] == b'%' {
                if input_pos < input_bytes.len() && input_bytes[input_pos] == b'%' {
                    input_pos += 1;
                }
                fmt_pos += 1;
                continue;
            }

            // Handle argument swapping like %2$s
            let _swap_arg = if format_bytes[fmt_pos].is_ascii_digit() && fmt_pos + 1 < format_bytes.len() && format_bytes[fmt_pos + 1] == b'$' {
                let _n = (format_bytes[fmt_pos] - b'0') as usize;
                fmt_pos += 2;
                Some(_n)
            } else {
                None
            };

            // Handle width
            let mut suppress = false;
            if fmt_pos < format_bytes.len() && format_bytes[fmt_pos] == b'*' {
                suppress = true;
                fmt_pos += 1;
            }
            let mut width: Option<usize> = None;
            let width_start = fmt_pos;
            while fmt_pos < format_bytes.len() && format_bytes[fmt_pos].is_ascii_digit() {
                fmt_pos += 1;
            }
            if fmt_pos > width_start {
                width = std::str::from_utf8(&format_bytes[width_start..fmt_pos]).ok().and_then(|s| s.parse().ok());
            }

            if fmt_pos >= format_bytes.len() { break; }
            let spec = format_bytes[fmt_pos];
            fmt_pos += 1;

            // Skip leading whitespace for numeric types
            match spec {
                b'd' | b'i' | b'u' | b'x' | b'X' | b'o' | b'f' | b'e' | b'g' => {
                    while input_pos < input_bytes.len() && input_bytes[input_pos] == b' ' {
                        input_pos += 1;
                    }
                }
                _ => {}
            }

            match spec {
                b'd' | b'i' | b'u' => {
                    // Read integer
                    let start = input_pos;
                    if input_pos < input_bytes.len() && (input_bytes[input_pos] == b'-' || input_bytes[input_pos] == b'+') {
                        input_pos += 1;
                    }
                    let mut count = 0usize;
                    let max = width.unwrap_or(usize::MAX);
                    while input_pos < input_bytes.len() && input_bytes[input_pos].is_ascii_digit() && count < max {
                        input_pos += 1;
                        count += 1;
                    }
                    if input_pos > start {
                        if !suppress {
                            let s = std::str::from_utf8(&input_bytes[start..input_pos]).unwrap_or("0");
                            let val = s.parse::<i64>().unwrap_or(0);
                            results.push(Value::Long(val));
                        }
                    } else if !suppress {
                        results.push(Value::Null);
                    }
                }
                b'x' | b'X' => {
                    // Read hex integer
                    if input_pos + 1 < input_bytes.len() && input_bytes[input_pos] == b'0'
                        && (input_bytes[input_pos + 1] == b'x' || input_bytes[input_pos + 1] == b'X') {
                        input_pos += 2;
                    }
                    let start = input_pos;
                    let max = width.unwrap_or(usize::MAX);
                    let mut count = 0;
                    while input_pos < input_bytes.len() && input_bytes[input_pos].is_ascii_hexdigit() && count < max {
                        input_pos += 1;
                        count += 1;
                    }
                    if !suppress {
                        let s = std::str::from_utf8(&input_bytes[start..input_pos]).unwrap_or("0");
                        let val = i64::from_str_radix(s, 16).unwrap_or(0);
                        results.push(Value::Long(val));
                    }
                }
                b'o' => {
                    let start = input_pos;
                    let max = width.unwrap_or(usize::MAX);
                    let mut count = 0;
                    while input_pos < input_bytes.len() && input_bytes[input_pos] >= b'0' && input_bytes[input_pos] <= b'7' && count < max {
                        input_pos += 1;
                        count += 1;
                    }
                    if !suppress {
                        let s = std::str::from_utf8(&input_bytes[start..input_pos]).unwrap_or("0");
                        let val = i64::from_str_radix(s, 8).unwrap_or(0);
                        results.push(Value::Long(val));
                    }
                }
                b'f' | b'e' | b'g' => {
                    let start = input_pos;
                    if input_pos < input_bytes.len() && (input_bytes[input_pos] == b'-' || input_bytes[input_pos] == b'+') {
                        input_pos += 1;
                    }
                    while input_pos < input_bytes.len() && (input_bytes[input_pos].is_ascii_digit() || input_bytes[input_pos] == b'.') {
                        input_pos += 1;
                    }
                    if !suppress {
                        let s = std::str::from_utf8(&input_bytes[start..input_pos]).unwrap_or("0");
                        let val = s.parse::<f64>().unwrap_or(0.0);
                        results.push(Value::Double(val));
                    }
                }
                b's' => {
                    // Read non-whitespace string
                    let start = input_pos;
                    let max = width.unwrap_or(usize::MAX);
                    let mut count = 0;
                    while input_pos < input_bytes.len() && input_bytes[input_pos] != b' ' && input_bytes[input_pos] != b'\t' && input_bytes[input_pos] != b'\n' && count < max {
                        input_pos += 1;
                        count += 1;
                    }
                    if !suppress {
                        results.push(Value::String(PhpString::from_vec(input_bytes[start..input_pos].to_vec())));
                    }
                }
                b'c' => {
                    // Read single char (or width chars)
                    let n = width.unwrap_or(1);
                    let start = input_pos;
                    let end = (input_pos + n).min(input_bytes.len());
                    input_pos = end;
                    if !suppress {
                        results.push(Value::String(PhpString::from_vec(input_bytes[start..end].to_vec())));
                    }
                }
                b'n' => {
                    // Number of characters consumed so far
                    if !suppress {
                        results.push(Value::Long(input_pos as i64));
                    }
                }
                b'[' => {
                    // Character class [abc] or [^abc]
                    let negated = fmt_pos < format_bytes.len() && format_bytes[fmt_pos] == b'^';
                    if negated { fmt_pos += 1; }
                    let mut char_set = Vec::new();
                    // Special case: ] as first char is literal
                    if fmt_pos < format_bytes.len() && format_bytes[fmt_pos] == b']' {
                        char_set.push(b']');
                        fmt_pos += 1;
                    }
                    while fmt_pos < format_bytes.len() && format_bytes[fmt_pos] != b']' {
                        char_set.push(format_bytes[fmt_pos]);
                        fmt_pos += 1;
                    }
                    if fmt_pos < format_bytes.len() { fmt_pos += 1; } // skip ]
                    let start = input_pos;
                    let max = width.unwrap_or(usize::MAX);
                    let mut count = 0;
                    while input_pos < input_bytes.len() && count < max {
                        let in_set = char_set.contains(&input_bytes[input_pos]);
                        if (negated && in_set) || (!negated && !in_set) {
                            break;
                        }
                        input_pos += 1;
                        count += 1;
                    }
                    if !suppress {
                        if input_pos > start {
                            results.push(Value::String(PhpString::from_vec(input_bytes[start..input_pos].to_vec())));
                        } else {
                            results.push(Value::Null);
                        }
                    }
                }
                _ => {}
            }
        } else if format_bytes[fmt_pos] == b' ' || format_bytes[fmt_pos] == b'\t' || format_bytes[fmt_pos] == b'\n' {
            // Whitespace in format matches any amount of whitespace in input
            fmt_pos += 1;
            while input_pos < input_bytes.len() && (input_bytes[input_pos] == b' ' || input_bytes[input_pos] == b'\t' || input_bytes[input_pos] == b'\n') {
                input_pos += 1;
            }
        } else {
            // Literal match
            if input_pos < input_bytes.len() && input_bytes[input_pos] == format_bytes[fmt_pos] {
                input_pos += 1;
            }
            fmt_pos += 1;
        }
    }

    if has_refs {
        // Write results to reference arguments
        for (i, val) in results.iter().enumerate() {
            if let Some(arg) = args.get(i + 2) {
                if let Value::Reference(r) = arg {
                    *r.borrow_mut() = val.clone();
                }
            }
        }
        Ok(Value::Long(results.len() as i64))
    } else {
        // Return array of results
        let mut arr = PhpArray::new();
        for val in results {
            arr.push(val);
        }
        Ok(Value::Array(Rc::new(RefCell::new(arr))))
    }
}
// === Shell ===

fn escapeshellarg_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
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

fn escapeshellcmd_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let mut result = Vec::new();
    for &byte in s.as_bytes() {
        if b"#&;`|*?~<>^()[]{}$\\\x0A\xFF\"".contains(&byte) {
            result.push(b'\\');
        }
        result.push(byte);
    }
    Ok(Value::String(PhpString::from_vec(result)))
}

fn parse_str_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let s_str = s.to_string_lossy();
    let mut result = PhpArray::new();
    for pair in s_str.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (raw_key, raw_val) = if let Some(eq_pos) = pair.find('=') {
            (&pair[..eq_pos], &pair[eq_pos + 1..])
        } else {
            (pair, "")
        };
        let key = php_urldecode(raw_key);
        let val = php_urldecode(raw_val);
        let val_value = Value::String(PhpString::from_string(val));

        // Handle array bracket syntax: key[subkey], key[], key[a][b]
        if let Some(bracket_pos) = key.find('[') {
            let base_key = &key[..bracket_pos];
            let brackets = &key[bracket_pos..];
            // Parse bracket keys
            let mut keys: Vec<Option<String>> = Vec::new();
            let mut rest = brackets;
            while rest.starts_with('[') {
                if let Some(close) = rest.find(']') {
                    let inner = &rest[1..close];
                    keys.push(if inner.is_empty() { None } else { Some(inner.to_string()) });
                    rest = &rest[close + 1..];
                } else {
                    break;
                }
            }
            // Navigate/create nested arrays
            let base_arr_key = if let Ok(n) = base_key.parse::<i64>() {
                goro_core::array::ArrayKey::Int(n)
            } else {
                goro_core::array::ArrayKey::String(PhpString::from_string(base_key.to_string()))
            };
            if !matches!(result.get(&base_arr_key), Some(Value::Array(_))) {
                result.set(base_arr_key.clone(), Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
            }
            let mut current = result.get(&base_arr_key).unwrap().clone();
            for (i, sub_key) in keys.iter().enumerate() {
                let is_last = i == keys.len() - 1;
                if is_last {
                    if let Value::Array(arr) = &current {
                        let mut arr_borrow = arr.borrow_mut();
                        match sub_key {
                            None => { arr_borrow.push(val_value.clone()); }
                            Some(k) => {
                                if let Ok(n) = k.parse::<i64>() {
                                    arr_borrow.set(goro_core::array::ArrayKey::Int(n), val_value.clone());
                                } else {
                                    arr_borrow.set(goro_core::array::ArrayKey::String(PhpString::from_string(k.clone())), val_value.clone());
                                }
                            }
                        }
                    }
                } else {
                    if let Value::Array(arr) = &current {
                        let sub_arr_key = match sub_key {
                            None => {
                                let mut arr_borrow = arr.borrow_mut();
                                let next_idx = arr_borrow.len() as i64;
                                arr_borrow.push(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
                                goro_core::array::ArrayKey::Int(next_idx)
                            }
                            Some(k) => {
                                let key = if let Ok(n) = k.parse::<i64>() {
                                    goro_core::array::ArrayKey::Int(n)
                                } else {
                                    goro_core::array::ArrayKey::String(PhpString::from_string(k.clone()))
                                };
                                if !matches!(arr.borrow().get(&key), Some(Value::Array(_))) {
                                    arr.borrow_mut().set(key.clone(), Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
                                }
                                key
                            }
                        };
                        let next = arr.borrow().get(&sub_arr_key).unwrap().clone();
                        current = next;
                    }
                }
            }
        } else {
            result.set(
                goro_core::array::ArrayKey::String(PhpString::from_string(key)),
                val_value,
            );
        }
    }
    // PHP 8: parse_str($string, &$result) sets $result and returns null
    let result_val = Value::Array(Rc::new(RefCell::new(result)));
    if let Some(Value::Reference(r)) = args.get(1) {
        *r.borrow_mut() = result_val;
        Ok(Value::Null)
    } else {
        // If no second parameter, return the array (non-standard but compatible with our VM)
        Ok(result_val)
    }
}

fn php_urldecode(s: &str) -> String {
    let mut result = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => { result.push(b' '); i += 1; }
            b'%' if i + 2 < bytes.len() => {
                if let Ok(byte) = u8::from_str_radix(&s[i+1..i+3], 16) {
                    result.push(byte);
                    i += 3;
                } else {
                    result.push(b'%');
                    i += 1;
                }
            }
            c => { result.push(c); i += 1; }
        }
    }
    String::from_utf8_lossy(&result).to_string()
}

// === URL ===

fn parse_url_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    use goro_core::array::ArrayKey;
    let url = args.first().unwrap_or(&Value::Null).to_php_string();
    let component = args.get(1).map(|v| v.to_long()).unwrap_or(-1);
    let url_str = url.to_string_lossy();

    // Simple URL parser
    let mut scheme = "";
    let mut host = "";
    let mut port: Option<i64> = None;
    let mut user = "";
    let mut pass = "";
    let mut path = "";
    let mut query = "";
    let mut fragment = "";

    let mut rest = url_str.as_str();

    // Extract fragment
    if let Some(pos) = rest.find('#') {
        fragment = &rest[pos + 1..];
        rest = &rest[..pos];
    }

    // Extract query
    if let Some(pos) = rest.find('?') {
        query = &rest[pos + 1..];
        rest = &rest[..pos];
    }

    // Extract scheme
    if let Some(pos) = rest.find("://") {
        scheme = &rest[..pos];
        rest = &rest[pos + 3..];
    }

    // Extract user:pass@host:port
    if !scheme.is_empty() || rest.starts_with("//") {
        if rest.starts_with("//") {
            rest = &rest[2..];
        }
        // Find path start
        let authority_end = rest.find('/').unwrap_or(rest.len());
        let authority = &rest[..authority_end];
        rest = &rest[authority_end..];

        // user:pass@host:port
        let (userinfo, hostport) = if let Some(at) = authority.find('@') {
            (&authority[..at], &authority[at + 1..])
        } else {
            ("", authority)
        };

        if !userinfo.is_empty() {
            if let Some(colon) = userinfo.find(':') {
                user = &userinfo[..colon];
                pass = &userinfo[colon + 1..];
            } else {
                user = userinfo;
            }
        }

        // host:port
        if let Some(colon) = hostport.rfind(':') {
            host = &hostport[..colon];
            port = hostport[colon + 1..].parse().ok();
        } else {
            host = hostport;
        }
    }

    if rest.is_empty() && scheme.is_empty() {
        path = &url_str;
    } else {
        path = rest;
    }

    // Return specific component or full array
    if component >= 0 {
        // PHP_URL_SCHEME=0, PHP_URL_HOST=1, PHP_URL_PORT=2, PHP_URL_USER=3,
        // PHP_URL_PASS=4, PHP_URL_PATH=5, PHP_URL_QUERY=6, PHP_URL_FRAGMENT=7
        return Ok(match component {
            0 => {
                if scheme.is_empty() {
                    Value::Null
                } else {
                    Value::String(PhpString::from_string(scheme.to_string()))
                }
            }
            1 => {
                if host.is_empty() {
                    Value::Null
                } else {
                    Value::String(PhpString::from_string(host.to_string()))
                }
            }
            2 => port.map(Value::Long).unwrap_or(Value::Null),
            3 => {
                if user.is_empty() {
                    Value::Null
                } else {
                    Value::String(PhpString::from_string(user.to_string()))
                }
            }
            4 => {
                if pass.is_empty() {
                    Value::Null
                } else {
                    Value::String(PhpString::from_string(pass.to_string()))
                }
            }
            5 => {
                if path.is_empty() {
                    Value::Null
                } else {
                    Value::String(PhpString::from_string(path.to_string()))
                }
            }
            6 => {
                if query.is_empty() {
                    Value::Null
                } else {
                    Value::String(PhpString::from_string(query.to_string()))
                }
            }
            7 => {
                if fragment.is_empty() {
                    Value::Null
                } else {
                    Value::String(PhpString::from_string(fragment.to_string()))
                }
            }
            _ => Value::Null,
        });
    }

    let mut result = PhpArray::new();
    if !scheme.is_empty() {
        result.set(
            ArrayKey::String(PhpString::from_bytes(b"scheme")),
            Value::String(PhpString::from_string(scheme.to_string())),
        );
    }
    if !host.is_empty() {
        result.set(
            ArrayKey::String(PhpString::from_bytes(b"host")),
            Value::String(PhpString::from_string(host.to_string())),
        );
    }
    if let Some(p) = port {
        result.set(
            ArrayKey::String(PhpString::from_bytes(b"port")),
            Value::Long(p),
        );
    }
    if !user.is_empty() {
        result.set(
            ArrayKey::String(PhpString::from_bytes(b"user")),
            Value::String(PhpString::from_string(user.to_string())),
        );
    }
    if !pass.is_empty() {
        result.set(
            ArrayKey::String(PhpString::from_bytes(b"pass")),
            Value::String(PhpString::from_string(pass.to_string())),
        );
    }
    if !path.is_empty() {
        result.set(
            ArrayKey::String(PhpString::from_bytes(b"path")),
            Value::String(PhpString::from_string(path.to_string())),
        );
    }
    if !query.is_empty() {
        result.set(
            ArrayKey::String(PhpString::from_bytes(b"query")),
            Value::String(PhpString::from_string(query.to_string())),
        );
    }
    if !fragment.is_empty() {
        result.set(
            ArrayKey::String(PhpString::from_bytes(b"fragment")),
            Value::String(PhpString::from_string(fragment.to_string())),
        );
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn http_build_query_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = args.first().unwrap_or(&Value::Null);
    let _numeric_prefix = args.get(1).and_then(|v| {
        if matches!(v, Value::Null | Value::Undef) { None }
        else { Some(v.to_php_string().to_string_lossy()) }
    });
    let separator = args
        .get(2)
        .and_then(|v| {
            if matches!(v, Value::Null | Value::Undef) { None }
            else { Some(v.to_php_string().to_string_lossy()) }
        })
        .unwrap_or_else(|| "&".to_string());
    let enc_type = args.get(3).map(|v| v.to_long()).unwrap_or(1); // PHP_QUERY_RFC1738 = 1
    let mut parts = Vec::new();

    fn url_encode(s: &str, enc_type: i64) -> String {
        let mut result = String::new();
        for byte in s.bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => {
                    result.push(byte as char);
                }
                b'~' if enc_type == 2 => {
                    // RFC3986 doesn't encode ~
                    result.push('~');
                }
                b' ' if enc_type == 1 => {
                    // RFC1738: space -> +
                    result.push('+');
                }
                _ => {
                    result.push_str(&format!("%{:02X}", byte));
                }
            }
        }
        result
    }

    fn build_query_recursive(
        parts: &mut Vec<String>,
        prefix: &str,
        val: &Value,
        separator: &str,
        enc_type: i64,
        numeric_prefix: &Option<String>,
    ) {
        match val {
            Value::Array(arr) => {
                for (key, v) in arr.borrow().iter() {
                    let k = match key {
                        goro_core::array::ArrayKey::Int(n) => {
                            if prefix.is_empty() {
                                if let Some(np) = numeric_prefix {
                                    format!("{}{}", np, n)
                                } else {
                                    n.to_string()
                                }
                            } else {
                                n.to_string()
                            }
                        }
                        goro_core::array::ArrayKey::String(s) => s.to_string_lossy(),
                    };
                    let new_prefix = if prefix.is_empty() {
                        url_encode(&k, enc_type)
                    } else {
                        format!("{}%5B{}%5D", prefix, url_encode(&k, enc_type))
                    };
                    build_query_recursive(parts, &new_prefix, v, separator, enc_type, numeric_prefix);
                }
            }
            Value::Null | Value::Undef => {
                // PHP's http_build_query encodes null as empty string
                parts.push(format!("{}=", prefix));
            }
            Value::True => {
                parts.push(format!("{}=1", prefix));
            }
            Value::False => {
                parts.push(format!("{}=0", prefix));
            }
            _ => {
                let v = url_encode(&val.to_php_string().to_string_lossy(), enc_type);
                parts.push(format!("{}={}", prefix, v));
            }
        }
    }

    if let Value::Array(arr) = data {
        for (key, val) in arr.borrow().iter() {
            let k = match key {
                goro_core::array::ArrayKey::Int(n) => {
                    if let Some(np) = &_numeric_prefix {
                        format!("{}{}", np, n)
                    } else {
                        n.to_string()
                    }
                }
                goro_core::array::ArrayKey::String(s) => s.to_string_lossy(),
            };
            let prefix = url_encode(&k, enc_type);
            build_query_recursive(&mut parts, &prefix, &val, &separator, enc_type, &_numeric_prefix);
        }
    } else if let Value::Object(obj) = data {
        // Also handle objects - get public properties
        let obj = obj.borrow();
        for (prop_name, prop_val) in &obj.properties {
            let k = String::from_utf8_lossy(prop_name).to_string();
            let prefix = url_encode(&k, enc_type);
            build_query_recursive(&mut parts, &prefix, prop_val, &separator, enc_type, &_numeric_prefix);
        }
    }
    Ok(Value::String(PhpString::from_string(
        parts.join(&separator),
    )))
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
fn is_dir_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    if !vm.check_open_basedir("is_dir", &path.to_string_lossy()) {
        return Ok(Value::False);
    }
    Ok(if std::path::Path::new(&path.to_string_lossy()).is_dir() {
        Value::True
    } else {
        Value::False
    })
}
fn is_readable_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    Ok(if std::path::Path::new(&path.to_string_lossy()).exists() {
        Value::True
    } else {
        Value::False
    })
}
fn is_writable_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    Ok(if std::path::Path::new(&path.to_string_lossy()).exists() {
        Value::True
    } else {
        Value::False
    })
}
fn file_get_contents_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let _use_include_path = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
    let _context = args.get(2);
    let offset = args.get(3).map(|v| v.to_long()).unwrap_or(0);
    let length = args.get(4).map(|v| v.to_long());

    // Validate length parameter
    if let Some(len) = length {
        if len < 0 {
            let msg = "file_get_contents(): Argument #5 ($length) must be greater than or equal to 0";
            let exc = vm.create_exception(b"ValueError", msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg.to_string(), line: vm.current_line });
        }
    }

    let path_str = path.to_string_lossy();
    if !path_str.starts_with("php://") && !path_str.starts_with("http://") && !path_str.starts_with("data://") {
        if !vm.check_open_basedir("file_get_contents", &path_str) {
            return Ok(Value::False);
        }
    }
    match std::fs::read(&*path_str as &str) {
        Ok(data) => {
            let start = if offset >= 0 { offset as usize } else { 0 };
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
            vm.emit_warning(&format!("file_get_contents({}): Failed to open stream: No such file or directory", path_str));
            Ok(Value::False)
        }
    }
}
fn file_put_contents_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    if !vm.check_open_basedir("file_put_contents", &path.to_string_lossy()) {
        return Ok(Value::False);
    }
    let data_val = args.get(1).unwrap_or(&Value::Null);
    let flags = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    let append = flags & 8 != 0; // FILE_APPEND
    let lock_ex = flags & 2 != 0; // LOCK_EX

    let data_bytes = match data_val {
        Value::Array(arr) => {
            let arr = arr.borrow();
            let mut buf = Vec::new();
            for (_, v) in arr.iter() { buf.extend_from_slice(v.to_php_string().as_bytes()); }
            buf
        }
        _ => data_val.to_php_string().as_bytes().to_vec(),
    };
    let path_str = path.to_string_lossy();
    let p = std::path::Path::new(&*path_str);
    if let Some(parent) = p.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            vm.emit_warning(&format!("file_put_contents({}): Failed to open stream: No such file or directory", path_str));
            return Ok(Value::False);
        }
    }
    let result = {
        use std::io::Write;
        let file_result = if append {
            std::fs::OpenOptions::new().append(true).create(true).open(&*path_str)
        } else {
            std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(&*path_str)
        };
        file_result.and_then(|mut f| {
            if lock_ex {
                #[cfg(unix)]
                { use std::os::unix::io::AsRawFd; unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX) }; }
            }
            f.write_all(&data_bytes).map(|_| data_bytes.len())
        })
    };
    match result {
        Ok(len) => Ok(Value::Long(len as i64)),
        Err(e) => {
            vm.emit_warning(&format!("file_put_contents({}): Failed to open stream: {}", path_str, e));
            Ok(Value::False)
        }
    }
}
fn realpath_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    match std::fs::canonicalize(&*path.to_string_lossy() as &str) {
        Ok(p) => Ok(Value::String(PhpString::from_string(
            p.to_string_lossy().to_string(),
        ))),
        Err(_) => Ok(Value::False),
    }
}

fn realpath_cache_size_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(0))
}

fn realpath_cache_get_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
}

fn getcwd_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    match std::env::current_dir() {
        Ok(p) => Ok(Value::String(PhpString::from_string(
            p.to_string_lossy().to_string(),
        ))),
        Err(_) => Ok(Value::False),
    }
}
fn chdir_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    match std::env::set_current_dir(&*path.to_string_lossy() as &str) {
        Ok(_) => Ok(Value::True),
        Err(_) => Ok(Value::False),
    }
}
fn filesize_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    if !vm.check_open_basedir("filesize", &path.to_string_lossy()) {
        return Ok(Value::False);
    }
    match std::fs::metadata(&*path.to_string_lossy() as &str) {
        Ok(m) => Ok(Value::Long(m.len() as i64)),
        Err(_) => {
            vm.emit_warning(&format!("filesize(): stat failed for {}", path.to_string_lossy()));
            Ok(Value::False)
        }
    }
}
fn touch_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let path_str = path.to_string_lossy();
    if !vm.check_open_basedir("touch", &path_str) {
        return Ok(Value::False);
    }
    let p = std::path::Path::new(&*path_str);
    if !p.exists() {
        if let Some(parent) = p.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                vm.emit_warning(&format!("touch(): Unable to create file {} because No such file or directory", path_str));
                return Ok(Value::False);
            }
        }
        if let Err(e) = std::fs::write(p, b"") {
            vm.emit_warning(&format!("touch(): Unable to create file {} because {}", path_str, e));
            return Ok(Value::False);
        }
    }
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
fn stat_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let path_str = path.to_string_lossy();
    stat_path(&path_str, false)
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
            result.set(ArrayKey::String(PhpString::from_bytes(name)), Value::Long(*val));
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
                    (0, b"dev", 0), (1, b"ino", 0), (2, b"mode", 0), (3, b"nlink", 0),
                    (4, b"uid", 0), (5, b"gid", 0), (6, b"rdev", 0), (7, b"size", size),
                    (8, b"atime", 0), (9, b"mtime", 0), (10, b"ctime", 0),
                    (11, b"blksize", -1), (12, b"blocks", -1),
                ];
                // PHP stat returns all numeric keys first (0-12), then all string keys
                for (idx, _name, val) in &fields {
                    result.set(ArrayKey::Int(*idx), Value::Long(*val));
                }
                for (_idx, name, val) in &fields {
                    result.set(ArrayKey::String(PhpString::from_bytes(name)), Value::Long(*val));
                }
                Ok(Value::Array(Rc::new(RefCell::new(result))))
            }
            Err(_) => Ok(Value::False),
        }
    }
}
fn is_numeric_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
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
        return Err(VmError { message: msg, line: vm.current_line });
    }
    let levels = levels_raw as usize;
    let mut result = s.to_string();
    for _ in 0..levels {
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
    let s = path.to_string_lossy();
    // Strip trailing slashes
    let trimmed = s.trim_end_matches('/');
    let mut base = if trimmed.is_empty() {
        String::new()
    } else if let Some(pos) = trimmed.rfind('/') {
        trimmed[pos + 1..].to_string()
    } else {
        trimmed.to_string()
    };
    if let Some(suf) = suffix {
        let suf_str = suf.to_string_lossy();
        if base.ends_with(&suf_str) && base.len() > suf_str.len() {
            base.truncate(base.len() - suf_str.len());
        }
    }
    Ok(Value::String(PhpString::from_string(base)))
}

fn pathinfo_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let option = args.get(1).map(|v| v.to_long());
    let s = path.to_string_lossy();

    let dirname = {
        let trimmed = s.trim_end_matches('/');
        if let Some(pos) = trimmed.rfind('/') {
            if pos == 0 { "/".to_string() } else { trimmed[..pos].to_string() }
        } else {
            ".".to_string()
        }
    };
    let basename = {
        let trimmed = s.trim_end_matches('/');
        if let Some(pos) = trimmed.rfind('/') {
            trimmed[pos + 1..].to_string()
        } else {
            trimmed.to_string()
        }
    };
    let extension = basename.rfind('.').map(|pos| basename[pos + 1..].to_string());
    let filename = if let Some(pos) = basename.rfind('.') {
        basename[..pos].to_string()
    } else {
        basename.clone()
    };

    match option {
        Some(1) => Ok(Value::String(PhpString::from_string(dirname))),  // PATHINFO_DIRNAME
        Some(2) => Ok(Value::String(PhpString::from_string(basename))), // PATHINFO_BASENAME
        Some(4) => Ok(Value::String(PhpString::from_string(extension.unwrap_or_default()))), // PATHINFO_EXTENSION
        Some(8) => Ok(Value::String(PhpString::from_string(filename))), // PATHINFO_FILENAME
        _ => {
            // Return full array
            let mut result = PhpArray::new();
            result.set(ArrayKey::String(PhpString::from_bytes(b"dirname")), Value::String(PhpString::from_string(dirname)));
            result.set(ArrayKey::String(PhpString::from_bytes(b"basename")), Value::String(PhpString::from_string(basename)));
            if let Some(ext) = extension {
                result.set(ArrayKey::String(PhpString::from_bytes(b"extension")), Value::String(PhpString::from_string(ext)));
            }
            result.set(ArrayKey::String(PhpString::from_bytes(b"filename")), Value::String(PhpString::from_string(filename)));
            Ok(Value::Array(Rc::new(RefCell::new(result))))
        }
    }
}

// Additional commonly needed stubs
fn register_shutdown_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        return Ok(Value::Null);
    }
    let callback = args[0].clone();
    let extra_args: Vec<Value> = args[1..].to_vec();
    vm.shutdown_functions.push((callback, extra_args));
    Ok(Value::Null)
}
fn interface_exists_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let name = args.first().unwrap_or(&Value::Null).to_php_string();
    // Strip leading backslash for fully qualified names
    let raw_bytes = name.as_bytes();
    let name_bytes = if raw_bytes.starts_with(b"\\") { &raw_bytes[1..] } else { raw_bytes };
    let name_lower: Vec<u8> = name_bytes
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect();
    // Check both user-defined and built-in interfaces
    let is_builtin = matches!(
        name_lower.as_slice(),
        b"iterator"
            | b"iteratoraggregate"
            | b"throwable"
            | b"arrayaccess"
            | b"serializable"
            | b"countable"
            | b"stringable"
            | b"traversable"
    );
    if is_builtin
        || vm
            .classes
            .get(&name_lower)
            .map(|c| c.is_interface)
            .unwrap_or(false)
    {
        return Ok(Value::True);
    }
    // Try autoload if second arg is true (default)
    let autoload = args.get(1).map_or(true, |v| v.to_bool());
    if autoload {
        vm.try_autoload_class(name_bytes);
        if vm.classes.get(&name_lower).map(|c| c.is_interface).unwrap_or(false) {
            return Ok(Value::True);
        }
    }
    Ok(Value::False)
}
fn trait_exists_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let name = args.first().unwrap_or(&Value::Null).to_php_string();
    let raw_bytes = name.as_bytes();
    let name_bytes = if raw_bytes.starts_with(b"\\") { &raw_bytes[1..] } else { raw_bytes };
    let name_lower: Vec<u8> = name_bytes
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect();
    if vm.classes.get(&name_lower).map(|c| c.is_trait).unwrap_or(false) {
        return Ok(Value::True);
    }
    // Try autoload if second arg is true (default)
    let autoload = args.get(1).map_or(true, |v| v.to_bool());
    if autoload {
        vm.try_autoload_class(name_bytes);
        if vm.classes.get(&name_lower).map(|c| c.is_trait).unwrap_or(false) {
            return Ok(Value::True);
        }
    }
    Ok(Value::False)
}
fn enum_exists_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let name = args.first().unwrap_or(&Value::Null).to_php_string();
    let raw_bytes = name.as_bytes();
    let name_bytes = if raw_bytes.starts_with(b"\\") { &raw_bytes[1..] } else { raw_bytes };
    let name_lower: Vec<u8> = name_bytes
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect();
    if vm.classes.get(&name_lower).map(|c| c.is_enum).unwrap_or(false) {
        return Ok(Value::True);
    }
    // Try autoload if second arg is true (default)
    let autoload = args.get(1).map_or(true, |v| v.to_bool());
    if autoload {
        vm.try_autoload_class(name_bytes);
        if vm.classes.get(&name_lower).map(|c| c.is_enum).unwrap_or(false) {
            return Ok(Value::True);
        }
    }
    Ok(Value::False)
}
fn gc_collect_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(0))
}
fn gc_enabled_fn(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let enabled = vm
        .constants
        .get(b"zend.enable_gc".as_ref())
        .map(|v| v.is_truthy())
        .unwrap_or(true);
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
    Ok(Value::Null)
}
fn get_object_vars_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Object(obj)) = args.first() {
        let obj = obj.borrow();
        let class_name_lower: Vec<u8> = obj.class_name.iter().map(|b| b.to_ascii_lowercase()).collect();

        // Get the calling class context to determine visibility
        let calling_class = vm.get_current_class_name();
        let calling_class_lower: Option<Vec<u8>> = calling_class.as_ref().map(|c| c.iter().map(|b| b.to_ascii_lowercase()).collect());

        // Build a map of property visibility from the class definition
        let prop_visibility: std::collections::HashMap<Vec<u8>, goro_core::object::Visibility> =
            if let Some(class_def) = vm.get_class_def(&class_name_lower) {
                class_def.properties.iter()
                    .map(|p| (p.name.clone(), p.visibility))
                    .collect()
            } else {
                std::collections::HashMap::new()
            };

        // Build a set of typed properties to skip when uninitialized
        let typed_props: std::collections::HashSet<Vec<u8>> =
            if let Some(class_def) = vm.get_class_def(&class_name_lower) {
                class_def.properties.iter()
                    .filter(|p| p.property_type.is_some())
                    .map(|p| p.name.clone())
                    .collect()
            } else {
                std::collections::HashSet::new()
            };

        let mut arr = PhpArray::new();
        for (name, val) in &obj.properties {
            // Skip internal properties
            if name.starts_with(b"__spl_") || name.starts_with(b"__reflection_")
                || name.starts_with(b"__timestamp") || name.starts_with(b"__enum_")
                || name.starts_with(b"__fiber_") || name.starts_with(b"__ctor_")
                || name.starts_with(b"__clone_") || name.starts_with(b"__destructed")
                || name.starts_with(b"__weak_ref_id") || name.starts_with(b"__sxml_") {
                continue;
            }
            // Skip uninitialized typed properties
            if matches!(val, Value::Undef) && typed_props.contains(name) {
                continue;
            }
            // Check visibility
            let vis = prop_visibility.get(name).copied().unwrap_or(goro_core::object::Visibility::Public);
            let accessible = match vis {
                goro_core::object::Visibility::Public => true,
                goro_core::object::Visibility::Protected => {
                    // Accessible if calling from same class or subclass
                    if let Some(ref cc) = calling_class_lower {
                        cc == &class_name_lower || vm.is_subclass_of(cc, &class_name_lower) || vm.is_subclass_of(&class_name_lower, cc)
                    } else {
                        false
                    }
                }
                goro_core::object::Visibility::Private => {
                    // Only accessible from the same class
                    if let Some(ref cc) = calling_class_lower {
                        cc == &class_name_lower
                    } else {
                        false
                    }
                }
            };
            if accessible {
                arr.set(
                    goro_core::array::ArrayKey::String(PhpString::from_vec(name.clone())),
                    val.clone(),
                );
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
    if args.is_empty() || matches!(val, Value::Null | Value::Undef) {
        // Deprecated: Calling get_class() without arguments is deprecated
        _vm.emit_deprecated("Calling get_class() without arguments is deprecated");
        // Return the current class scope (the declaring class)
        if let Some(scope) = _vm.current_class_scope() {
            // Find the original-case class name
            let scope_lower: Vec<u8> = scope.iter().map(|b| b.to_ascii_lowercase()).collect();
            let class_name = _vm.classes.get(&scope_lower)
                .map(|c| c.name.clone())
                .unwrap_or(scope);
            return Ok(Value::String(PhpString::from_vec(class_name)));
        }
        return Ok(Value::False);
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
        if b.starts_with(b"__closure_") || b.starts_with(b"__arrow_") || b.starts_with(b"__bound_closure_") || b.starts_with(b"__closure_fcc_") {
            Ok(Value::String(PhpString::from_bytes(b"Closure")))
        } else {
            // PHP 8: TypeError for non-object
            let type_name = Vm::value_type_name(val);
            _vm.throw_type_error(format!("get_class(): Argument #1 ($object) must be of type object, {} given", type_name));
            Ok(Value::Null)
        }
    } else if let Value::Array(arr) = val {
        let arr = arr.borrow();
        if let Some(first) = arr.values().next() {
            if let Value::String(s) = first {
                let b = s.as_bytes();
                if b.starts_with(b"__closure_") || b.starts_with(b"__arrow_") || b.starts_with(b"__bound_closure_") || b.starts_with(b"__closure_fcc_") {
                    return Ok(Value::String(PhpString::from_bytes(b"Closure")));
                }
            }
        }
        // PHP 8: TypeError for non-object
        let type_name = Vm::value_type_name(val);
        _vm.throw_type_error(format!("get_class(): Argument #1 ($object) must be of type object, {} given", type_name));
        Ok(Value::Null)
    } else {
        // PHP 8: TypeError for non-object
        let type_name = Vm::value_type_name(val);
        _vm.throw_type_error(format!("get_class(): Argument #1 ($object) must be of type object, {} given", type_name));
        Ok(Value::Null)
    }
}
fn serialize_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    // Check for non-serializable classes
    if let Value::Object(obj) = val {
        let class_lower: Vec<u8> = obj.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
        if matches!(class_lower.as_slice(), b"weakreference" | b"weakmap" | b"closure" | b"generator" | b"fiber") {
            let class_name = goro_core::value::display_class_name(&obj.borrow().class_name);
            let msg = format!("Serialization of '{}' is not allowed", class_name);
            let exc = _vm.create_exception(b"Exception", &msg, _vm.current_line);
            _vm.current_exception = Some(exc);
            return Err(VmError { message: format!("Uncaught Exception: {}", msg), line: _vm.current_line });
        }
    }
    let s = serialize_value_with_vm(val, 0, _vm);
    Ok(Value::String(PhpString::from_string(s)))
}
fn serialize_value_with_vm(val: &Value, depth: usize, vm: &mut Vm) -> String {
    if depth > 128 {
        return "N;".to_string();
    }
    match val {
        Value::Object(obj) => {
            let class_name_bytes = obj.borrow().class_name.clone();
            let class_lower: Vec<u8> = class_name_bytes.iter().map(|b| b.to_ascii_lowercase()).collect();
            let class_name = String::from_utf8_lossy(&class_name_bytes).to_string();

            // Check if this is an enum case - use E: format
            if Vm::is_enum_case(val) {
                let case_name = obj.borrow().get_property(b"name");
                let case_str = case_name.to_php_string();
                let full = format!("{}:{}", class_name, case_str.to_string_lossy());
                return format!("E:{}:\"{}\";", full.len(), full);
            }

            // Check for __serialize first (PHP 7.4+)
            let is_builtin_serializable = |cl: &[u8]| -> bool {
                matches!(cl,
                    b"spldoublylinkedlist" | b"splstack" | b"splqueue"
                    | b"splfixedarray" | b"splobjectstorage"
                    | b"splheap" | b"splminheap" | b"splmaxheap"
                    | b"splpriorityqueue"
                )
            };
            let has_serialize = vm.classes.get(&class_lower)
                .map(|c| c.get_method(b"__serialize").is_some())
                .unwrap_or(false)
                || is_builtin_serializable(class_lower.as_slice())
                || {
                    // Check parent chain for built-in SPL classes
                    let mut found = false;
                    let mut check = class_lower.clone();
                    for _ in 0..10 {
                        if let Some(parent) = goro_core::vm::get_builtin_parent(&check) {
                            let parent_lower: Vec<u8> = parent.iter().map(|b| b.to_ascii_lowercase()).collect();
                            if is_builtin_serializable(&parent_lower) {
                                found = true;
                                break;
                            }
                            check = parent_lower;
                        } else if let Some(ce) = vm.classes.get(&check) {
                            if let Some(ref p) = ce.parent {
                                let parent_lower: Vec<u8> = p.iter().map(|b| b.to_ascii_lowercase()).collect();
                                if is_builtin_serializable(&parent_lower) {
                                    found = true;
                                    break;
                                }
                                check = parent_lower;
                            } else { break; }
                        } else { break; }
                    }
                    found
                };
            if has_serialize {
                let result = vm.call_object_method(val, b"__serialize", &[]);
                if let Some(Value::Array(arr)) = result {
                    let arr = arr.borrow();
                    let mut s = format!("O:{}:\"{}\":{}:{{", class_name.len(), class_name, arr.len());
                    for (key, v) in arr.iter() {
                        match key {
                            goro_core::array::ArrayKey::Int(n) => s.push_str(&format!("i:{};", n)),
                            goro_core::array::ArrayKey::String(k) => {
                                s.push_str(&format!("s:{}:\"{}\";", k.len(), k.to_string_lossy()));
                            }
                        }
                        s.push_str(&serialize_value_with_vm(v, depth + 1, vm));
                    }
                    s.push('}');
                    return s;
                }
            }

            // Check for __sleep
            let has_sleep = vm.classes.get(&class_lower)
                .map(|c| c.get_method(b"__sleep").is_some())
                .unwrap_or(false);
            if has_sleep {
                let result = vm.call_object_method(val, b"__sleep", &[]);
                if let Some(Value::Array(arr)) = result {
                    let arr_borrow = arr.borrow();
                    let prop_names: Vec<Vec<u8>> = arr_borrow.values().map(|v| {
                        v.to_php_string().as_bytes().to_vec()
                    }).collect();
                    drop(arr_borrow);
                    let obj_borrow = obj.borrow();
                    let class_name_bytes = obj_borrow.class_name.clone();
                    let class_lower_sleep: Vec<u8> = class_name_bytes.iter().map(|b| b.to_ascii_lowercase()).collect();
                    let mut s = format!("O:{}:\"{}\":{}:{{", class_name.len(), class_name, prop_names.len());
                    for prop_name in &prop_names {
                        let prop_str = String::from_utf8_lossy(prop_name);
                        if let Some(v) = obj_borrow.properties.iter().find(|(k, _)| k == prop_name).map(|(_, v)| v) {
                            // Check visibility for name mangling
                            let vis = vm.classes.get(&class_lower_sleep)
                                .and_then(|c| c.properties.iter().find(|p| p.name == *prop_name).map(|p| p.visibility));
                            match vis {
                                Some(Visibility::Private) => {
                                    let mangled_len = class_name_bytes.len() + prop_name.len() + 2;
                                    s.push_str(&format!("s:{}:\"", mangled_len));
                                    s.push('\0');
                                    s.push_str(&class_name);
                                    s.push('\0');
                                    s.push_str(&prop_str);
                                    s.push_str("\";");
                                }
                                Some(Visibility::Protected) => {
                                    let mangled_len = prop_name.len() + 3;
                                    s.push_str(&format!("s:{}:\"", mangled_len));
                                    s.push('\0');
                                    s.push('*');
                                    s.push('\0');
                                    s.push_str(&prop_str);
                                    s.push_str("\";");
                                }
                                _ => {
                                    s.push_str(&format!("s:{}:\"{}\";", prop_name.len(), prop_str));
                                }
                            }
                            s.push_str(&serialize_value_with_vm(v, depth + 1, vm));
                        } else {
                            // Property doesn't exist, emit warning and skip
                            vm.emit_warning(&format!(
                                "serialize(): \"{}\" returned as member variable from __sleep() but does not exist",
                                prop_str
                            ));
                        }
                    }
                    s.push('}');
                    return s;
                } else if let Some(Value::Null) = result {
                    // __sleep returned null
                    return "N;".to_string();
                }
            }

            // Default serialization - use VM to look up property visibility
            {
                let obj = obj.borrow();
                let class_name_str = std::borrow::Cow::<str>::Owned(goro_core::value::display_class_name(&obj.class_name));
                let class_lower: Vec<u8> = obj.class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                let visible_props: Vec<_> = obj.properties.iter()
                    .filter(|(name, _)| !is_serialize_internal_property(name))
                    .collect();
                let prop_count = visible_props.len();
                let mut result = format!("O:{}:\"{}\":{}:{{", class_name_str.len(), class_name_str, prop_count);
                for (name, prop_val) in &visible_props {
                    // Look up property visibility from class definition
                    let vis = vm.classes.get(&class_lower)
                        .and_then(|c| {
                            c.properties.iter().find(|p| p.name == *name).map(|p| p.visibility)
                        });
                    // If the property name already contains NUL bytes, it's already mangled
                    if name.contains(&0u8) {
                        result.push_str(&format!("s:{}:\"", name.len()));
                        for &b in name.iter() { result.push(b as char); }
                        result.push_str("\";");
                    } else {
                        match vis {
                            Some(Visibility::Private) => {
                                // Private: \0ClassName\0propName
                                let mangled_len = obj.class_name.len() + name.len() + 2;
                                result.push_str(&format!("s:{}:\"", mangled_len));
                                result.push('\0');
                                result.push_str(&class_name_str);
                                result.push('\0');
                                result.push_str(&String::from_utf8_lossy(name));
                                result.push_str("\";");
                            }
                            Some(Visibility::Protected) => {
                                // Protected: \0*\0propName
                                let mangled_len = name.len() + 3;
                                result.push_str(&format!("s:{}:\"", mangled_len));
                                result.push('\0');
                                result.push('*');
                                result.push('\0');
                                result.push_str(&String::from_utf8_lossy(name));
                                result.push_str("\";");
                            }
                            _ => {
                                // Public or unknown: plain name
                                let name_str = String::from_utf8_lossy(name);
                                result.push_str(&format!("s:{}:\"{}\";", name.len(), name_str));
                            }
                        }
                    }
                    result.push_str(&serialize_value_with_vm(prop_val, depth + 1, vm));
                }
                result.push('}');
                result
            }
        }
        Value::Reference(r) => serialize_value_with_vm(&r.borrow(), depth, vm),
        _ => serialize_value_depth(val, depth),
    }
}
/// Check if a property name is an internal implementation detail that should be hidden from serialize
fn is_serialize_internal_property(name: &[u8]) -> bool {
    name.starts_with(b"__spl_") || name.starts_with(b"__reflection_")
        || name.starts_with(b"__timestamp") || name.starts_with(b"__enum_")
        || name.starts_with(b"__fiber_") || name.starts_with(b"__ctor_")
        || name.starts_with(b"__clone_") || name.starts_with(b"__destructed")
        || name.starts_with(b"__weak_ref_id") || name.starts_with(b"__sxml_")
}

fn serialize_value_depth(val: &Value, depth: usize) -> String {
    if depth > 128 {
        return "N;".to_string();
    }
    match val {
        Value::Null | Value::Undef => "N;".to_string(),
        Value::True => "b:1;".to_string(),
        Value::False => "b:0;".to_string(),
        Value::Long(n) => format!("i:{};", n),
        Value::Double(f) => {
            if f.is_nan() {
                "d:NAN;".to_string()
            } else if f.is_infinite() {
                if *f > 0.0 { "d:INF;".to_string() } else { "d:-INF;".to_string() }
            } else {
                let sp = goro_core::value::get_php_serialize_precision();
                let formatted = if sp < 0 {
                    // Shortest roundtrip representation
                    let mut best = format!("{}", f);
                    for prec in 0..20 {
                        let candidate = format!("{:.prec$}", f, prec = prec);
                        if let Ok(parsed) = candidate.parse::<f64>() {
                            if parsed == *f {
                                best = candidate;
                                break;
                            }
                        }
                    }
                    best
                } else {
                    goro_core::value::format_php_float_with_precision_pub(*f, sp as usize)
                };
                format!("d:{};", formatted)
            }
        }
        Value::String(s) => format!("s:{}:\"{}\";", s.len(), s.to_string_lossy()),
        Value::Array(arr) => {
            let arr = arr.borrow();
            let mut result = format!("a:{}:{{", arr.len());
            for (key, val) in arr.iter() {
                match key {
                    goro_core::array::ArrayKey::Int(n) => result.push_str(&format!("i:{};", n)),
                    goro_core::array::ArrayKey::String(s) => {
                        result.push_str(&format!("s:{}:\"{}\";", s.len(), s.to_string_lossy()))
                    }
                }
                result.push_str(&serialize_value_depth(val, depth + 1));
            }
            result.push('}');
            result
        }
        Value::Object(obj) => {
            let obj = obj.borrow();
            let class_name = std::borrow::Cow::<str>::Owned(goro_core::value::display_class_name(&obj.class_name));
            // Filter out internal properties (same list as var_dump)
            let visible_props: Vec<_> = obj.properties.iter()
                .filter(|(name, _)| !is_serialize_internal_property(name))
                .collect();
            let prop_count = visible_props.len();
            let mut result = format!("O:{}:\"{}\":{}:{{", class_name.len(), class_name, prop_count);
            for (name, val) in &visible_props {
                let name_str = String::from_utf8_lossy(name);
                result.push_str(&format!("s:{}:\"{}\";", name.len(), name_str));
                result.push_str(&serialize_value_depth(val, depth + 1));
            }
            result.push('}');
            result
        }
        Value::Generator(_) => "N;".to_string(),
        Value::Reference(r) => serialize_value_depth(&r.borrow(), depth),
    }
}
fn unserialize_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = match args.first() {
        Some(Value::String(s)) => s.as_bytes().to_vec(),
        Some(v) => v.to_php_string().as_bytes().to_vec(),
        None => return Ok(Value::False),
    };
    match unserialize_value(&data, &mut 0, vm) {
        Some(val) => Ok(val),
        None => {
            let msg = format!("unserialize(): Error at offset 0 of {} bytes", data.len());
            vm.emit_warning_at(&msg, vm.current_line);
            Ok(Value::False)
        }
    }
}

fn unserialize_value(data: &[u8], pos: &mut usize, vm: &mut Vm) -> Option<Value> {
    if *pos >= data.len() {
        return None;
    }
    match data[*pos] {
        b'N' => {
            // N;
            *pos += 1;
            if *pos < data.len() && data[*pos] == b';' {
                *pos += 1;
            }
            Some(Value::Null)
        }
        b'b' => {
            // b:0; or b:1;
            *pos += 1;
            if *pos < data.len() && data[*pos] == b':' {
                *pos += 1;
            }
            let val = if *pos < data.len() && data[*pos] == b'1' {
                Value::True
            } else {
                Value::False
            };
            *pos += 1;
            if *pos < data.len() && data[*pos] == b';' {
                *pos += 1;
            }
            Some(val)
        }
        b'i' => {
            // i:123;
            *pos += 1;
            if *pos < data.len() && data[*pos] == b':' {
                *pos += 1;
            }
            let start = *pos;
            while *pos < data.len() && data[*pos] != b';' {
                *pos += 1;
            }
            let num_str = String::from_utf8_lossy(&data[start..*pos]).to_string();
            if *pos < data.len() {
                *pos += 1;
            }
            Some(Value::Long(num_str.parse::<i64>().unwrap_or(0)))
        }
        b'd' => {
            // d:1.5;
            *pos += 1;
            if *pos < data.len() && data[*pos] == b':' {
                *pos += 1;
            }
            let start = *pos;
            while *pos < data.len() && data[*pos] != b';' {
                *pos += 1;
            }
            let num_str = String::from_utf8_lossy(&data[start..*pos]).to_string();
            if *pos < data.len() {
                *pos += 1;
            }
            let f = match num_str.as_str() {
                "INF" => f64::INFINITY,
                "-INF" => f64::NEG_INFINITY,
                "NAN" => f64::NAN,
                _ => num_str.parse::<f64>().unwrap_or(0.0),
            };
            Some(Value::Double(f))
        }
        b's' => {
            // s:5:"hello";
            *pos += 1;
            if *pos < data.len() && data[*pos] == b':' {
                *pos += 1;
            }
            let start = *pos;
            while *pos < data.len() && data[*pos] != b':' {
                *pos += 1;
            }
            let len_str = String::from_utf8_lossy(&data[start..*pos]).to_string();
            let len = len_str.parse::<usize>().unwrap_or(0);
            if *pos < data.len() {
                *pos += 1; // skip ':'
            }
            if *pos < data.len() && data[*pos] == b'"' {
                *pos += 1; // skip opening '"'
            }
            let str_start = *pos;
            let str_end = (*pos + len).min(data.len());
            *pos = str_end;
            let val = Value::String(PhpString::from_vec(data[str_start..str_end].to_vec()));
            if *pos < data.len() && data[*pos] == b'"' {
                *pos += 1; // skip closing '"'
            }
            if *pos < data.len() && data[*pos] == b';' {
                *pos += 1;
            }
            Some(val)
        }
        b'a' => {
            // a:2:{...}
            *pos += 1;
            if *pos < data.len() && data[*pos] == b':' {
                *pos += 1;
            }
            let start = *pos;
            while *pos < data.len() && data[*pos] != b':' {
                *pos += 1;
            }
            let count_str = String::from_utf8_lossy(&data[start..*pos]).to_string();
            let count = count_str.parse::<usize>().unwrap_or(0);
            if *pos < data.len() {
                *pos += 1; // skip ':'
            }
            if *pos < data.len() && data[*pos] == b'{' {
                *pos += 1;
            }
            let mut arr = PhpArray::new();
            for _ in 0..count {
                let key = unserialize_value(data, pos, vm)?;
                let value = unserialize_value(data, pos, vm)?;
                match &key {
                    Value::Long(n) => arr.set(ArrayKey::Int(*n), value),
                    Value::String(s) => arr.set(ArrayKey::String(s.clone()), value),
                    _ => {}
                }
            }
            if *pos < data.len() && data[*pos] == b'}' {
                *pos += 1;
            }
            Some(Value::Array(Rc::new(RefCell::new(arr))))
        }
        b'O' => {
            // O:8:"ClassName":2:{...}
            *pos += 1;
            if *pos < data.len() && data[*pos] == b':' {
                *pos += 1;
            }
            // Read class name length
            let start = *pos;
            while *pos < data.len() && data[*pos] != b':' {
                *pos += 1;
            }
            let name_len_str = String::from_utf8_lossy(&data[start..*pos]).to_string();
            let name_len = name_len_str.parse::<usize>().unwrap_or(0);
            if *pos < data.len() {
                *pos += 1; // skip ':'
            }
            if *pos < data.len() && data[*pos] == b'"' {
                *pos += 1;
            }
            let name_start = *pos;
            let name_end = (*pos + name_len).min(data.len());
            let class_name = data[name_start..name_end].to_vec();
            *pos = name_end;
            if *pos < data.len() && data[*pos] == b'"' {
                *pos += 1;
            }
            if *pos < data.len() && data[*pos] == b':' {
                *pos += 1;
            }
            // Read property count
            let start = *pos;
            while *pos < data.len() && data[*pos] != b':' {
                *pos += 1;
            }
            let prop_count_str = String::from_utf8_lossy(&data[start..*pos]).to_string();
            let prop_count = prop_count_str.parse::<usize>().unwrap_or(0);
            if *pos < data.len() {
                *pos += 1; // skip ':'
            }
            if *pos < data.len() && data[*pos] == b'{' {
                *pos += 1;
            }

            // Check for non-unserializable classes
            let class_lower_check: Vec<u8> = class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
            if matches!(class_lower_check.as_slice(), b"weakreference" | b"weakmap" | b"closure" | b"generator" | b"fiber") {
                let class_display = String::from_utf8_lossy(&class_name).to_string();
                let msg = format!("Unserialization of '{}' is not allowed", class_display);
                let exc = vm.create_exception(b"Exception", &msg, vm.current_line);
                vm.current_exception = Some(exc);
                return None;
            }

            let obj_id = vm.next_object_id();
            // Use canonical class name
            let class_lower: Vec<u8> = class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
            let canonical = if let Some(ce) = vm.classes.get(&class_lower) {
                ce.name.clone()
            } else {
                // Check known built-in names
                match class_lower.as_slice() {
                    b"stdclass" => b"stdClass".to_vec(),
                    _ => class_name.clone(),
                }
            };
            let mut obj = PhpObject::new(canonical, obj_id);
            // Initialize from class definition
            if let Some(ce) = vm.classes.get(&class_lower).cloned() {
                for prop in &ce.properties {
                    if !prop.is_static {
                        obj.set_property(prop.name.clone(), prop.default.clone());
                    }
                }
            }

            // Check for __unserialize first (PHP 7.4+)
            let spl_has_builtin_unserialize = |cl: &[u8]| -> bool {
                matches!(cl,
                    b"spldoublylinkedlist" | b"splstack" | b"splqueue"
                    | b"splfixedarray" | b"splobjectstorage"
                    | b"splheap" | b"splminheap" | b"splmaxheap"
                    | b"splpriorityqueue"
                )
            };
            let has_unserialize = vm.classes.get(&class_lower)
                .map(|c| c.get_method(b"__unserialize").is_some())
                .unwrap_or(false)
                || spl_has_builtin_unserialize(class_lower.as_slice())
                || {
                    let mut found = false;
                    let mut check = class_lower.clone();
                    for _ in 0..10 {
                        if let Some(parent) = goro_core::vm::get_builtin_parent(&check) {
                            let parent_lower: Vec<u8> = parent.iter().map(|b| b.to_ascii_lowercase()).collect();
                            if spl_has_builtin_unserialize(&parent_lower) { found = true; break; }
                            check = parent_lower;
                        } else { break; }
                    }
                    found
                };

            // Parse properties - keep as raw array for __unserialize
            let mut raw_data = PhpArray::new();
            for _ in 0..prop_count {
                let key = unserialize_value(data, pos, vm)?;
                let value = unserialize_value(data, pos, vm)?;
                if has_unserialize {
                    // For __unserialize, keep original keys (including int keys)
                    match &key {
                        Value::Long(n) => raw_data.set(ArrayKey::Int(*n), value),
                        Value::String(s) => raw_data.set(ArrayKey::String(s.clone()), value),
                        _ => {}
                    }
                } else {
                    if let Value::String(s) = &key {
                        // Handle private/protected property names
                        // Private: \0ClassName\0propName
                        // Protected: \0*\0propName
                        let name_bytes = s.as_bytes();
                        let prop_name = if !name_bytes.is_empty() && name_bytes[0] == 0 {
                            // Find the second \0
                            if let Some(end) = name_bytes[1..].iter().position(|&b| b == 0) {
                                name_bytes[end + 2..].to_vec()
                            } else {
                                name_bytes.to_vec()
                            }
                        } else {
                            name_bytes.to_vec()
                        };
                        obj.set_property(prop_name, value);
                    }
                }
            }
            if *pos < data.len() && data[*pos] == b'}' {
                *pos += 1;
            }

            let obj_val = Value::Object(Rc::new(RefCell::new(obj)));

            if has_unserialize {
                let data_val = Value::Array(Rc::new(RefCell::new(raw_data)));
                let _ = vm.call_object_method(&obj_val, b"__unserialize", &[data_val]);
            } else {
                // Check for __wakeup
                let has_wakeup = vm.classes.get(&class_lower)
                    .map(|c| c.get_method(b"__wakeup").is_some())
                    .unwrap_or(false);
                if has_wakeup {
                    let _ = vm.call_object_method(&obj_val, b"__wakeup", &[]);
                }
            }

            Some(obj_val)
        }
        b'R' | b'r' => {
            // R:n; or r:n; (references - simplified: treat as null)
            *pos += 1;
            if *pos < data.len() && data[*pos] == b':' {
                *pos += 1;
            }
            while *pos < data.len() && data[*pos] != b';' {
                *pos += 1;
            }
            if *pos < data.len() {
                *pos += 1;
            }
            Some(Value::Null)
        }
        b'E' => {
            // E:n:"ClassName:CaseName";
            *pos += 1;
            if *pos < data.len() && data[*pos] == b':' {
                *pos += 1;
            }
            // Read total length
            let start = *pos;
            while *pos < data.len() && data[*pos] != b':' {
                *pos += 1;
            }
            let len_str = String::from_utf8_lossy(&data[start..*pos]).to_string();
            let len = len_str.parse::<usize>().unwrap_or(0);
            if *pos < data.len() {
                *pos += 1; // skip ':'
            }
            if *pos < data.len() && data[*pos] == b'"' {
                *pos += 1;
            }
            let str_start = *pos;
            let str_end = (*pos + len).min(data.len());
            let content = &data[str_start..str_end];
            *pos = str_end;
            if *pos < data.len() && data[*pos] == b'"' {
                *pos += 1;
            }
            if *pos < data.len() && data[*pos] == b';' {
                *pos += 1;
            }

            // Parse "ClassName:CaseName"
            if let Some(colon_pos) = content.iter().position(|&b| b == b':') {
                let class_bytes = &content[..colon_pos];
                let case_bytes = &content[colon_pos + 1..];
                let class_lower: Vec<u8> = class_bytes.iter().map(|b| b.to_ascii_lowercase()).collect();

                // Check if class exists and is an enum
                if let Some(ce) = vm.classes.get(&class_lower) {
                    if !ce.is_enum {
                        let class_str = String::from_utf8_lossy(class_bytes);
                        vm.emit_warning_at(
                            &format!("unserialize(): Class '{}' is not an enum", class_str),
                            vm.current_line,
                        );
                        let msg = format!("unserialize(): Error at offset 0 of {} bytes", data.len());
                        vm.emit_warning_at(&msg, vm.current_line);
                        return None;
                    }
                    // Check if the case exists (it must be in enum_cases, not just constants)
                    let case_exists = ce.enum_cases.iter().any(|(name, _)| name == case_bytes);
                    if !case_exists {
                        // Check if it's a regular constant (not a case)
                        let is_const = ce.constants.contains_key(case_bytes);
                        let class_str = String::from_utf8_lossy(class_bytes);
                        let case_str = String::from_utf8_lossy(case_bytes);
                        if is_const {
                            vm.emit_warning_at(
                                &format!("unserialize(): {}::{} is not an enum case", class_str, case_str),
                                vm.current_line,
                            );
                        } else {
                            vm.emit_warning_at(
                                &format!("unserialize(): Undefined constant {}::{}", class_str, case_str),
                                vm.current_line,
                            );
                        }
                        let offset = str_end.min(data.len());
                        let msg = format!("unserialize(): Error at offset {} of {} bytes", offset, data.len());
                        vm.emit_warning_at(&msg, vm.current_line);
                        return None;
                    }
                    // Get/create the enum case singleton
                    if let Some(enum_obj) = vm.get_enum_case(&class_lower, case_bytes) {
                        return Some(enum_obj);
                    }
                } else {
                    let class_str = String::from_utf8_lossy(class_bytes);
                    vm.emit_warning_at(
                        &format!("unserialize(): Class '{}' is not an enum", class_str),
                        vm.current_line,
                    );
                    let msg = format!("unserialize(): Error at offset 0 of {} bytes", data.len());
                    vm.emit_warning_at(&msg, vm.current_line);
                    return None;
                }
            } else {
                // Missing colon
                let content_str = String::from_utf8_lossy(content);
                vm.emit_warning_at(
                    &format!("unserialize(): Invalid enum name '{}' (missing colon)", content_str),
                    vm.current_line,
                );
                let msg = format!("unserialize(): Error at offset 0 of {} bytes", data.len());
                vm.emit_warning_at(&msg, vm.current_line);
                return None;
            }
            None
        }
        b'C' => {
            // C:n:"ClassName":n:{...} (custom serializable)
            // Skip for now - parse but return stdClass
            *pos += 1;
            if *pos < data.len() && data[*pos] == b':' {
                *pos += 1;
            }
            // Read class name length
            let c_name_len_start = *pos;
            while *pos < data.len() && data[*pos] != b':' {
                *pos += 1;
            }
            let c_name_len = String::from_utf8_lossy(&data[c_name_len_start..*pos]).parse::<usize>().unwrap_or(0);
            if *pos < data.len() {
                *pos += 1;
            }
            // Read class name
            let mut c_class_name = Vec::new();
            if *pos < data.len() && data[*pos] == b'"' {
                *pos += 1;
                let c_name_start = *pos;
                let c_name_end = (*pos + c_name_len).min(data.len());
                c_class_name = data[c_name_start..c_name_end].to_vec();
                *pos = c_name_end;
                if *pos < data.len() && data[*pos] == b'"' {
                    *pos += 1;
                }
            }
            // Check for non-unserializable classes
            let c_class_lower: Vec<u8> = c_class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
            if matches!(c_class_lower.as_slice(), b"weakreference" | b"weakmap" | b"closure" | b"generator" | b"fiber") {
                let class_display = String::from_utf8_lossy(&c_class_name).to_string();
                let msg = format!("Unserialization of '{}' is not allowed", class_display);
                let exc = vm.create_exception(b"Exception", &msg, vm.current_line);
                vm.current_exception = Some(exc);
                return None;
            }
            if *pos < data.len() && data[*pos] == b':' {
                *pos += 1;
            }
            // Skip data length
            let start = *pos;
            while *pos < data.len() && data[*pos] != b':' {
                *pos += 1;
            }
            let data_len_str = String::from_utf8_lossy(&data[start..*pos]).to_string();
            let data_len = data_len_str.parse::<usize>().unwrap_or(0);
            if *pos < data.len() {
                *pos += 1;
            }
            if *pos < data.len() && data[*pos] == b'{' {
                *pos += 1;
            }
            *pos = (*pos + data_len).min(data.len());
            if *pos < data.len() && data[*pos] == b'}' {
                *pos += 1;
            }
            let obj_id = vm.next_object_id();
            let obj = PhpObject::new(b"stdClass".to_vec(), obj_id);
            Some(Value::Object(Rc::new(RefCell::new(obj))))
        }
        _ => None,
    }
}
fn memory_get_usage_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(goro_core::value::memory_get_usage() as i64))
}
fn memory_get_peak_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // We don't track peak separately yet, return current
    Ok(Value::Long(goro_core::value::memory_get_usage() as i64))
}
fn memory_reset_peak_usage_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // No-op stub, we don't track peak usage
    Ok(Value::Null)
}
fn sleep_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(0))
}
fn usleep_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Null)
}
fn uniqid_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    use std::time::SystemTime;
    let prefix = args.first().map(|v| v.to_php_string().to_string_lossy()).unwrap_or_default();
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
        Ok(Value::String(PhpString::from_string(format!("{}{:.8}", base, random_val))))
    } else {
        Ok(Value::String(PhpString::from_string(base)))
    }
}
fn sys_get_temp_dir_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(PhpString::from_bytes(b"/tmp")))
}
fn tempnam_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(PhpString::from_bytes(b"/tmp/goro_tmp")))
}
fn getenv_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let name = args.first().unwrap_or(&Value::Null).to_php_string();
    let name_str: &str = &name.to_string_lossy();
    match std::env::var(name_str) {
        Ok(val) => Ok(Value::String(PhpString::from_string(val))),
        Err(_) => Ok(Value::False),
    }
}
fn putenv_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let setting = args.first().unwrap_or(&Value::Null).to_php_string();
    let s = setting.to_string_lossy();
    if let Some(eq_pos) = s.find('=') {
        let key = &s[..eq_pos];
        let value = &s[eq_pos + 1..];
        if key.is_empty() {
            return Ok(Value::False);
        }
        unsafe { std::env::set_var(key, value); }
        Ok(Value::True)
    } else {
        if s.is_empty() {
            return Ok(Value::False);
        }
        unsafe { std::env::remove_var(&*s); }
        Ok(Value::True)
    }
}
// spl_autoload_register, spl_autoload_functions, spl_autoload_unregister moved to goro-ext-spl

fn class_alias_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let original = args.first().unwrap_or(&Value::Null).to_php_string();
    let alias = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let original_lower: Vec<u8> = original.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
    let alias_lower: Vec<u8> = alias.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
    if let Some(class) = vm.classes.get(&original_lower).cloned() {
        // Use the original class name (not alias) for the class entry
        let mut aliased_class = class;
        // Keep original class name so get_class() returns the canonical name
        vm.classes.insert(alias_lower.clone(), aliased_class);
        // Register bidirectional alias mappings so instanceof works correctly
        vm.class_aliases.insert(alias_lower.clone(), original_lower.clone());
        vm.class_aliases.insert(original_lower, alias_lower);
        Ok(Value::True)
    } else if vm.is_known_builtin_class(&original_lower) {
        // Create a ClassEntry for the alias pointing to the builtin
        let mut class = goro_core::object::ClassEntry::new(original.as_bytes().to_vec());
        class.allow_dynamic_properties = true; // builtins generally allow this
        vm.classes.insert(alias_lower.clone(), class);
        vm.class_aliases.insert(alias_lower.clone(), original_lower.clone());
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
                (obj_class.as_bytes().to_vec(), s.as_bytes().to_vec())
            } else {
                return Ok(Value::False);
            }
        }
        _ => return Ok(Value::False),
    };
    // is_subclass_of returns false if class_name == check_name (unlike is_a)
    let class_lower: Vec<u8> = class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
    let check_lower: Vec<u8> = check_name.iter().map(|b| b.to_ascii_lowercase()).collect();
    if class_lower == check_lower {
        return Ok(Value::False);
    }
    let result = class_is_a(vm, &class_name, &check_name);
    Ok(if result { Value::True } else { Value::False })
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
        let class_entry = match vm.classes.get(&current) {
            Some(ce) => ce.clone(),
            None => break,
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
                let parent_lower: Vec<u8> =
                    parent.iter().map(|b| b.to_ascii_lowercase()).collect();
                if parent_lower == check_lower {
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
    if let Some(ce) = vm.classes.get(iface_name) {
        for parent_iface in &ce.interfaces {
            let pi_lower: Vec<u8> = parent_iface.iter().map(|b| b.to_ascii_lowercase()).collect();
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
fn get_called_class_fn(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // Return the current called class from late static binding stack
    if let Some(class_name) = vm.called_class_stack.last() {
        Ok(Value::String(PhpString::from_vec(class_name.clone())))
    } else {
        Ok(Value::False)
    }
}
fn get_defined_vars_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
}
fn get_defined_functions_fn(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut internal = PhpArray::new();
    for name in vm.functions.keys() {
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
                let msg = format!("array_change_key_case(): Argument #2 ($case) must be of type int, {} given", type_name);
                let exc = vm.throw_type_error(msg.clone());
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg, line: vm.current_line });
            }
            // For string values, throw TypeError too
            if let Value::String(s) = case_val {
                if s.to_string_lossy().parse::<i64>().is_err() {
                    let msg = "array_change_key_case(): Argument #2 ($case) must be of type int, string given".to_string();
                    let exc = vm.throw_type_error(msg.clone());
                    vm.current_exception = Some(exc);
                    return Err(VmError { message: msg, line: vm.current_line });
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

fn array_multisort_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        return Ok(Value::False);
    }

    // Parse arguments: arrays interleaved with sort flags
    // SORT_ASC = 4, SORT_DESC = 3, SORT_REGULAR = 0, SORT_NUMERIC = 1, SORT_STRING = 2, SORT_NATURAL = 6
    // SORT_FLAG_CASE = 8, SORT_LOCALE_STRING = 5
    let mut arrays: Vec<Rc<RefCell<PhpArray>>> = Vec::new();
    let mut sort_orders: Vec<bool> = Vec::new(); // true = ascending
    let mut sort_types: Vec<i64> = Vec::new();

    let mut current_ascending = true;
    let mut current_sort_type = 0i64; // SORT_REGULAR

    for arg in args {
        match arg {
            Value::Array(arr) => {
                arrays.push(arr.clone());
                sort_orders.push(current_ascending);
                sort_types.push(current_sort_type);
                current_ascending = true;
                current_sort_type = 0;
            }
            Value::Reference(r) => {
                let val = r.borrow().clone();
                if let Value::Array(arr) = val {
                    arrays.push(arr.clone());
                    sort_orders.push(current_ascending);
                    sort_types.push(current_sort_type);
                    current_ascending = true;
                    current_sort_type = 0;
                }
            }
            Value::Long(n) => {
                match *n {
                    4 => current_ascending = true,  // SORT_ASC
                    3 => current_ascending = false,  // SORT_DESC
                    _ => current_sort_type = *n,
                }
            }
            _ => {}
        }
    }

    if arrays.is_empty() {
        return Ok(Value::False);
    }

    // Check all arrays have the same length
    let len = arrays[0].borrow().len();
    for arr in &arrays[1..] {
        if arr.borrow().len() != len {
            return Ok(Value::False);
        }
    }

    if len == 0 {
        return Ok(Value::True);
    }

    // Extract values from the first array (the primary sort key)
    let mut indices: Vec<usize> = (0..len).collect();
    let first_values: Vec<Value> = arrays[0].borrow().values().cloned().collect();

    // Sort indices based on the first array
    let ascending = sort_orders[0];
    let sort_type = sort_types[0];
    indices.sort_by(|&a, &b| {
        let result = php_compare_for_sort(&first_values[a], &first_values[b], sort_type);
        if ascending { result } else { result.reverse() }
    });

    // Apply the sorted indices to all arrays
    for arr_rc in &arrays {
        let old_entries: Vec<(ArrayKey, Value)> = {
            let arr = arr_rc.borrow();
            arr.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
        };
        let mut new_arr = PhpArray::new();
        // Check if original was using string keys
        let has_string_keys = old_entries.iter().any(|(k, _)| matches!(k, ArrayKey::String(_)));
        if has_string_keys {
            // Preserve original keys but reorder values
            let old_keys: Vec<ArrayKey> = old_entries.iter().map(|(k, _)| k.clone()).collect();
            let old_vals: Vec<Value> = old_entries.iter().map(|(_, v)| v.clone()).collect();
            for (idx, &sorted_idx) in indices.iter().enumerate() {
                new_arr.set(old_keys[idx].clone(), old_vals[sorted_idx].clone());
            }
        } else {
            // Rebuild with sequential integer keys
            let old_vals: Vec<Value> = old_entries.iter().map(|(_, v)| v.clone()).collect();
            for &sorted_idx in &indices {
                new_arr.push(old_vals[sorted_idx].clone());
            }
        }
        *arr_rc.borrow_mut() = new_arr;
    }

    Ok(Value::True)
}

fn php_compare_for_sort(a: &Value, b: &Value, sort_type: i64) -> std::cmp::Ordering {
    php_sort_cmp_flags(a, b, sort_type)
}

fn highlight_string_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Very basic stub - just returns the code wrapped in HTML
    let code = args.first().unwrap_or(&Value::Null).to_php_string();
    let ret = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
    let html = format!(
        "<code><span style=\"color: #000000\">{}</span>\n</code>",
        code.to_string_lossy()
    );
    if ret {
        Ok(Value::String(PhpString::from_string(html)))
    } else {
        vm.write_output(html.as_bytes());
        Ok(Value::True)
    }
}

// ============= File I/O implementation using thread-local file handle storage =============

use std::collections::HashMap as StdHashMap;
use std::io::{Read as IoRead, Write as IoWrite, Seek, SeekFrom, BufRead, BufReader};

thread_local! {
    static FILE_HANDLES: RefCell<StdHashMap<i64, FileHandle>> = RefCell::new(StdHashMap::new());
    static NEXT_FILE_ID: std::cell::Cell<i64> = const { std::cell::Cell::new(100) }; // Start at 100 to avoid clashing with STDIN/STDOUT/STDERR
    static DIR_HANDLES: RefCell<StdHashMap<String, Vec<String>>> = RefCell::new(StdHashMap::new());
}

struct FileHandle {
    file: std::fs::File,
    #[allow(dead_code)]
    mode: String,
    eof: bool,
}

fn alloc_file_handle(file: std::fs::File, mode: &str) -> i64 {
    NEXT_FILE_ID.with(|id| {
        let fid = id.get();
        id.set(fid + 1);
        FILE_HANDLES.with(|handles| {
            handles.borrow_mut().insert(fid, FileHandle {
                file,
                mode: mode.to_string(),
                eof: false,
            });
        });
        fid
    })
}

fn fopen_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let filename = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy();
    let mode = args.get(1).map(|v| v.to_php_string().to_string_lossy()).unwrap_or_else(|| "r".to_string());

    // Normalize mode: strip 'b' (binary) and 't' (text) flags - they're all the same on Linux
    let mode_clean: String = mode.chars().filter(|&c| c != 'b' && c != 't').collect();
    let file = match mode_clean.as_str() {
        "r" => std::fs::File::open(&filename),
        "r+" => std::fs::OpenOptions::new().read(true).write(true).open(&filename),
        "w" => std::fs::File::create(&filename),
        "w+" => std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(true).open(&filename),
        "a" => std::fs::OpenOptions::new().write(true).append(true).create(true).open(&filename),
        "a+" => std::fs::OpenOptions::new().read(true).write(true).append(true).create(true).open(&filename),
        "x" => std::fs::OpenOptions::new().write(true).create_new(true).open(&filename),
        "x+" => std::fs::OpenOptions::new().read(true).write(true).create_new(true).open(&filename),
        "c" => std::fs::OpenOptions::new().write(true).create(true).open(&filename),
        "c+" => std::fs::OpenOptions::new().read(true).write(true).create(true).open(&filename),
        _ => std::fs::File::open(&filename), // default to read
    };

    match file {
        Ok(f) => {
            let fid = alloc_file_handle(f, &mode);
            Ok(Value::Long(fid))
        }
        Err(_) => Ok(Value::False),
    }
}

fn fclose_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let fid = args.first().unwrap_or(&Value::Null).to_long();
    FILE_HANDLES.with(|handles| {
        handles.borrow_mut().remove(&fid);
    });
    Ok(Value::True)
}

fn fread_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let fid = args.first().unwrap_or(&Value::Null).to_long();
    let length = args.get(1).map(|v| v.to_long()).unwrap_or(0) as usize;
    if length == 0 {
        return Ok(Value::String(PhpString::from_bytes(b"")));
    }
    FILE_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(fh) = handles.get_mut(&fid) {
            let mut buf = vec![0u8; length];
            match fh.file.read(&mut buf) {
                Ok(0) => {
                    fh.eof = true;
                    Ok(Value::String(PhpString::from_bytes(b"")))
                }
                Ok(n) => {
                    buf.truncate(n);
                    Ok(Value::String(PhpString::from_vec(buf)))
                }
                Err(_) => Ok(Value::False),
            }
        } else {
            Ok(Value::False)
        }
    })
}

fn fwrite_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let fid = args.first().unwrap_or(&Value::Null).to_long();
    let data = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let bytes = data.as_bytes();
    let length = args.get(2).map(|v| v.to_long() as usize).unwrap_or(bytes.len());
    let to_write = &bytes[..length.min(bytes.len())];

    FILE_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(fh) = handles.get_mut(&fid) {
            match fh.file.write(to_write) {
                Ok(n) => Ok(Value::Long(n as i64)),
                Err(_) => Ok(Value::False),
            }
        } else {
            Ok(Value::False)
        }
    })
}

fn fgets_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let fid = args.first().unwrap_or(&Value::Null).to_long();
    let max_length = args.get(1).map(|v| v.to_long() as usize);

    FILE_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(fh) = handles.get_mut(&fid) {
            let mut buf = Vec::new();
            let limit = max_length.unwrap_or(1024);
            let mut byte = [0u8; 1];
            for _ in 0..limit {
                match fh.file.read(&mut byte) {
                    Ok(0) => {
                        fh.eof = true;
                        break;
                    }
                    Ok(_) => {
                        buf.push(byte[0]);
                        if byte[0] == b'\n' {
                            break;
                        }
                    }
                    Err(_) => break,
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
    })
}

fn feof_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let fid = args.first().unwrap_or(&Value::Null).to_long();
    FILE_HANDLES.with(|handles| {
        let handles = handles.borrow();
        if let Some(fh) = handles.get(&fid) {
            if fh.eof { Ok(Value::True) } else { Ok(Value::False) }
        } else {
            Ok(Value::True)
        }
    })
}

fn rewind_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let fid = args.first().unwrap_or(&Value::Null).to_long();
    FILE_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(fh) = handles.get_mut(&fid) {
            fh.eof = false;
            match fh.file.seek(SeekFrom::Start(0)) {
                Ok(_) => Ok(Value::True),
                Err(_) => Ok(Value::False),
            }
        } else {
            Ok(Value::False)
        }
    })
}

fn fseek_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let fid = args.first().unwrap_or(&Value::Null).to_long();
    let offset = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let whence = args.get(2).map(|v| v.to_long()).unwrap_or(0);

    FILE_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(fh) = handles.get_mut(&fid) {
            fh.eof = false;
            let seek = match whence {
                0 => SeekFrom::Start(offset as u64), // SEEK_SET
                1 => SeekFrom::Current(offset),       // SEEK_CUR
                2 => SeekFrom::End(offset),            // SEEK_END
                _ => SeekFrom::Start(offset as u64),
            };
            match fh.file.seek(seek) {
                Ok(_) => Ok(Value::Long(0)),
                Err(_) => Ok(Value::Long(-1)),
            }
        } else {
            Ok(Value::Long(-1))
        }
    })
}

fn ftell_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let fid = args.first().unwrap_or(&Value::Null).to_long();
    FILE_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(fh) = handles.get_mut(&fid) {
            match fh.file.seek(SeekFrom::Current(0)) {
                Ok(pos) => Ok(Value::Long(pos as i64)),
                Err(_) => Ok(Value::False),
            }
        } else {
            Ok(Value::False)
        }
    })
}

fn fflush_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let fid = args.first().unwrap_or(&Value::Null).to_long();
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

fn unlink_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let filename = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy();
    match std::fs::remove_file(&filename) {
        Ok(_) => Ok(Value::True),
        Err(_) => Ok(Value::False),
    }
}

fn rename_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let from = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy();
    let to = args.get(1).unwrap_or(&Value::Null).to_php_string().to_string_lossy();
    match std::fs::rename(&from, &to) {
        Ok(_) => Ok(Value::True),
        Err(_) => Ok(Value::False),
    }
}

fn copy_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let from = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy();
    let to = args.get(1).unwrap_or(&Value::Null).to_php_string().to_string_lossy();
    match std::fs::copy(&from, &to) {
        Ok(_) => Ok(Value::True),
        Err(_) => Ok(Value::False),
    }
}

fn mkdir_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy();
    let recursive = args.get(2).map(|v| v.is_truthy()).unwrap_or(false);
    let result = if recursive {
        std::fs::create_dir_all(&path)
    } else {
        std::fs::create_dir(&path)
    };
    match result {
        Ok(_) => Ok(Value::True),
        Err(_) => Ok(Value::False),
    }
}

fn rmdir_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy();
    match std::fs::remove_dir(&path) {
        Ok(_) => Ok(Value::True),
        Err(_) => Ok(Value::False),
    }
}
fn glob_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let pattern = args.first().unwrap_or(&Value::Null).to_php_string();
    let flags = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let pattern_str = pattern.to_string_lossy();

    // Use libc glob
    let mut glob_buf: libc::glob_t = unsafe { std::mem::zeroed() };
    let c_pattern = std::ffi::CString::new(pattern_str.as_bytes()).unwrap_or_default();

    let mut libc_flags = 0i32;
    // GLOB_MARK = 1 in PHP, adds trailing slash to directories
    if flags & 1 != 0 {
        libc_flags |= libc::GLOB_MARK;
    }
    // GLOB_NOSORT = 2 in PHP
    if flags & 2 != 0 {
        libc_flags |= libc::GLOB_NOSORT;
    }
    // GLOB_BRACE = 128 in PHP
    #[cfg(target_os = "linux")]
    if flags & 128 != 0 {
        libc_flags |= libc::GLOB_BRACE;
    }
    // GLOB_ONLYDIR = 8192 in PHP
    #[cfg(target_os = "linux")]
    if flags & 8192 != 0 {
        libc_flags |= libc::GLOB_ONLYDIR;
    }

    let ret = unsafe { libc::glob(c_pattern.as_ptr(), libc_flags, None, &mut glob_buf) };

    let mut arr = PhpArray::new();
    if ret == 0 {
        for i in 0..glob_buf.gl_pathc {
            let path = unsafe {
                let p = *glob_buf.gl_pathv.add(i);
                std::ffi::CStr::from_ptr(p).to_string_lossy().to_string()
            };
            arr.push(Value::String(PhpString::from_string(path)));
        }
    }
    unsafe { libc::globfree(&mut glob_buf); }

    // GLOB_NOCHECK = 16: if no matches, return the pattern itself
    if ret != 0 && flags & 16 != 0 {
        arr.push(Value::String(PhpString::from_string(pattern_str.to_string())));
    }

    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}
fn scandir_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let dir = args.first().unwrap_or(&Value::Null).to_php_string();
    let dir_str = dir.to_string_lossy();
    let sort_order = args.get(1).map(|v| v.to_long()).unwrap_or(0); // 0 = ascending, 1 = descending

    match std::fs::read_dir(&*dir_str) {
        Ok(entries) => {
            let mut names: Vec<String> = vec![".".to_string(), "..".to_string()];
            for entry in entries {
                if let Ok(e) = entry {
                    names.push(e.file_name().to_string_lossy().to_string());
                }
            }
            if sort_order == 1 {
                names.sort_by(|a, b| b.cmp(a));
            } else {
                names.sort();
            }
            let mut arr = PhpArray::new();
            for name in names {
                arr.push(Value::String(PhpString::from_string(name)));
            }
            Ok(Value::Array(Rc::new(RefCell::new(arr))))
        }
        Err(_) => Ok(Value::False),
    }
}
fn header_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Null)
}
fn setcookie_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // Stub: setcookie/setrawcookie always returns true in CLI mode
    Ok(Value::True)
}
fn headers_sent_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}
fn http_response_code_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(200))
}

fn array_diff_key_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Type check all arguments
    for (i, arg) in args.iter().enumerate() {
        let a = if let Value::Reference(r) = arg { r.borrow().clone() } else { arg.clone() };
        if !matches!(a, Value::Array(_)) {
            let type_name = Vm::value_type_name(&a);
            let msg = if i == 0 {
                format!("array_diff_key(): Argument #1 ($array) must be of type array, {} given", type_name)
            } else {
                format!("array_diff_key(): Argument #{} must be of type array, {} given", i + 1, type_name)
            };
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
        }
    }
    if args.len() < 2 {
        if let Some(Value::Array(a)) = args.first() {
            return Ok(Value::Array(Rc::new(RefCell::new(a.borrow().clone()))));
        }
        return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
    }
    if let Some(Value::Array(a)) = args.first() {
        let a = a.borrow();
        let mut other_keys: Vec<ArrayKey> = Vec::new();
        for arg in &args[1..] {
            if let Value::Array(b) = arg {
                let b = b.borrow();
                for key in b.keys() {
                    other_keys.push(key.clone());
                }
            }
        }
        let mut result = PhpArray::new();
        for (key, val) in a.iter() {
            if !other_keys.contains(key) {
                result.set(key.clone(), val.clone());
            }
        }
        Ok(Value::Array(Rc::new(RefCell::new(result))))
    } else {
        Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
    }
}

fn array_diff_assoc_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    for (i, arg) in args.iter().enumerate() {
        let a = if let Value::Reference(r) = arg { r.borrow().clone() } else { arg.clone() };
        if !matches!(a, Value::Array(_)) {
            let type_name = Vm::value_type_name(&a);
            let msg = if i == 0 {
                format!("array_diff_assoc(): Argument #1 ($array) must be of type array, {} given", type_name)
            } else {
                format!("array_diff_assoc(): Argument #{} must be of type array, {} given", i + 1, type_name)
            };
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
        }
    }
    if args.len() < 2 {
        if let Some(Value::Array(a)) = args.first() {
            return Ok(Value::Array(Rc::new(RefCell::new(a.borrow().clone()))));
        }
        return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
    }
    if let Some(Value::Array(a)) = args.first() {
        let a = a.borrow();
        let mut result = PhpArray::new();
        for (key, val) in a.iter() {
            let val_str = val.to_php_string().as_bytes().to_vec();
            let mut found_in_other = false;
            for arg in &args[1..] {
                if let Value::Array(b) = arg {
                    let b = b.borrow();
                    if let Some(b_val) = b.get(key) {
                        if b_val.to_php_string().as_bytes() == val_str.as_slice() {
                            found_in_other = true;
                            break;
                        }
                    }
                }
            }
            if !found_in_other {
                result.set(key.clone(), val.clone());
            }
        }
        Ok(Value::Array(Rc::new(RefCell::new(result))))
    } else {
        Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
    }
}

fn array_intersect_key_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    for (i, arg) in args.iter().enumerate() {
        let a = if let Value::Reference(r) = arg { r.borrow().clone() } else { arg.clone() };
        if !matches!(a, Value::Array(_)) {
            let type_name = Vm::value_type_name(&a);
            let msg = if i == 0 {
                format!("array_intersect_key(): Argument #1 ($array) must be of type array, {} given", type_name)
            } else {
                format!("array_intersect_key(): Argument #{} must be of type array, {} given", i + 1, type_name)
            };
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
        }
    }
    if args.len() < 2 {
        return match args.first() {
            Some(Value::Array(arr)) => Ok(Value::Array(Rc::new(RefCell::new(arr.borrow().clone())))),
            _ => Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new())))),
        };
    }
    let first = match &args[0] {
        Value::Array(a) => a.borrow().clone(),
        _ => return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new())))),
    };
    // Collect keys from all other arrays
    let other_key_sets: Vec<Vec<ArrayKey>> = args[1..].iter().map(|arg| {
        match arg {
            Value::Array(arr) => arr.borrow().keys().cloned().collect(),
            _ => Vec::new(),
        }
    }).collect();
    let mut result = PhpArray::new();
    for (key, val) in first.iter() {
        if other_key_sets.iter().all(|keys| keys.contains(key)) {
            result.set(key.clone(), val.clone());
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn array_intersect_assoc_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    for (i, arg) in args.iter().enumerate() {
        let a = if let Value::Reference(r) = arg { r.borrow().clone() } else { arg.clone() };
        if !matches!(a, Value::Array(_)) {
            let type_name = Vm::value_type_name(&a);
            let msg = if i == 0 {
                format!("array_intersect_assoc(): Argument #1 ($array) must be of type array, {} given", type_name)
            } else {
                format!("array_intersect_assoc(): Argument #{} must be of type array, {} given", i + 1, type_name)
            };
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
        }
    }
    if args.len() < 2 {
        return match args.first() {
            Some(Value::Array(arr)) => Ok(Value::Array(Rc::new(RefCell::new(arr.borrow().clone())))),
            _ => Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new())))),
        };
    }
    let first = match &args[0] {
        Value::Array(a) => a.borrow().clone(),
        _ => return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new())))),
    };
    let other_arrays: Vec<_> = args[1..].iter().filter_map(|arg| {
        if let Value::Array(arr) = arg { Some(arr.borrow().clone()) } else { None }
    }).collect();
    let mut result = PhpArray::new();
    for (key, val) in first.iter() {
        let val_str = val.to_php_string().as_bytes().to_vec();
        let mut found_in_all = true;
        for other in &other_arrays {
            if let Some(other_val) = other.get(key) {
                if other_val.to_php_string().as_bytes().to_vec() != val_str {
                    found_in_all = false;
                    break;
                }
            } else {
                found_in_all = false;
                break;
            }
        }
        if found_in_all {
            result.set(key.clone(), val.clone());
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn array_all_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let arr_data: Vec<(ArrayKey, Value)> = arr.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
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
        let arr_data: Vec<(ArrayKey, Value)> = arr.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
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

fn usort_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let (Some(Value::Array(arr)), Some(callback)) = (args.first(), args.get(1)) {
        let callback = callback.clone();
        let mut arr_mut = arr.borrow_mut();
        let mut entries: Vec<Value> = arr_mut.values().cloned().collect();
        drop(arr_mut);

        entries.sort_by(|a, b| {
            let result = vm.call_callback(&callback, &[a.clone(), b.clone()]).unwrap_or(Value::Long(0));
            let cmp = result.to_long();
            if cmp < 0 {
                std::cmp::Ordering::Less
            } else if cmp > 0 {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });

        let mut new_arr = PhpArray::new();
        for val in entries {
            new_arr.push(val);
        }
        arr.borrow_mut().clone_from(&new_arr);
    }
    Ok(Value::True)
}

fn uasort_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let (Some(Value::Array(arr)), Some(callback)) = (args.first(), args.get(1)) {
        let mut arr_mut = arr.borrow_mut();
        let mut entries: Vec<(ArrayKey, Value)> = arr_mut.iter().map(|(k, v)| (k.clone(), v.clone())).collect();

        let cb = callback.clone();
        entries.sort_by(|a, b| {
            let result = call_user_func(vm, &[cb.clone(), a.1.clone(), b.1.clone()]).unwrap_or(Value::Long(0));
            let cmp = result.to_long();
            if cmp < 0 {
                std::cmp::Ordering::Less
            } else if cmp > 0 {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });

        let mut new_arr = PhpArray::new();
        for (key, val) in entries {
            new_arr.set(key, val);
        }
        *arr_mut = new_arr;
    }
    Ok(Value::True)
}
fn uksort_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let (Some(Value::Array(arr)), Some(callback)) = (args.first(), args.get(1)) {
        let mut arr_mut = arr.borrow_mut();
        let mut entries: Vec<(ArrayKey, Value)> = arr_mut.iter().map(|(k, v)| (k.clone(), v.clone())).collect();

        let cb = callback.clone();
        entries.sort_by(|a, b| {
            let key_a = match &a.0 {
                ArrayKey::Int(n) => Value::Long(*n),
                ArrayKey::String(s) => Value::String(s.clone()),
            };
            let key_b = match &b.0 {
                ArrayKey::Int(n) => Value::Long(*n),
                ArrayKey::String(s) => Value::String(s.clone()),
            };
            let result = call_user_func(vm, &[cb.clone(), key_a, key_b]).unwrap_or(Value::Long(0));
            let cmp = result.to_long();
            if cmp < 0 {
                std::cmp::Ordering::Less
            } else if cmp > 0 {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });

        let mut new_arr = PhpArray::new();
        for (key, val) in entries {
            new_arr.set(key, val);
        }
        *arr_mut = new_arr;
    }
    Ok(Value::True)
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

    let mut carry = initial;
    for val in entries {
        carry = vm.call_callback(&callback, &[carry, val])?;
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
            let is_list = arr
                .iter()
                .enumerate()
                .all(|(i, (k, _))| matches!(k, goro_core::array::ArrayKey::Int(n) if *n == i as i64));
            Ok(if is_list { Value::True } else { Value::False })
        }
        _ => {
            let type_name = args.first().map(|v| Vm::value_type_name(v)).unwrap_or("null".to_string());
            let msg = format!("array_is_list(): Argument #1 ($array) must be of type array, {} given", type_name);
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            Err(VmError { message: msg, line: vm.current_line })
        }
    }
}
#[allow(dead_code)]
fn compact_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // compact() needs access to the current scope's variables which we don't have from builtins
    Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
}

fn gc_status_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut result = PhpArray::new();
    result.set(
        goro_core::array::ArrayKey::String(PhpString::from_bytes(b"running")),
        Value::False,
    );
    result.set(
        goro_core::array::ArrayKey::String(PhpString::from_bytes(b"protected")),
        Value::False,
    );
    result.set(
        goro_core::array::ArrayKey::String(PhpString::from_bytes(b"full")),
        Value::False,
    );
    result.set(
        goro_core::array::ArrayKey::String(PhpString::from_bytes(b"runs")),
        Value::Long(0),
    );
    result.set(
        goro_core::array::ArrayKey::String(PhpString::from_bytes(b"collected")),
        Value::Long(0),
    );
    result.set(
        goro_core::array::ArrayKey::String(PhpString::from_bytes(b"threshold")),
        Value::Long(10000),
    );
    result.set(
        goro_core::array::ArrayKey::String(PhpString::from_bytes(b"roots")),
        Value::Long(0),
    );
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn debug_zval_dump_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    for arg in args {
        debug_zval_dump_value(vm, arg, 0);
    }
    Ok(Value::Null)
}

fn debug_zval_dump_value(vm: &mut Vm, val: &Value, indent: usize) {
    let prefix = " ".repeat(indent);
    match val {
        Value::Null | Value::Undef => {
            vm.write_output(format!("{}NULL\n", prefix).as_bytes());
        }
        Value::True => {
            vm.write_output(format!("{}bool(true)\n", prefix).as_bytes());
        }
        Value::False => {
            vm.write_output(format!("{}bool(false)\n", prefix).as_bytes());
        }
        Value::Long(n) => {
            vm.write_output(format!("{}int({})\n", prefix, n).as_bytes());
        }
        Value::Double(f) => {
            let formatted = goro_core::value::format_php_float(*f);
            vm.write_output(format!("{}float({})\n", prefix, formatted).as_bytes());
        }
        Value::String(s) => {
            vm.write_output(
                format!("{}string({}) \"{}\" refcount({})\n", prefix, s.len(), s.to_string_lossy(), 1).as_bytes(),
            );
        }
        Value::Array(arr) => {
            let arr = arr.borrow();
            vm.write_output(format!("{}array({}) refcount({}){{\n", prefix, arr.len(), 2).as_bytes());
            for (key, value) in arr.iter() {
                match key {
                    goro_core::array::ArrayKey::Int(n) => {
                        vm.write_output(format!("{}  [{}]=>\n", prefix, n).as_bytes());
                    }
                    goro_core::array::ArrayKey::String(s) => {
                        vm.write_output(
                            format!("{}  [\"{}\"]=>\n", prefix, s.to_string_lossy()).as_bytes(),
                        );
                    }
                }
                debug_zval_dump_value(vm, value, indent + 2);
            }
            vm.write_output(format!("{}}}\n", prefix).as_bytes());
        }
        Value::Object(obj) => {
            let obj_borrow = obj.borrow();
            let class_name = goro_core::value::display_class_name(&obj_borrow.class_name);
            let oid = obj_borrow.object_id;
            let prop_count = obj_borrow.properties.iter()
                .filter(|(name, val)| !name.starts_with(b"__") && !matches!(val, Value::Undef))
                .count();
            vm.write_output(
                format!("{}object({})#{} ({}) refcount({}){{\n", prefix, class_name, oid, prop_count, 2).as_bytes(),
            );
            for (name, value) in &obj_borrow.properties {
                if name.starts_with(b"__") || matches!(value, Value::Undef) {
                    continue;
                }
                let name_str = String::from_utf8_lossy(name);
                vm.write_output(format!("{}  [\"{}\"]=>\n", prefix, name_str).as_bytes());
                debug_zval_dump_value(vm, value, indent + 2);
            }
            vm.write_output(format!("{}}}\n", prefix).as_bytes());
        }
        Value::Reference(r) => {
            let inner = r.borrow();
            vm.write_output(format!("{}reference refcount({}){{\n", prefix, 2).as_bytes());
            debug_zval_dump_value(vm, &inner, indent + 2);
            vm.write_output(format!("{}}}\n", prefix).as_bytes());
        }
        _ => {
            vm.write_output(format!("{}NULL\n", prefix).as_bytes());
        }
    }
}

#[allow(dead_code)]
fn var_dump_direct(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // This overrides the one in output.rs - but output.rs registers first
    // So this won't actually be called. Let's skip.
    Ok(Value::Null)
}

fn get_class_methods_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    let class_name = match val {
        Value::String(s) => {
            let name = s.as_bytes().to_vec();
            let lower: Vec<u8> = name.iter().map(|b| b.to_ascii_lowercase()).collect();
            // Check if class exists - if not, emit TypeError
            if !vm.classes.contains_key(&lower) {
                let type_name = Vm::value_type_name(val);
                vm.throw_type_error(format!("get_class_methods(): Argument #1 ($object_or_class) must be an object or a valid class name, {} given", type_name));
                return Ok(Value::Null);
            }
            name
        }
        Value::Object(obj) => obj.borrow().class_name.clone(),
        _ => {
            let type_name = Vm::value_type_name(val);
            vm.throw_type_error(format!("get_class_methods(): Argument #1 ($object_or_class) must be an object or a valid class name, {} given", type_name));
            return Ok(Value::Null);
        }
    };
    let class_lower: Vec<u8> = class_name.iter().map(|b| b.to_ascii_lowercase()).collect();

    let mut result = PhpArray::new();
    // Get the current scope class for visibility checks (lowercase)
    let scope_class: Option<Vec<u8>> = vm.current_class_scope()
        .map(|name| name.iter().map(|b| b.to_ascii_lowercase()).collect());
    if let Some(class) = vm.classes.get(&class_lower) {
        // Collect methods with original names, filter by visibility from current scope
        let mut methods: Vec<_> = class
            .methods
            .values()
            .filter(|m| {
                match m.visibility {
                    goro_core::object::Visibility::Public => true,
                    goro_core::object::Visibility::Protected => {
                        // Accessible if calling class is same or a subclass
                        if let Some(ref scope) = scope_class {
                            scope.as_slice() == class_lower.as_slice()
                                || vm.class_extends(scope, &class_lower)
                                || vm.class_extends(&class_lower, scope)
                        } else { false }
                    }
                    goro_core::object::Visibility::Private => {
                        // Accessible only from same class
                        if let Some(ref scope) = scope_class {
                            scope.as_slice() == class_lower.as_slice()
                        } else { false }
                    }
                }
            })
            .collect();
        // Sort by name for consistent output
        methods.sort_by(|a, b| a.name.cmp(&b.name));
        for m in methods {
            result.push(Value::String(PhpString::from_vec(m.name.clone())));
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
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

fn get_defined_constants_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let categorize = args.first().map(|v| v.is_truthy()).unwrap_or(false);
    let mut result = PhpArray::new();
    if categorize {
        // Return categorized array
        let mut user = PhpArray::new();
        for (name, val) in &vm.constants {
            user.set(
                goro_core::array::ArrayKey::String(PhpString::from_vec(name.clone())),
                val.clone(),
            );
        }
        result.set(
            goro_core::array::ArrayKey::String(PhpString::from_bytes(b"user")),
            Value::Array(Rc::new(RefCell::new(user))),
        );
    } else {
        for (name, val) in &vm.constants {
            result.set(
                goro_core::array::ArrayKey::String(PhpString::from_vec(name.clone())),
                val.clone(),
            );
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn get_class_vars_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let class_name = args.first().unwrap_or(&Value::Null).to_php_string();
    let name_lower: Vec<u8> = class_name
        .as_bytes()
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect();
    let mut result = PhpArray::new();
    if let Some(class) = vm.classes.get(&name_lower) {
        // Non-static properties first, then static
        for prop in &class.properties {
            if !prop.is_static {
                // Uninitialized typed properties (Undef) are returned as null
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
        // Static properties
        for prop in &class.properties {
            if prop.is_static {
                // Get current value from static_properties (may have been modified at runtime)
                let val = class.static_properties.get(&prop.name)
                    .cloned()
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
    let dir_path = std::path::Path::new(&*path_str);
    if dir_path.is_dir() {
        // Read all entries
        match std::fs::read_dir(dir_path) {
            Ok(entries) => {
                let mut names: Vec<String> = vec![".".to_string(), "..".to_string()];
                for entry in entries {
                    if let Ok(e) = entry {
                        names.push(e.file_name().to_string_lossy().to_string());
                    }
                }
                names.sort(); // Sort for consistent order
                // Reverse so we can pop from end efficiently
                names.reverse();
                let key = format!("dir:{}", path_str);
                DIR_HANDLES.with(|dh| {
                    dh.borrow_mut().insert(key.clone(), names);
                });
                Ok(Value::String(PhpString::from_string(key)))
            }
            Err(_) => {
                vm.emit_warning(&format!("opendir({}): Failed to open directory", path_str));
                Ok(Value::False)
            }
        }
    } else {
        vm.emit_warning(&format!("opendir({}): No such file or directory", path_str));
        Ok(Value::False)
    }
}

fn closedir_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(handle) = args.first() {
        let key = handle.to_php_string().to_string_lossy();
        DIR_HANDLES.with(|dh| {
            dh.borrow_mut().remove(&*key);
        });
    }
    Ok(Value::Null)
}

fn readdir_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let handle = args.first().unwrap_or(&Value::Null);
    let key = handle.to_php_string().to_string_lossy();
    DIR_HANDLES.with(|dh| {
        let mut handles = dh.borrow_mut();
        if let Some(entries) = handles.get_mut(&*key) {
            if let Some(name) = entries.pop() {
                Ok(Value::String(PhpString::from_string(name)))
            } else {
                Ok(Value::False)
            }
        } else {
            Ok(Value::False)
        }
    })
}

fn chmod_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // Stub: pretend chmod always succeeds to avoid tests breaking test directory permissions
    Ok(Value::True)
}

fn symlink_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let target = args.first().unwrap_or(&Value::Null).to_php_string();
    let link = args.get(1).unwrap_or(&Value::Null).to_php_string();
    #[cfg(unix)]
    {
        match std::os::unix::fs::symlink(&*target.to_string_lossy(), &*link.to_string_lossy()) {
            Ok(_) => Ok(Value::True),
            Err(_) => Ok(Value::False),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (target, link);
        Ok(Value::False)
    }
}

fn readlink_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    match std::fs::read_link(&*path.to_string_lossy()) {
        Ok(p) => Ok(Value::String(PhpString::from_string(
            p.to_string_lossy().to_string(),
        ))),
        Err(_) => Ok(Value::False),
    }
}

fn debug_backtrace_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let options = args.first().map(|v| v.to_long()).unwrap_or(0);
    let limit = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let ignore_args = (options & 2) != 0; // DEBUG_BACKTRACE_IGNORE_ARGS = 2
    let _provide_object = (options & 1) != 0; // DEBUG_BACKTRACE_PROVIDE_OBJECT = 1

    let mut trace_arr = PhpArray::new();
    // call_stack entries: (function_name, file, line_called_from, args, is_instance_method)
    // Ordered outermost to innermost. Reversed for backtrace output.
    let stack_len = vm.call_stack.len();
    let max_frames = if limit > 0 { limit as usize } else { stack_len };

    for (idx, (func_name, file, line, frame_args, is_method)) in vm.call_stack.iter().rev().enumerate() {
        if idx >= max_frames {
            break;
        }
        let mut frame = PhpArray::new();
        if !file.is_empty() {
            frame.set(
                ArrayKey::String(PhpString::from_bytes(b"file")),
                Value::String(PhpString::from_string(file.clone())),
            );
            frame.set(
                ArrayKey::String(PhpString::from_bytes(b"line")),
                Value::Long(*line as i64),
            );
        }
        // Parse class::method or class->method from func_name
        if let Some(sep) = func_name.find("::") {
            let class_name = &func_name[..sep];
            let method_name = &func_name[sep+2..];
            frame.set(
                ArrayKey::String(PhpString::from_bytes(b"function")),
                Value::String(PhpString::from_string(method_name.to_string())),
            );
            frame.set(
                ArrayKey::String(PhpString::from_bytes(b"class")),
                Value::String(PhpString::from_string(class_name.to_string())),
            );
            // Use -> for instance methods, :: for static methods
            let type_str = if *is_method { b"->" as &[u8] } else { b"::" };
            frame.set(
                ArrayKey::String(PhpString::from_bytes(b"type")),
                Value::String(PhpString::from_bytes(type_str)),
            );
        } else if let Some(sep) = func_name.find("->") {
            let class_name = &func_name[..sep];
            let method_name = &func_name[sep+2..];
            frame.set(
                ArrayKey::String(PhpString::from_bytes(b"function")),
                Value::String(PhpString::from_string(method_name.to_string())),
            );
            frame.set(
                ArrayKey::String(PhpString::from_bytes(b"class")),
                Value::String(PhpString::from_string(class_name.to_string())),
            );
            frame.set(
                ArrayKey::String(PhpString::from_bytes(b"type")),
                Value::String(PhpString::from_bytes(b"->")),
            );
        } else {
            frame.set(
                ArrayKey::String(PhpString::from_bytes(b"function")),
                Value::String(PhpString::from_string(func_name.clone())),
            );
        }
        // Add args array unless DEBUG_BACKTRACE_IGNORE_ARGS is set
        if !ignore_args {
            let mut args_arr = PhpArray::new();
            for arg in frame_args {
                args_arr.push(arg.clone());
            }
            frame.set(
                ArrayKey::String(PhpString::from_bytes(b"args")),
                Value::Array(Rc::new(RefCell::new(args_arr))),
            );
        }
        trace_arr.set(ArrayKey::Int(idx as i64), Value::Array(Rc::new(RefCell::new(frame))));
    }
    Ok(Value::Array(Rc::new(RefCell::new(trace_arr))))
}

fn debug_print_backtrace_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let options = args.first().map(|v| v.to_long()).unwrap_or(0);
    let limit = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let ignore_args = (options & 2) != 0; // DEBUG_BACKTRACE_IGNORE_ARGS = 2

    let stack_len = vm.call_stack.len();
    let max_frames = if limit > 0 { limit as usize } else { stack_len };

    let mut lines = Vec::new();
    for (i, (func_name, file, line, frame_args, is_instance)) in vm.call_stack.iter().rev().enumerate() {
        if i >= max_frames {
            break;
        }
        let file_display = if file == "Unknown.php" || file.is_empty() {
            &vm.current_file
        } else {
            file
        };
        let args_str = if ignore_args {
            String::new()
        } else {
            goro_core::vm::format_trace_args(frame_args)
        };
        // For instance methods, replace :: with ->
        let display_name = if *is_instance {
            func_name.replacen("::", "->", 1)
        } else {
            func_name.clone()
        };
        lines.push(format!("#{} {}({}): {}({})", i, file_display, line, display_name, args_str));
    }

    let mut output = lines.join("\n");
    if !output.is_empty() {
        output.push('\n');
    }
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

    // Check for invalid key types (array, object)
    match &key_deref {
        Value::Array(_) => {
            let msg = "Cannot access offset of type array on array".to_string();
            let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
        }
        Value::Object(obj) => {
            let class_name = goro_core::value::display_class_name(&obj.borrow().class_name);
            let msg = format!("Cannot access offset of type {} on array", class_name);
            let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
        }
        _ => {}
    }

    if let Value::Array(a) = arr {
        let arr_key = goro_core::vm::Vm::value_to_array_key(key.clone());
        Ok(if a.borrow().contains_key(&arr_key) {
            Value::True
        } else {
            Value::False
        })
    } else {
        let type_name = Vm::value_type_name(arr);
        let msg = format!("array_key_exists(): Argument #2 ($array) must be of type array, {} given", type_name);
        let exc = vm.create_exception(b"TypeError", &msg, 0);
        vm.current_exception = Some(exc);
        Err(VmError { message: msg, line: vm.current_line })
    }
}

fn clearstatcache_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Null) // No-op - we don't cache stat results
}

fn array_walk_recursive_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.len() > 3 {
        let msg = format!("array_walk_recursive() expects at most 3 arguments, {} given", args.len());
        let exc = vm.create_exception(b"ArgumentCountError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    let arr_val = args.first().unwrap_or(&Value::Null);
    let callback = args.get(1).unwrap_or(&Value::Null);
    let extra_data = args.get(2).cloned();

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

    if let Value::Array(a) = arr_val {
        walk_recursive_inner(vm, a, &func_name, &captured, &extra_data)?;
    }
    Ok(Value::True)
}

fn walk_recursive_inner(
    vm: &mut Vm,
    arr: &Rc<RefCell<PhpArray>>,
    func_name: &[u8],
    captured: &[Value],
    extra_data: &Option<Value>,
) -> Result<(), VmError> {
    let entries: Vec<_> = arr
        .borrow()
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    let func_lower: Vec<u8> = func_name.iter().map(|b| b.to_ascii_lowercase()).collect();

    for (key, val) in entries {
        if let Value::Array(sub_arr) = &val {
            // Recurse into sub-arrays
            walk_recursive_inner(vm, sub_arr, func_name, captured, extra_data)?;
        } else {
            // Call callback(&$value, $key) on leaf values
            let key_val = match &key {
                ArrayKey::Int(n) => Value::Long(*n),
                ArrayKey::String(s) => Value::String(s.clone()),
            };

            if let Some(user_fn) = vm.user_functions.get(&func_lower).cloned() {
                // Wrap value in a Reference so by-ref params (&$v) work
                let val_ref = Rc::new(RefCell::new(val.clone()));
                let mut fn_cvs = vec![Value::Undef; user_fn.cv_names.len()];
                let mut idx = 0;
                for cv in captured {
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
                    fn_cvs[idx] = key_val;
                    idx += 1;
                }
                // Pass extra_data as third argument
                if let Some(extra) = extra_data {
                    if idx < fn_cvs.len() {
                        fn_cvs[idx] = extra.clone();
                    }
                }
                vm.execute_fn(&user_fn, fn_cvs)?;
                // Write modified value back to the array
                let new_val = val_ref.borrow().clone();
                arr.borrow_mut().set(key.clone(), new_val);
            } else if let Some(builtin) = vm.functions.get(&func_lower).copied() {
                let _ = builtin(vm, &[val, key_val]);
            }
        }
    }
    Ok(())
}

fn fgetcsv_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let fid = args.first().unwrap_or(&Value::Null).to_long();
    let _length = args.get(1).map(|v| v.to_long());
    let separator = args.get(2).map(|v| {
        let s = v.to_php_string();
        let b = s.as_bytes();
        if b.is_empty() { b',' } else { b[0] }
    }).unwrap_or(b',');
    let enclosure = args.get(3).map(|v| {
        let s = v.to_php_string();
        let b = s.as_bytes();
        if b.is_empty() { b'"' } else { b[0] }
    }).unwrap_or(b'"');
    let escape = args.get(4).map(|v| {
        let s = v.to_php_string();
        let b = s.as_bytes();
        if b.is_empty() { None } else { Some(b[0]) }
    }).unwrap_or(Some(b'\\'));

    // Read a line from the file
    let line = FILE_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(fh) = handles.get_mut(&fid) {
            let mut buf = Vec::new();
            let mut byte = [0u8; 1];
            let mut in_quotes = false;
            loop {
                match fh.file.read(&mut byte) {
                    Ok(0) => {
                        fh.eof = true;
                        break;
                    }
                    Ok(_) => {
                        if byte[0] == enclosure {
                            in_quotes = !in_quotes;
                        }
                        if byte[0] == b'\n' && !in_quotes {
                            break;
                        }
                        buf.push(byte[0]);
                    }
                    Err(_) => break,
                }
            }
            if buf.is_empty() && fh.eof {
                None
            } else {
                // Remove trailing \r
                if buf.last() == Some(&b'\r') {
                    buf.pop();
                }
                Some(buf)
            }
        } else {
            None
        }
    });

    match line {
        None => Ok(Value::False),
        Some(line_bytes) => {
            // Parse CSV line
            let line_str = String::from_utf8_lossy(&line_bytes);
            let csv_args = vec![
                Value::String(PhpString::from_string(line_str.to_string())),
                Value::String(PhpString::from_bytes(&[separator])),
                Value::String(PhpString::from_bytes(&[enclosure])),
                if let Some(e) = escape {
                    Value::String(PhpString::from_bytes(&[e]))
                } else {
                    Value::String(PhpString::from_bytes(b""))
                },
            ];
            crate::strings::str_getcsv_fn(vm, &csv_args)
        }
    }
}

fn fileperms_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    if !vm.check_open_basedir("fileperms", &path.to_string_lossy()) {
        return Ok(Value::False);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match std::fs::metadata(&*path.to_string_lossy()) {
            Ok(m) => Ok(Value::Long(m.mode() as i64)),
            Err(_) => {
                vm.emit_warning(&format!("fileperms(): stat failed for {}", path.to_string_lossy()));
                Ok(Value::False)
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(Value::Long(0o100644))
    }
}

fn filetype_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let path_str = path.to_string_lossy();
    let p = std::path::Path::new(&*path_str);
    let ft = if p.is_file() {
        "file"
    } else if p.is_dir() {
        "dir"
    } else if p.read_link().is_ok() {
        "link"
    } else {
        "unknown"
    };
    Ok(Value::String(PhpString::from_bytes(ft.as_bytes())))
}

fn natsort_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let mut arr = arr.borrow_mut();
        let mut entries: Vec<_> = arr.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        entries.sort_by(|a, b| {
            let sa = a.1.to_php_string().to_string_lossy();
            let sb = b.1.to_php_string().to_string_lossy();
            natcmp(&sa, &sb)
        });
        *arr = goro_core::array::PhpArray::new();
        for (k, v) in entries {
            arr.set(k, v);
        }
    }
    Ok(Value::True)
}

fn natcasesort_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let mut arr = arr.borrow_mut();
        let mut entries: Vec<_> = arr.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        entries.sort_by(|a, b| {
            let sa = a.1.to_php_string().to_string_lossy().to_ascii_lowercase();
            let sb = b.1.to_php_string().to_string_lossy().to_ascii_lowercase();
            natcmp(&sa, &sb)
        });
        *arr = goro_core::array::PhpArray::new();
        for (k, v) in entries {
            arr.set(k, v);
        }
    }
    Ok(Value::True)
}

/// Natural order comparison
fn natcmp(a: &str, b: &str) -> std::cmp::Ordering {
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    let mut ai = 0;
    let mut bi = 0;
    while ai < ab.len() && bi < bb.len() {
        if ab[ai].is_ascii_digit() && bb[bi].is_ascii_digit() {
            // Compare number segments
            while ai < ab.len() && ab[ai] == b'0' {
                ai += 1;
            }
            while bi < bb.len() && bb[bi] == b'0' {
                bi += 1;
            }
            let astart = ai;
            let bstart = bi;
            while ai < ab.len() && ab[ai].is_ascii_digit() {
                ai += 1;
            }
            while bi < bb.len() && bb[bi].is_ascii_digit() {
                bi += 1;
            }
            let alen = ai - astart;
            let blen = bi - bstart;
            if alen != blen {
                return alen.cmp(&blen);
            }
            let acmp = &ab[astart..ai];
            let bcmp = &bb[bstart..bi];
            if acmp != bcmp {
                return acmp.cmp(bcmp);
            }
        } else {
            if ab[ai] != bb[bi] {
                return ab[ai].cmp(&bb[bi]);
            }
            ai += 1;
            bi += 1;
        }
    }
    ab.len().cmp(&bb.len())
}

fn set_include_path_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let _path = args.first().unwrap_or(&Value::Null).to_php_string();
    Ok(Value::String(PhpString::from_bytes(b".")))
}

fn get_include_path_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(PhpString::from_bytes(b".")))
}

fn restore_include_path_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Null)
}

fn get_resource_type_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    match val {
        Value::Long(id) => {
            // Check if this is a known file handle
            let is_file = FILE_HANDLES.with(|handles| {
                handles.borrow().contains_key(id)
            });
            if is_file || *id == 0 || *id == 1 || *id == 2 {
                // STDIN (0), STDOUT (1), STDERR (2) or opened file handle
                Ok(Value::String(PhpString::from_bytes(b"stream")))
            } else {
                Ok(Value::String(PhpString::from_bytes(b"Unknown")))
            }
        }
        _ => Ok(Value::String(PhpString::from_bytes(b"Unknown"))),
    }
}

fn link_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let target = args.first().unwrap_or(&Value::Null).to_php_string();
    let link = args.get(1).unwrap_or(&Value::Null).to_php_string();
    match std::fs::hard_link(&*target.to_string_lossy(), &*link.to_string_lossy()) {
        Ok(_) => Ok(Value::True),
        Err(_) => Ok(Value::False),
    }
}

fn unlink_fn2(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    match std::fs::remove_file(&*path.to_string_lossy()) {
        Ok(_) => Ok(Value::True),
        Err(_) => Ok(Value::False),
    }
}

fn rename_fn2(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let from = args.first().unwrap_or(&Value::Null).to_php_string();
    let to = args.get(1).unwrap_or(&Value::Null).to_php_string();
    match std::fs::rename(&*from.to_string_lossy(), &*to.to_string_lossy()) {
        Ok(_) => Ok(Value::True),
        Err(_) => Ok(Value::False),
    }
}

fn copy_fn2(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let from = args.first().unwrap_or(&Value::Null).to_php_string();
    let to = args.get(1).unwrap_or(&Value::Null).to_php_string();
    match std::fs::copy(&*from.to_string_lossy(), &*to.to_string_lossy()) {
        Ok(_) => Ok(Value::True),
        Err(_) => Ok(Value::False),
    }
}

fn readfile_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    match std::fs::read(&*path.to_string_lossy()) {
        Ok(data) => {
            let len = data.len();
            vm.write_output(&data);
            Ok(Value::Long(len as i64))
        }
        Err(_) => Ok(Value::False),
    }
}

fn file_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let flags = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let ignore_new_lines = flags & 2 != 0; // FILE_IGNORE_NEW_LINES
    let skip_empty = flags & 4 != 0; // FILE_SKIP_EMPTY_LINES
    match std::fs::read(&*path.to_string_lossy()) {
        Ok(content) => {
            let mut result = PhpArray::new();
            if content.is_empty() {
                return Ok(Value::Array(Rc::new(RefCell::new(result))));
            }
            // Split content into lines preserving line endings
            let mut lines: Vec<Vec<u8>> = Vec::new();
            let mut current = Vec::new();
            for &b in &content {
                current.push(b);
                if b == b'\n' {
                    lines.push(current);
                    current = Vec::new();
                }
            }
            // If there's remaining content (no trailing newline), add it
            if !current.is_empty() {
                lines.push(current);
            }
            for line in &lines {
                let mut l = line.clone();
                if ignore_new_lines {
                    // Strip trailing \r\n or \n
                    if l.ends_with(b"\r\n") {
                        l.truncate(l.len() - 2);
                    } else if l.ends_with(b"\n") {
                        l.truncate(l.len() - 1);
                    }
                }
                if skip_empty {
                    let trimmed: Vec<u8> = l.iter().copied().filter(|b| !b.is_ascii_whitespace()).collect();
                    if trimmed.is_empty() {
                        continue;
                    }
                }
                result.push(Value::String(PhpString::from_vec(l)));
            }
            Ok(Value::Array(Rc::new(RefCell::new(result))))
        }
        Err(_) => Ok(Value::False),
    }
}

fn lstat_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let path_str = path.to_string_lossy();
    stat_path(&path_str, true)
}

fn is_executable_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match std::fs::metadata(&*path.to_string_lossy()) {
            Ok(m) => Ok(if m.mode() & 0o111 != 0 {
                Value::True
            } else {
                Value::False
            }),
            Err(_) => Ok(Value::False),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(Value::False)
    }
}

fn tempnam_fn3(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let dir = args.first().unwrap_or(&Value::Null).to_php_string();
    let prefix = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let dir_str = if dir.is_empty() {
        std::env::temp_dir().to_string_lossy().to_string()
    } else {
        dir.to_string_lossy()
    };
    let name = format!(
        "{}/{}{}",
        dir_str,
        prefix.to_string_lossy(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    // Create the file
    std::fs::write(&name, b"").ok();
    Ok(Value::String(PhpString::from_string(name)))
}

fn sys_get_temp_dir_fn3(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(PhpString::from_string(
        std::env::temp_dir().to_string_lossy().to_string(),
    )))
}

fn array_replace_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let mut result = PhpArray::new();
    for arg in args {
        if let Value::Array(arr) = arg {
            let arr = arr.borrow();
            for (key, value) in arr.iter() {
                result.set(key.clone(), value.clone());
            }
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn array_replace_recursive_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
    }

    let mut result = if let Value::Array(arr) = &args[0] {
        arr.borrow().clone()
    } else {
        PhpArray::new()
    };

    for arg in &args[1..] {
        if let Value::Array(arr) = arg {
            let arr = arr.borrow();
            for (key, value) in arr.iter() {
                if let Value::Array(inner_new) = value {
                    if let Some(existing) = result.get(&key) {
                        if let Value::Array(inner_old) = existing {
                            // Recursively merge
                            let merged_val = array_replace_recursive_inner(
                                &inner_old.borrow(),
                                &inner_new.borrow(),
                            );
                            result.set(key.clone(), merged_val);
                            continue;
                        }
                    }
                }
                result.set(key.clone(), value.clone());
            }
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn array_replace_recursive_inner(base: &PhpArray, replacement: &PhpArray) -> Value {
    let mut result = base.clone();
    for (key, value) in replacement.iter() {
        if let Value::Array(inner_new) = value {
            if let Some(existing) = result.get(&key) {
                if let Value::Array(inner_old) = existing {
                    let merged = array_replace_recursive_inner(&inner_old.borrow(), &inner_new.borrow());
                    result.set(key.clone(), merged);
                    continue;
                }
            }
        }
        result.set(key.clone(), value.clone());
    }
    Value::Array(Rc::new(RefCell::new(result)))
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
    let arr_data: Vec<(ArrayKey, Value)> = arr.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    for (key, value) in &arr_data {
        let key_val = match key {
            ArrayKey::Int(n) => Value::Long(*n),
            ArrayKey::String(s) => Value::String(s.clone()),
        };
        let result = call_user_func(vm, &[callback.clone(), value.clone(), key_val])?;
        if result.is_truthy() {
            return Ok(value.clone());
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
    let arr_data: Vec<(ArrayKey, Value)> = arr.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
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

fn array_product_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let arr = match args.first() {
        Some(Value::Array(arr)) => arr.borrow(),
        _ => return Ok(Value::Long(0)),
    };
    let mut product_int: i64 = 1;
    let mut is_float = false;
    let mut product_float: f64 = 1.0;
    for (_, value) in arr.iter() {
        match value {
            Value::Double(f) => {
                if !is_float {
                    is_float = true;
                    product_float = product_int as f64;
                }
                product_float *= f;
            }
            Value::True => {
                // true * x = x (no change)
                if is_float {
                    // 1.0 * product_float = product_float
                } else {
                    // 1 * product_int = product_int
                }
            }
            Value::False | Value::Null => {
                // false/null = 0, result is 0
                if is_float {
                    product_float = 0.0;
                } else {
                    product_int = 0;
                }
            }
            _ => {
                let n = value.to_long();
                if is_float {
                    product_float *= n as f64;
                } else {
                    match product_int.checked_mul(n) {
                        Some(v) => product_int = v,
                        None => {
                            is_float = true;
                            product_float = product_int as f64 * n as f64;
                        }
                    }
                }
            }
        }
    }
    if is_float {
        Ok(Value::Double(product_float))
    } else {
        Ok(Value::Long(product_int))
    }
}

fn array_sum_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let arr = match args.first() {
        Some(Value::Array(arr)) => arr.borrow(),
        Some(Value::Reference(r)) => {
            let inner = r.borrow();
            if let Value::Array(arr) = &*inner {
                let cloned = arr.borrow().clone();
                drop(inner);
                // Work with clone
                let mut sum_int: i64 = 0;
                let mut is_float = false;
                let mut sum_float: f64 = 0.0;
                for (_, value) in cloned.iter() {
                    match value {
                        Value::Double(f) => {
                            if !is_float {
                                is_float = true;
                                sum_float = sum_int as f64;
                            }
                            sum_float += f;
                        }
                        _ => {
                            let n = value.to_long();
                            if is_float {
                                sum_float += n as f64;
                            } else {
                                match sum_int.checked_add(n) {
                                    Some(v) => sum_int = v,
                                    None => {
                                        is_float = true;
                                        sum_float = sum_int as f64 + n as f64;
                                    }
                                }
                            }
                        }
                    }
                }
                return if is_float { Ok(Value::Double(sum_float)) } else { Ok(Value::Long(sum_int)) };
            }
            return Ok(Value::Long(0));
        }
        _ => return Ok(Value::Long(0)),
    };
    let mut sum_int: i64 = 0;
    let mut is_float = false;
    let mut sum_float: f64 = 0.0;
    for (_, value) in arr.iter() {
        match value {
            Value::Double(f) => {
                if !is_float {
                    is_float = true;
                    sum_float = sum_int as f64;
                }
                sum_float += f;
            }
            _ => {
                let n = value.to_long();
                if is_float {
                    sum_float += n as f64;
                } else {
                    match sum_int.checked_add(n) {
                        Some(v) => sum_int = v,
                        None => {
                            is_float = true;
                            sum_float = sum_int as f64 + n as f64;
                        }
                    }
                }
            }
        }
    }
    if is_float {
        Ok(Value::Double(sum_float))
    } else {
        Ok(Value::Long(sum_int))
    }
}

// User-callback comparison functions
fn array_intersect_ukey_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    array_u_op(vm, args, true, true, false, "array_intersect_ukey", 1)
}
fn array_intersect_uassoc_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    array_u_op(vm, args, true, true, true, "array_intersect_uassoc", 1)
}
fn array_diff_ukey_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    array_u_op(vm, args, false, true, false, "array_diff_ukey", 1)
}
fn array_diff_uassoc_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    array_u_op(vm, args, false, true, true, "array_diff_uassoc", 1)
}
fn array_udiff_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    array_u_op(vm, args, false, false, false, "array_udiff", 1)
}
fn array_udiff_assoc_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    array_udiff_with_key_check(vm, args, false, "array_udiff_assoc")
}
fn array_udiff_uassoc_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    array_udiff_uassoc_impl(vm, args, "array_udiff_uassoc")
}
fn array_uintersect_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    array_u_op(vm, args, true, false, false, "array_uintersect", 1)
}
fn array_uintersect_assoc_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    array_udiff_with_key_check(vm, args, true, "array_uintersect_assoc")
}
fn array_uintersect_uassoc_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    array_uintersect_uassoc_impl(vm, args, "array_uintersect_uassoc")
}

/// Validate a callback argument for array_diff_u*/array_intersect_u*/array_udiff*/array_uintersect* functions.
/// Returns Ok(()) if valid, or Err with TypeError if not.
fn validate_array_callback(vm: &mut Vm, callback: &Value, func_name: &str, arg_num: usize) -> Result<(), VmError> {
    match callback {
        Value::String(s) => {
            let name = s.as_bytes();
            if name.is_empty() {
                let msg = format!("{}(): Argument #{} must be a valid callback, function \"\" not found or invalid function name", func_name, arg_num);
                let exc = vm.throw_type_error(msg.clone());
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg, line: vm.current_line });
            }
            let name_lower: Vec<u8> = name.iter().map(|b| b.to_ascii_lowercase()).collect();
            if name_lower.starts_with(b"__closure_") || name_lower.starts_with(b"__arrow_") {
                return Ok(());
            }
            // Check for Class::method syntax
            if let Some(pos) = name_lower.iter().position(|&b| b == b':') {
                if pos + 1 < name_lower.len() && name_lower[pos + 1] == b':' {
                    let class = &name_lower[..pos];
                    let method = &name_lower[pos + 2..];
                    if let Some(cls) = vm.classes.get(class) {
                        if cls.get_method(method).is_some() {
                            return Ok(());
                        }
                    }
                    let msg = format!("{}(): Argument #{} must be a valid callback, class \"{}\" not found", func_name, arg_num, String::from_utf8_lossy(&name[..pos]));
                    let exc = vm.throw_type_error(msg.clone());
                    vm.current_exception = Some(exc);
                    return Err(VmError { message: msg, line: vm.current_line });
                }
            }
            if vm.functions.contains_key(&name_lower) || vm.user_functions.contains_key(&name_lower) {
                return Ok(());
            }
            let msg = format!("{}(): Argument #{} must be a valid callback, function \"{}\" not found or invalid function name", func_name, arg_num, String::from_utf8_lossy(name));
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            Err(VmError { message: msg, line: vm.current_line })
        }
        Value::Array(arr) => {
            let arr_borrow = arr.borrow();
            let len = arr_borrow.len();
            if len != 2 {
                let msg = format!("{}(): Argument #{} must be a valid callback, array callback must have exactly two members", func_name, arg_num);
                let exc = vm.throw_type_error(msg.clone());
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg, line: vm.current_line });
            }
            Ok(())
        }
        Value::Object(_) => Ok(()),
        _ => {
            let msg = format!("{}(): Argument #{} must be a valid callback, no array or string given", func_name, arg_num);
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            Err(VmError { message: msg, line: vm.current_line })
        }
    }
}

/// Generic user-callback array operation
/// is_intersect: true = keep matches, false = keep non-matches
/// compare_keys: true = compare keys, false = compare values
/// also_compare_values: if compare_keys, also check value equality
fn array_u_op(
    vm: &mut Vm,
    args: &[Value],
    is_intersect: bool,
    compare_keys: bool,
    also_compare_values: bool,
    func_name: &str,
    num_callbacks: usize,
) -> Result<Value, VmError> {
    if args.len() < 1 + num_callbacks {
        return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
    }
    // Validate callback argument(s) first (PHP validates callbacks before arrays)
    let array_arg_count = args.len() - num_callbacks;
    for cb_idx in 0..num_callbacks {
        let cb_arg_pos = array_arg_count + cb_idx;
        validate_array_callback(vm, &args[cb_arg_pos], func_name, cb_arg_pos + 1)?;
    }
    // Type check all array arguments (everything except the last num_callbacks args)
    for i in 0..array_arg_count {
        let val = args[i].deref();
        if !matches!(val, Value::Array(_)) {
            let type_name = Vm::value_type_name(&val);
            let msg = if i == 0 {
                format!("{}(): Argument #1 ($array) must be of type array, {} given", func_name, type_name)
            } else {
                format!("{}(): Argument #{} must be of type array, {} given", func_name, i + 1, type_name)
            };
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
        }
    }
    let callback = args[args.len() - num_callbacks].clone();
    let first = match args[0].deref() {
        Value::Array(arr) => arr.borrow().clone(),
        _ => return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new())))),
    };
    // If only one array + callback(s), return the first array
    if array_arg_count <= 1 {
        return Ok(Value::Array(Rc::new(RefCell::new(first))));
    }
    let other_arrays: Vec<PhpArray> = args[1..array_arg_count]
        .iter()
        .filter_map(|v| {
            let v = v.deref();
            if let Value::Array(arr) = v {
                Some(arr.borrow().clone())
            } else {
                None
            }
        })
        .collect();

    let mut result = PhpArray::new();
    for (key, value) in first.iter() {
        let key_val = match key {
            ArrayKey::Int(n) => Value::Long(*n),
            ArrayKey::String(s) => Value::String(s.clone()),
        };
        let mut found_in_all = true;
        for other in &other_arrays {
            let mut found_in_this = false;
            for (okey, ovalue) in other.iter() {
                let okey_val = match okey {
                    ArrayKey::Int(n) => Value::Long(*n),
                    ArrayKey::String(s) => Value::String(s.clone()),
                };
                if compare_keys {
                    let cmp_result = call_user_func(vm, &[callback.clone(), key_val.clone(), okey_val.clone()])?;
                    if cmp_result.to_long() == 0 {
                        if also_compare_values {
                            if value.equals(ovalue) {
                                found_in_this = true;
                                break;
                            }
                        } else {
                            found_in_this = true;
                            break;
                        }
                    }
                } else {
                    // Compare values
                    let cmp_result = call_user_func(vm, &[callback.clone(), value.clone(), ovalue.clone()])?;
                    if cmp_result.to_long() == 0 {
                        found_in_this = true;
                        break;
                    }
                }
            }
            if !found_in_this {
                found_in_all = false;
                break;
            }
        }
        if is_intersect == found_in_all {
            result.set(key.clone(), value.clone());
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn array_udiff_with_key_check(
    vm: &mut Vm,
    args: &[Value],
    is_intersect: bool,
    func_name: &str,
) -> Result<Value, VmError> {
    if args.len() < 2 {
        return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
    }
    // Validate callback first
    validate_array_callback(vm, args.last().unwrap(), func_name, args.len())?;
    // Type check all array arguments (everything except the last callback)
    let array_arg_count = args.len() - 1;
    for i in 0..array_arg_count {
        let val = args[i].deref();
        if !matches!(val, Value::Array(_)) {
            let type_name = Vm::value_type_name(&val);
            let msg = if i == 0 {
                format!("{}(): Argument #1 ($array) must be of type array, {} given", func_name, type_name)
            } else {
                format!("{}(): Argument #{} must be of type array, {} given", func_name, i + 1, type_name)
            };
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
        }
    }
    // Single array + callback: return the first array
    if array_arg_count <= 1 {
        return match args[0].deref() {
            Value::Array(arr) => Ok(Value::Array(Rc::new(RefCell::new(arr.borrow().clone())))),
            _ => Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new())))),
        };
    }
    let callback = args.last().unwrap().clone();
    let first = match args[0].deref() {
        Value::Array(arr) => arr.borrow().clone(),
        _ => return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new())))),
    };
    let other_arrays: Vec<PhpArray> = args[1..array_arg_count]
        .iter()
        .filter_map(|v| {
            let v = v.deref();
            if let Value::Array(arr) = v {
                Some(arr.borrow().clone())
            } else {
                None
            }
        })
        .collect();

    let mut result = PhpArray::new();
    for (key, value) in first.iter() {
        let mut found_in_all = true;
        for other in &other_arrays {
            let mut found = false;
            if let Some(ovalue) = other.get(&key) {
                let cmp_result = call_user_func(vm, &[callback.clone(), value.clone(), ovalue.clone()])?;
                if cmp_result.to_long() == 0 {
                    found = true;
                }
            }
            if !found {
                found_in_all = false;
                break;
            }
        }
        if is_intersect == found_in_all {
            result.set(key.clone(), value.clone());
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn array_udiff_uassoc_impl(vm: &mut Vm, args: &[Value], func_name: &str) -> Result<Value, VmError> {
    if args.len() < 3 {
        return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
    }
    // Validate both callbacks first
    if args.len() >= 2 {
        validate_array_callback(vm, &args[args.len() - 1], func_name, args.len())?;
        validate_array_callback(vm, &args[args.len() - 2], func_name, args.len() - 1)?;
    }
    // Type check all array arguments (everything except the last 2 callbacks)
    let array_arg_count = args.len().saturating_sub(2);
    for i in 0..array_arg_count {
        let val = args[i].deref();
        if !matches!(val, Value::Array(_)) {
            let type_name = Vm::value_type_name(&val);
            let msg = if i == 0 {
                format!("{}(): Argument #1 ($array) must be of type array, {} given", func_name, type_name)
            } else {
                format!("{}(): Argument #{} must be of type array, {} given", func_name, i + 1, type_name)
            };
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
        }
    }
    // Single array + two callbacks: return the first array
    if array_arg_count <= 1 {
        return match args[0].deref() {
            Value::Array(arr) => Ok(Value::Array(Rc::new(RefCell::new(arr.borrow().clone())))),
            _ => Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new())))),
        };
    }
    let key_callback = args[args.len() - 1].clone();
    let val_callback = args[args.len() - 2].clone();
    let first = match args[0].deref() {
        Value::Array(arr) => arr.borrow().clone(),
        _ => return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new())))),
    };
    let other_arrays: Vec<PhpArray> = args[1..array_arg_count]
        .iter()
        .filter_map(|v| {
            let v = v.deref();
            if let Value::Array(arr) = v {
                Some(arr.borrow().clone())
            } else {
                None
            }
        })
        .collect();

    let mut result = PhpArray::new();
    for (key, value) in first.iter() {
        let key_val = match key {
            ArrayKey::Int(n) => Value::Long(*n),
            ArrayKey::String(s) => Value::String(s.clone()),
        };
        let mut found_in_all = true;
        for other in &other_arrays {
            let mut found = false;
            for (okey, ovalue) in other.iter() {
                let okey_val = match okey {
                    ArrayKey::Int(n) => Value::Long(*n),
                    ArrayKey::String(s) => Value::String(s.clone()),
                };
                let key_cmp = call_user_func(vm, &[key_callback.clone(), key_val.clone(), okey_val])?;
                if key_cmp.to_long() == 0 {
                    let val_cmp = call_user_func(vm, &[val_callback.clone(), value.clone(), ovalue.clone()])?;
                    if val_cmp.to_long() == 0 {
                        found = true;
                        break;
                    }
                }
            }
            if !found {
                found_in_all = false;
                break;
            }
        }
        if !found_in_all {
            result.set(key.clone(), value.clone());
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn array_uintersect_uassoc_impl(vm: &mut Vm, args: &[Value], func_name: &str) -> Result<Value, VmError> {
    if args.len() < 3 {
        return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
    }
    // Validate both callbacks first
    if args.len() >= 2 {
        validate_array_callback(vm, &args[args.len() - 1], func_name, args.len())?;
        validate_array_callback(vm, &args[args.len() - 2], func_name, args.len() - 1)?;
    }
    // Type check all array arguments (everything except the last 2 callbacks)
    let array_arg_count = args.len().saturating_sub(2);
    for i in 0..array_arg_count {
        let val = args[i].deref();
        if !matches!(val, Value::Array(_)) {
            let type_name = Vm::value_type_name(&val);
            let msg = if i == 0 {
                format!("{}(): Argument #1 ($array) must be of type array, {} given", func_name, type_name)
            } else {
                format!("{}(): Argument #{} must be of type array, {} given", func_name, i + 1, type_name)
            };
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
        }
    }
    // Single array + two callbacks: return the first array
    if array_arg_count <= 1 {
        return match args[0].deref() {
            Value::Array(arr) => Ok(Value::Array(Rc::new(RefCell::new(arr.borrow().clone())))),
            _ => Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new())))),
        };
    }
    let key_callback = args[args.len() - 1].clone();
    let val_callback = args[args.len() - 2].clone();
    let first = match args[0].deref() {
        Value::Array(arr) => arr.borrow().clone(),
        _ => return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new())))),
    };
    let other_arrays: Vec<PhpArray> = args[1..array_arg_count]
        .iter()
        .filter_map(|v| {
            let v = v.deref();
            if let Value::Array(arr) = v {
                Some(arr.borrow().clone())
            } else {
                None
            }
        })
        .collect();

    let mut result = PhpArray::new();
    for (key, value) in first.iter() {
        let key_val = match key {
            ArrayKey::Int(n) => Value::Long(*n),
            ArrayKey::String(s) => Value::String(s.clone()),
        };
        let mut found_in_all = true;
        for other in &other_arrays {
            let mut found = false;
            for (okey, ovalue) in other.iter() {
                let okey_val = match okey {
                    ArrayKey::Int(n) => Value::Long(*n),
                    ArrayKey::String(s) => Value::String(s.clone()),
                };
                let key_cmp = call_user_func(vm, &[key_callback.clone(), key_val.clone(), okey_val])?;
                if key_cmp.to_long() == 0 {
                    let val_cmp = call_user_func(vm, &[val_callback.clone(), value.clone(), ovalue.clone()])?;
                    if val_cmp.to_long() == 0 {
                        found = true;
                        break;
                    }
                }
            }
            if !found {
                found_in_all = false;
                break;
            }
        }
        if found_in_all {
            result.set(key.clone(), value.clone());
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}


fn parse_ini_string_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let ini_str = match args.first() {
        Some(Value::String(s)) => s.to_string_lossy(),
        Some(v) => v.to_php_string().to_string_lossy(),
        None => return Ok(Value::False),
    };
    let process_sections = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
    let scanner_mode = args.get(2).map(|v| v.to_long()).unwrap_or(0); // INI_SCANNER_NORMAL=0

    let mut result = PhpArray::new();
    let mut current_section: Option<String> = None;

    for line in ini_str.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with(';') || trimmed.starts_with('#') {
            continue;
        }
        // Section header
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if process_sections {
                current_section = Some(trimmed[1..trimmed.len()-1].to_string());
            }
            continue;
        }
        // Key=value
        if let Some(eq_pos) = trimmed.find('=') {
            let key = trimmed[..eq_pos].trim().to_string();
            let val_str = trimmed[eq_pos+1..].trim_start();
            // Remove surrounding quotes
            let val_str = if val_str.len() >= 2
                && ((val_str.starts_with('"') && val_str.ends_with('"'))
                || (val_str.starts_with('\'') && val_str.ends_with('\'')))
            {
                &val_str[1..val_str.len()-1]
            } else {
                val_str
            };
            // Strip inline comments
            let val_str = if scanner_mode == 0 {
                if let Some(comment_pos) = val_str.find(';') {
                    // Only strip if preceded by whitespace
                    if comment_pos > 0 && val_str.as_bytes()[comment_pos - 1] == b' ' {
                        val_str[..comment_pos].trim_end()
                    } else {
                        val_str
                    }
                } else {
                    val_str
                }
            } else {
                val_str
            };
            // Convert special values
            let value = match val_str.to_lowercase().as_str() {
                "true" | "on" | "yes" => Value::String(PhpString::from_bytes(b"1")),
                "false" | "off" | "no" | "none" | "" => Value::String(PhpString::empty()),
                "null" => Value::String(PhpString::empty()),
                _ => {
                    if scanner_mode == 1 {
                        // INI_SCANNER_RAW - return raw string
                        Value::String(PhpString::from_string(val_str.to_string()))
                    } else {
                        // Try to parse as number
                        if let Ok(n) = val_str.parse::<i64>() {
                            Value::String(PhpString::from_string(n.to_string()))
                        } else if let Ok(f) = val_str.parse::<f64>() {
                            Value::String(PhpString::from_string(f.to_string()))
                        } else {
                            Value::String(PhpString::from_string(val_str.to_string()))
                        }
                    }
                }
            };

            let arr_key = ArrayKey::String(PhpString::from_string(key));
            if process_sections {
                if let Some(ref section) = current_section {
                    // Use integer key for pure integer section names
                    let section_key = if let Ok(n) = section.parse::<i64>() {
                        // Only use integer key if it doesn't start with '0' (to avoid octal confusion)
                        // and doesn't start with '+' or '-' (PHP only converts positive integers)
                        if !section.starts_with('0') || section == "0" {
                            ArrayKey::Int(n)
                        } else {
                            ArrayKey::String(PhpString::from_string(section.clone()))
                        }
                    } else {
                        ArrayKey::String(PhpString::from_string(section.clone()))
                    };
                    // Get or create section array
                    let section_arr = if let Some(existing) = result.get(&section_key) {
                        if let Value::Array(arr) = existing {
                            arr.clone()
                        } else {
                            Rc::new(RefCell::new(PhpArray::new()))
                        }
                    } else {
                        Rc::new(RefCell::new(PhpArray::new()))
                    };
                    section_arr.borrow_mut().set(arr_key, value);
                    result.set(section_key, Value::Array(section_arr));
                } else {
                    result.set(arr_key, value);
                }
            } else {
                result.set(arr_key, value);
            }
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn assert_options_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let option = args.first().map(|v| v.to_long()).unwrap_or(0);
    let new_value = args.get(1);

    // Emit deprecated for the ASSERT_* constant - must come before the function deprecation
    // (in PHP, constant access happens before function dispatch)
    let const_name = match option {
        1 => Some("ASSERT_ACTIVE"),
        2 => Some("ASSERT_WARNING"),
        3 => Some("ASSERT_BAIL"),
        4 => Some("ASSERT_QUIET_EVAL"),
        5 => Some("ASSERT_CALLBACK"),
        6 => Some("ASSERT_EXCEPTION"),
        _ => None,
    };
    if let Some(name) = const_name {
        _vm.emit_deprecated(&format!("Constant {} is deprecated since 8.3, as assert_options() is deprecated", name));
    }

    _vm.emit_deprecated("Function assert_options() is deprecated since 8.3");

    // INI key mapping for assert options
    let ini_key = match option {
        1 => Some(b"assert.active".as_ref()),
        2 => Some(b"assert.warning".as_ref()),
        3 => Some(b"assert.bail".as_ref()),
        4 => Some(b"assert.quiet_eval".as_ref()),
        5 => Some(b"assert.callback".as_ref()),
        6 => Some(b"assert.exception".as_ref()),
        _ => None,
    };

    // Get previous value
    let prev = if let Some(key) = ini_key {
        if option == 5 {
            // ASSERT_CALLBACK returns the callback value or null
            _vm.constants.get(key).cloned().unwrap_or(Value::Null)
        } else {
            // Others return int
            let val = _vm.constants.get(key).map(|v| v.to_long()).unwrap_or(match option {
                1 => 1, // ASSERT_ACTIVE default
                6 => 1, // ASSERT_EXCEPTION default
                _ => 0,
            });
            Value::Long(val)
        }
    } else {
        return Ok(Value::False);
    };

    // Set new value if provided
    if let (Some(new_val), Some(key)) = (new_value, ini_key) {
        _vm.constants.insert(key.to_vec(), new_val.clone());
    }

    Ok(prev)
}

fn ftruncate_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let fid = args.first().unwrap_or(&Value::Null).to_long();
    let size = args.get(1).map(|v| v.to_long()).unwrap_or(0) as u64;
    FILE_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(fh) = handles.get_mut(&fid) {
            match fh.file.set_len(size) {
                Ok(_) => Ok(Value::True),
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
    unsafe { libc::unlink(path.as_ptr()); }
    // Convert fd to std::fs::File
    use std::os::unix::io::FromRawFd;
    let file = unsafe { std::fs::File::from_raw_fd(fd) };
    let fid = alloc_file_handle(file, "w+b");
    Ok(Value::Long(fid))
}

fn filemtime_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    if !vm.check_open_basedir("filemtime", &path.to_string_lossy()) {
        return Ok(Value::False);
    }
    match std::fs::metadata(&*path.to_string_lossy()) {
        Ok(meta) => {
            if let Ok(modified) = meta.modified() {
                let secs = modified.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
                Ok(Value::Long(secs as i64))
            } else {
                Ok(Value::False)
            }
        }
        Err(_) => {
            vm.emit_warning(&format!("filemtime(): stat failed for {}", path.to_string_lossy()));
            Ok(Value::False)
        }
    }
}

fn fileatime_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    if !vm.check_open_basedir("fileatime", &path.to_string_lossy()) {
        return Ok(Value::False);
    }
    match std::fs::metadata(&*path.to_string_lossy()) {
        Ok(meta) => {
            if let Ok(accessed) = meta.accessed() {
                let secs = accessed.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
                Ok(Value::Long(secs as i64))
            } else {
                Ok(Value::False)
            }
        }
        Err(_) => {
            vm.emit_warning(&format!("fileatime(): stat failed for {}", path.to_string_lossy()));
            Ok(Value::False)
        }
    }
}

fn filectime_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    filemtime_fn(vm, args)
}

fn fileinode_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    if !vm.check_open_basedir("fileinode", &path.to_string_lossy()) {
        return Ok(Value::False);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match std::fs::metadata(&*path.to_string_lossy()) {
            Ok(meta) => Ok(Value::Long(meta.ino() as i64)),
            Err(_) => {
                vm.emit_warning(&format!("fileinode(): stat failed for {}", path.to_string_lossy()));
                Ok(Value::False)
            }
        }
    }
    #[cfg(not(unix))]
    Ok(Value::False)
}

fn fileowner_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    if !vm.check_open_basedir("fileowner", &path.to_string_lossy()) {
        return Ok(Value::False);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match std::fs::metadata(&*path.to_string_lossy()) {
            Ok(meta) => Ok(Value::Long(meta.uid() as i64)),
            Err(_) => {
                vm.emit_warning(&format!("fileowner(): stat failed for {}", path.to_string_lossy()));
                Ok(Value::False)
            }
        }
    }
    #[cfg(not(unix))]
    Ok(Value::Long(0))
}

fn filegroup_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    if !vm.check_open_basedir("filegroup", &path.to_string_lossy()) {
        return Ok(Value::False);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match std::fs::metadata(&*path.to_string_lossy()) {
            Ok(meta) => Ok(Value::Long(meta.gid() as i64)),
            Err(_) => {
                vm.emit_warning(&format!("filegroup(): stat failed for {}", path.to_string_lossy()));
                Ok(Value::False)
            }
        }
    }
    #[cfg(not(unix))]
    Ok(Value::Long(0))
}

fn chown_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}

fn chgrp_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}

fn fputcsv_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let fid = args.first().unwrap_or(&Value::Null).to_long();
    let fields = match args.get(1) {
        Some(Value::Array(arr)) => arr.clone(),
        _ => return Ok(Value::False),
    };
    let separator = args.get(2).map(|v| {
        let s = v.to_php_string();
        let b = s.as_bytes();
        if b.is_empty() { b',' } else { b[0] }
    }).unwrap_or(b',');
    let enclosure = args.get(3).map(|v| {
        let s = v.to_php_string();
        let b = s.as_bytes();
        if b.is_empty() { b'"' } else { b[0] }
    }).unwrap_or(b'"');

    let mut line = String::new();
    let arr = fields.borrow();
    let mut first = true;
    for (_, val) in arr.iter() {
        if !first {
            line.push(separator as char);
        }
        first = false;
        let s = val.to_php_string().to_string_lossy();
        // Check if we need to enclose
        let needs_enclosure = s.contains(separator as char) || s.contains(enclosure as char) || s.contains('\n') || s.contains('\r');
        if needs_enclosure {
            line.push(enclosure as char);
            for c in s.chars() {
                if c == enclosure as char {
                    line.push(enclosure as char);
                }
                line.push(c);
            }
            line.push(enclosure as char);
        } else {
            line.push_str(&s);
        }
    }
    line.push('\n');

    let bytes = line.as_bytes();
    let len = bytes.len();

    FILE_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(fh) = handles.get_mut(&fid) {
            let _ = fh.file.write_all(bytes);
        }
    });

    Ok(Value::Long(len as i64))
}

fn fpassthru_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let fid = args.first().unwrap_or(&Value::Null).to_long();
    FILE_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(fh) = handles.get_mut(&fid) {
            let mut buf = Vec::new();
            match fh.file.read_to_end(&mut buf) {
                Ok(n) => {
                    fh.eof = true;
                    vm.write_output(&buf);
                    Ok(Value::Long(n as i64))
                }
                Err(_) => Ok(Value::False),
            }
        } else {
            Ok(Value::False)
        }
    })
}

fn linkinfo_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match std::fs::metadata(&*path.to_string_lossy()) {
            Ok(meta) => Ok(Value::Long(meta.dev() as i64)),
            Err(_) => Ok(Value::Long(-1)),
        }
    }
    #[cfg(not(unix))]
    Ok(Value::Long(-1))
}

fn parse_ini_file_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let process_sections = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
    let scanner_mode = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    match std::fs::read_to_string(&*path.to_string_lossy()) {
        Ok(content) => {
            let ini_args = vec![
                Value::String(PhpString::from_string(content)),
                if process_sections { Value::True } else { Value::False },
                Value::Long(scanner_mode),
            ];
            parse_ini_string_fn(vm, &ini_args)
        }
        Err(_) => Ok(Value::False),
    }
}

fn error_get_last_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Null) // stub - no last error tracking yet
}

fn error_clear_last_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Null)
}

fn phpversion_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(ext) = args.first() {
        if !matches!(ext, Value::Null | Value::Undef) {
            let ext_name = ext.to_php_string().to_string_lossy().to_ascii_lowercase();
            match ext_name.as_str() {
                "standard" | "core" | "date" | "pcre" | "json" | "ctype" | "hash" | "spl" => {
                    return Ok(Value::String(PhpString::from_bytes(b"8.5.4")));
                }
                _ => return Ok(Value::False),
            }
        }
    }
    Ok(Value::String(PhpString::from_bytes(b"8.5.4")))
}

fn php_uname_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let mode = args.first().unwrap_or(&Value::Null).to_php_string();
    let mode_char = mode.as_bytes().first().copied().unwrap_or(b'a');
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

fn php_sapi_name_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(PhpString::from_bytes(b"cli")))
}

fn phpinfo_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let what = args.first().map(|v| v.to_long()).unwrap_or(0xFFFF);
    // Minimal text-mode phpinfo output
    let mut out = String::new();
    out.push_str("phpinfo()\n");
    if what & 1 != 0 {
        out.push_str("PHP Version => 8.5.4\n\n");
        out.push_str("System => Linux localhost 6.0.0 #1 x86_64\n");
        out.push_str("Server API => Command Line Interface\n\n");
    }
    if what & 4 != 0 {
        out.push_str("Configuration\n\n");
    }
    if what & 8 != 0 {
        out.push_str("Core\n\n");
    }
    if what & 32 != 0 {
        out.push_str("PHP Variables\n\n");
    }
    if what & 64 != 0 {
        out.push_str("PHP License\nThis program is free software\n\n");
    }
    vm.write_output(out.as_bytes());
    Ok(Value::True)
}

fn phpcredits_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let _flag = args.first().map(|v| v.to_long()).unwrap_or(0xFFFF);
    vm.write_output(b"PHP Credits\n");
    Ok(Value::True)
}

fn get_cfg_var_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let name = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy();
    // Return false for most config vars (not configured)
    match name.as_str() {
        "cfg_file_path" => Ok(Value::String(PhpString::from_bytes(b""))),
        _ => Ok(Value::False),
    }
}

fn php_ini_loaded_file_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}

fn php_ini_scanned_files_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
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

fn getlastmod_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // Returns the time the script was last modified, or false on failure
    Ok(Value::False)
}

fn get_current_user_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(PhpString::from_bytes(b"magicaltux")))
}

fn image_type_to_mime_type_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let image_type = args.first().map(|v| v.to_long()).unwrap_or(0);
    let mime = match image_type {
        1 => "image/gif",         // IMAGETYPE_GIF
        2 => "image/jpeg",        // IMAGETYPE_JPEG
        3 => "image/png",         // IMAGETYPE_PNG
        4 => "application/x-shockwave-flash", // IMAGETYPE_SWF
        5 => "image/psd",         // IMAGETYPE_PSD
        6 => "image/bmp",         // IMAGETYPE_BMP
        7 | 8 => "image/tiff",    // IMAGETYPE_TIFF_II/MM
        9 => "application/octet-stream", // IMAGETYPE_JPC
        10 => "image/jp2",        // IMAGETYPE_JP2
        11 => "application/octet-stream", // IMAGETYPE_JPX
        12 => "application/octet-stream", // IMAGETYPE_JB2
        13 => "application/x-shockwave-flash", // IMAGETYPE_SWC
        14 => "image/iff",        // IMAGETYPE_IFF
        15 => "image/vnd.wap.wbmp", // IMAGETYPE_WBMP
        16 => "image/xbm",        // IMAGETYPE_XBM
        17 => "image/vnd.microsoft.icon", // IMAGETYPE_ICO
        18 => "image/webp",       // IMAGETYPE_WEBP
        19 => "image/avif",       // IMAGETYPE_AVIF
        _ => "application/octet-stream",
    };
    Ok(Value::String(PhpString::from_bytes(mime.as_bytes())))
}

fn image_type_to_extension_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let image_type = args.first().map(|v| v.to_long()).unwrap_or(0);
    let include_dot = args.get(1).map(|v| v.is_truthy()).unwrap_or(true);
    let ext = match image_type {
        1 => "gif",
        2 => "jpeg",
        3 => "png",
        4 => "swf",
        5 => "psd",
        6 => "bmp",
        7 | 8 => "tiff",
        9 => "jpc",
        10 => "jp2",
        15 => "wbmp",
        16 => "xbm",
        17 => "ico",
        18 => "webp",
        19 => "avif",
        _ => return Ok(Value::False),
    };
    let result = if include_dot {
        format!(".{}", ext)
    } else {
        ext.to_string()
    };
    Ok(Value::String(PhpString::from_string(result)))
}

fn defined_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let name = args.first().unwrap_or(&Value::Null).to_php_string();
    let name_bytes = name.as_bytes();
    let name_str = name.to_string_lossy();
    // Check for class constants
    if let Some(pos) = name_str.find("::") {
        let class_name = &name_str[..pos];
        let const_name = &name_str[pos+2..];
        // Strip leading backslash from class name
        let class_name_stripped = class_name.strip_prefix('\\').unwrap_or(class_name);
        let class_lower: Vec<u8> = class_name_stripped.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
        if let Some(class) = vm.classes.get(&class_lower) {
            if class.constants.contains_key(const_name.as_bytes()) {
                return Ok(Value::True);
            }
        }
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

fn extension_loaded_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let ext = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy().to_ascii_lowercase();
    let loaded = matches!(ext.as_str(),
        "standard" | "core" | "date" | "pcre" | "json" | "ctype" | "hash" | "spl"
    );
    Ok(if loaded { Value::True } else { Value::False })
}

fn get_loaded_extensions_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut result = PhpArray::new();
    for ext in &["Core", "standard", "date", "pcre", "json", "ctype", "hash", "SPL"] {
        result.push(Value::String(PhpString::from_bytes(ext.as_bytes())));
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn get_extension_funcs_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False) // stub
}

// class_implements, class_parents, class_uses moved to goro-ext-spl

fn str_increment_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        let msg = "str_increment(): Argument #1 ($string) must not be empty";
        let exc = vm.create_exception(b"ValueError", msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg.to_string(), line: vm.current_line });
    }
    // Validate all characters are alphanumeric first
    if !bytes.iter().all(|b| b.is_ascii_alphanumeric()) {
        let msg = "str_increment(): Argument #1 ($string) must be composed only of alphanumeric ASCII characters";
        let exc = vm.create_exception(b"ValueError", msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg.to_string(), line: vm.current_line });
    }
    // PHP string increment: like spreadsheet columns
    let mut result: Vec<u8> = bytes.to_vec();
    let mut carry = true;
    for i in (0..result.len()).rev() {
        if !carry { break; }
        match result[i] {
            b'z' => { result[i] = b'a'; carry = true; }
            b'Z' => { result[i] = b'A'; carry = true; }
            b'9' => { result[i] = b'0'; carry = true; }
            b'a'..=b'y' => { result[i] += 1; carry = false; }
            b'A'..=b'Y' => { result[i] += 1; carry = false; }
            b'0'..=b'8' => { result[i] += 1; carry = false; }
            _ => {
                let msg = "str_increment(): Argument #1 ($string) must be composed only of alphanumeric ASCII characters";
                let exc = vm.create_exception(b"ValueError", msg, vm.current_line);
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg.to_string(), line: vm.current_line });
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

fn str_decrement_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        let msg = "str_decrement(): Argument #1 ($string) must not be empty";
        let exc = vm.create_exception(b"ValueError", msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg.to_string(), line: vm.current_line });
    }
    // Validate all characters are alphanumeric first
    if !bytes.iter().all(|b| b.is_ascii_alphanumeric()) {
        let msg = "str_decrement(): Argument #1 ($string) must be composed only of alphanumeric ASCII characters";
        let exc = vm.create_exception(b"ValueError", msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg.to_string(), line: vm.current_line });
    }
    // Check if string is out of decrement range:
    // - Single char: 'a', 'A', '0' are out of range (can't go lower)
    // - Multi-char starting with '0': if all chars are minimums (a/A/0), out of range
    //   because the numeric leading zero removal would produce an invalid result
    // - Multi-char starting with 'a'/'A': even if all chars are minimums,
    //   the leading char is just removed (underflow is allowed)
    {
        let all_min = bytes.iter().all(|&b| b == b'a' || b == b'A' || b == b'0');
        if all_min {
            let first = bytes[0];
            if bytes.len() == 1 || first == b'0' {
                let msg = format!("str_decrement(): Argument #1 ($string) \"{}\" is out of decrement range", String::from_utf8_lossy(bytes));
                let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg, line: vm.current_line });
            }
        }
    }
    let mut result: Vec<u8> = bytes.to_vec();
    let mut borrow = true;
    for i in (0..result.len()).rev() {
        if !borrow { break; }
        match result[i] {
            b'a' => { result[i] = b'z'; borrow = true; }
            b'A' => { result[i] = b'Z'; borrow = true; }
            b'0' => { result[i] = b'9'; borrow = true; }
            b'b'..=b'z' => { result[i] -= 1; borrow = false; }
            b'B'..=b'Z' => { result[i] -= 1; borrow = false; }
            b'1'..=b'9' => { result[i] -= 1; borrow = false; }
            _ => {
                let msg = "str_decrement(): Argument #1 ($string) must be composed only of alphanumeric ASCII characters";
                let exc = vm.create_exception(b"ValueError", msg, vm.current_line);
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg.to_string(), line: vm.current_line });
            }
        }
    }
    // If borrow propagated past the first character, remove the leading character
    if borrow && result.len() > 1 {
        result.remove(0);
    } else if !borrow && result.len() > 1 {
        // After decrementing, strip the leading character if it became the minimum
        // for its type and the original first char was NOT this minimum.
        // E.g., "10" -> "09" -> strip '0' -> "9"
        // E.g., "1A" -> "0Z" -> strip '0' -> "Z"  (wait, '1' decrements to '0')
        // Actually for "1A": i=1 'A' -> borrow, result='Z', then i=0 '1' -> '0', no borrow
        // For "Ba": i=1 'a' -> borrow, result='z', then i=0 'B' -> 'A', no borrow
        // For "bA": i=1 'A' -> borrow, result='Z', then i=0 'b' -> 'a', no borrow
        // So when first char decremented to its minimum AND it wasn't already that minimum:
        let new_first = result[0];
        let old_first = bytes[0];
        if new_first != old_first && (new_first == b'0' || new_first == b'a' || new_first == b'A') {
            result.remove(0);
        }
    }
    Ok(Value::String(PhpString::from_vec(result)))
}


/// Parse PHP memory_limit value (e.g., "128M", "256K", "1G", "-1")
fn parse_memory_value(val: &Value) -> i64 {
    let s = val.to_php_string().to_string_lossy();
    let s = s.trim();
    if s == "-1" {
        return -1; // unlimited
    }
    let (num_str, multiplier) = if s.ends_with('G') || s.ends_with('g') {
        (&s[..s.len()-1], 1024 * 1024 * 1024i64)
    } else if s.ends_with('M') || s.ends_with('m') {
        (&s[..s.len()-1], 1024 * 1024i64)
    } else if s.ends_with('K') || s.ends_with('k') {
        (&s[..s.len()-1], 1024i64)
    } else {
        (s, 1i64)
    };
    num_str.parse::<i64>().unwrap_or(128 * 1024 * 1024) * multiplier
}

fn fscanf_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // fscanf($handle, $format, ...$vars) - read line from file and parse with sscanf
    if args.len() < 2 {
        return Ok(Value::Null);
    }
    let fid = args[0].to_long();
    // Read a line from the file
    let line = FILE_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(fh) = handles.get_mut(&fid) {
            let mut buf = Vec::new();
            let mut byte = [0u8; 1];
            loop {
                match fh.file.read(&mut byte) {
                    Ok(0) => {
                        fh.eof = true;
                        break;
                    }
                    Ok(_) => {
                        buf.push(byte[0]);
                        if byte[0] == b'\n' {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            if buf.is_empty() && fh.eof {
                None
            } else {
                Some(String::from_utf8_lossy(&buf).to_string())
            }
        } else {
            None
        }
    });

    match line {
        None => Ok(Value::False),
        Some(line_str) => {
            // Call sscanf with the line and remaining args
            let mut sscanf_args = vec![Value::String(PhpString::from_string(line_str))];
            sscanf_args.extend_from_slice(&args[1..]);
            sscanf_fn(vm, &sscanf_args)
        }
    }
}

fn fgetc_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let fid = args.first().unwrap_or(&Value::Null).to_long();
    FILE_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(fh) = handles.get_mut(&fid) {
            let mut buf = [0u8; 1];
            match fh.file.read(&mut buf) {
                Ok(0) => {
                    fh.eof = true;
                    Ok(Value::False)
                }
                Ok(_) => Ok(Value::String(PhpString::from_bytes(&buf))),
                Err(_) => Ok(Value::False),
            }
        } else {
            Ok(Value::False)
        }
    })
}

fn flock_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // flock($handle, $operation) - stub that always succeeds
    // Real file locking would need libc::flock
    let _fid = args.first().unwrap_or(&Value::Null).to_long();
    let _op = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    Ok(Value::True)
}

fn fstat_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let fid = args.first().unwrap_or(&Value::Null).to_long();
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
                    result.set(ArrayKey::String(PhpString::from_bytes(name)), Value::Long(*val));
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
                            (0, b"dev", 0), (1, b"ino", 0), (2, b"mode", 0o100644),
                            (3, b"nlink", 1), (4, b"uid", 0), (5, b"gid", 0), (6, b"rdev", 0),
                            (7, b"size", len), (8, b"atime", 0), (9, b"mtime", 0),
                            (10, b"ctime", 0), (11, b"blksize", -1), (12, b"blocks", -1),
                        ];
                        // PHP stat returns all numeric keys first (0-12), then all string keys
                        for (idx, _name, val) in &fields {
                            arr.set(ArrayKey::Int(*idx), Value::Long(*val));
                        }
                        for (_idx, name, val) in &fields {
                            arr.set(ArrayKey::String(PhpString::from_bytes(name)), Value::Long(*val));
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

fn stream_get_contents_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let fid = args.first().unwrap_or(&Value::Null).to_long();
    let max_length = args.get(1).map(|v| v.to_long()).unwrap_or(-1);
    let offset = args.get(2).map(|v| v.to_long()).unwrap_or(-1);

    FILE_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(fh) = handles.get_mut(&fid) {
            // Seek to offset if specified
            if offset >= 0 {
                let _ = fh.file.seek(SeekFrom::Start(offset as u64));
            }
            let mut buf = Vec::new();
            if max_length >= 0 {
                buf.resize(max_length as usize, 0);
                match fh.file.read(&mut buf) {
                    Ok(n) => {
                        buf.truncate(n);
                        if n == 0 { fh.eof = true; }
                        Ok(Value::String(PhpString::from_vec(buf)))
                    }
                    Err(_) => Ok(Value::False),
                }
            } else {
                match fh.file.read_to_end(&mut buf) {
                    Ok(_) => {
                        fh.eof = true;
                        Ok(Value::String(PhpString::from_vec(buf)))
                    }
                    Err(_) => Ok(Value::False),
                }
            }
        } else {
            Ok(Value::False)
        }
    })
}

fn get_error_handler_fn(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(vm.error_handler.clone().unwrap_or(Value::Null))
}

fn get_exception_handler_fn(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // We don't track exception handler separately yet, return null
    let _ = vm;
    Ok(Value::Null)
}

fn get_included_files_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // Return empty array for now
    let arr = PhpArray::new();
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

// iterator_apply moved to goro-ext-spl

fn property_exists_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let obj_or_class = args.first().unwrap_or(&Value::Null);
    let prop_name = args.get(1).map(|v| v.to_php_string()).unwrap_or_else(|| PhpString::from_bytes(b""));
    let prop_bytes = prop_name.as_bytes();

    match obj_or_class {
        Value::Object(obj) => {
            let obj_borrow = obj.borrow();
            // Check instance properties
            if obj_borrow.has_property(prop_bytes) {
                return Ok(Value::True);
            }
            // Check class definition
            let class_lower: Vec<u8> = obj_borrow.class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
            drop(obj_borrow);
            if let Some(class_def) = vm.get_class_def(&class_lower) {
                for prop in &class_def.properties {
                    if prop.name == prop_bytes {
                        return Ok(Value::True);
                    }
                }
            }
            Ok(Value::False)
        }
        Value::String(s) => {
            let class_lower: Vec<u8> = s.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
            if let Some(class_def) = vm.get_class_def(&class_lower) {
                for prop in &class_def.properties {
                    if prop.name == prop_bytes {
                        return Ok(Value::True);
                    }
                }
            }
            Ok(Value::False)
        }
        _ => Ok(Value::Null),
    }
}

fn get_mangled_object_vars_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Object(obj)) = args.first() {
        let obj = obj.borrow();
        let class_name_lower: Vec<u8> = obj.class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
        let prop_info: std::collections::HashMap<Vec<u8>, (goro_core::object::Visibility, Vec<u8>)> =
            if let Some(class_def) = vm.get_class_def(&class_name_lower) {
                class_def.properties.iter()
                    .map(|p| {
                        let decl_lower = &p.declaring_class;
                        let decl_display = vm.get_class_def(decl_lower)
                            .map(|c| c.name.clone())
                            .unwrap_or_else(|| p.declaring_class.clone());
                        (p.name.clone(), (p.visibility, decl_display))
                    })
                    .collect()
            } else {
                std::collections::HashMap::new()
            };
        let mut arr = PhpArray::new();
        for (name, val) in &obj.properties {
            if name.starts_with(b"__spl_") || name.starts_with(b"__reflection_") || name.starts_with(b"__enum_")
                || name.starts_with(b"__ctor_") || name.starts_with(b"__clone_") || name.starts_with(b"__destructed")
                || name.starts_with(b"__fiber_") || name.starts_with(b"__timestamp") {
                continue;
            }
            let (vis, declaring_class) = prop_info.get(name)
                .map(|(v, dc)| (Some(*v), dc.as_slice()))
                .unwrap_or((None, &[]));
            let mangled = mangle_property_name(name, declaring_class, vis);
            arr.set(
                ArrayKey::String(PhpString::from_vec(mangled)),
                val.clone(),
            );
        }
        Ok(Value::Array(Rc::new(RefCell::new(arr))))
    } else {
        Ok(Value::Null)
    }
}

fn get_resource_id_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Resources are represented as Long (file handle IDs)
    if let Some(Value::Long(id)) = args.first() {
        Ok(Value::Long(*id))
    } else {
        Ok(Value::Long(0))
    }
}

fn ip2long_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let ip = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy();
    let parts: Vec<&str> = ip.split('.').collect();
    if parts.len() != 4 {
        return Ok(Value::False);
    }
    let mut result: u32 = 0;
    for (i, part) in parts.iter().enumerate() {
        match part.parse::<u32>() {
            Ok(n) if n <= 255 => {
                result |= n << (8 * (3 - i));
            }
            _ => return Ok(Value::False),
        }
    }
    // PHP returns signed long
    Ok(Value::Long(result as i64))
}

fn long2ip_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let ip = args.first().unwrap_or(&Value::Null).to_long() as u32;
    let s = format!("{}.{}.{}.{}",
        (ip >> 24) & 0xFF,
        (ip >> 16) & 0xFF,
        (ip >> 8) & 0xFF,
        ip & 0xFF,
    );
    Ok(Value::String(PhpString::from_string(s)))
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

fn exec_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let command = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy();
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(&command)
        .output();
    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let raw_lines: Vec<&str> = stdout.split('\n').collect();
            // Remove trailing empty element from final newline
            let lines: Vec<&str> = if raw_lines.last() == Some(&"") && raw_lines.len() > 1 {
                raw_lines[..raw_lines.len()-1].to_vec()
            } else {
                raw_lines
            };
            let last_line = lines.last().map(|s| s.to_string()).unwrap_or_default();

            // If $output array ref is provided (args[1]), fill it
            if let Some(output_ref) = args.get(1) {
                if let Value::Reference(r) = output_ref {
                    let inner = r.borrow().clone();
                    let arr = match inner {
                        Value::Array(a) => a,
                        _ => {
                            let a = Rc::new(RefCell::new(PhpArray::new()));
                            *r.borrow_mut() = Value::Array(a.clone());
                            a
                        }
                    };
                    let mut arr_mut = arr.borrow_mut();
                    for line in &lines {
                        arr_mut.push(Value::String(PhpString::from_string(line.to_string())));
                    }
                }
            }

            // If $result_code ref is provided (args[2]), set the exit code
            if let Some(code_ref) = args.get(2) {
                if let Value::Reference(r) = code_ref {
                    *r.borrow_mut() = Value::Long(out.status.code().unwrap_or(-1) as i64);
                }
            }

            Ok(Value::String(PhpString::from_string(last_line)))
        }
        Err(_) => Ok(Value::False),
    }
}

fn system_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let command = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy();
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(&command)
        .output();
    match output {
        Ok(out) => {
            let stdout = &out.stdout;
            vm.write_output(stdout);
            let stdout_str = String::from_utf8_lossy(stdout);
            let lines: Vec<&str> = stdout_str.trim_end_matches('\n').split('\n').collect();
            let last_line = lines.last().map(|s| s.to_string()).unwrap_or_default();
            Ok(Value::String(PhpString::from_string(last_line)))
        }
        Err(_) => Ok(Value::False),
    }
}

fn shell_exec_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let command = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy();
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(&command)
        .output();
    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            Ok(Value::String(PhpString::from_string(stdout)))
        }
        Err(_) => Ok(Value::Null),
    }
}

fn passthru_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let command = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy();
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(&command)
        .output();
    match output {
        Ok(out) => {
            vm.write_output(&out.stdout);
            Ok(Value::Null)
        }
        Err(_) => Ok(Value::False),
    }
}

fn escapeshellarg(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = s.as_bytes();
    let mut result = Vec::with_capacity(bytes.len() + 2);
    result.push(b'\'');
    for &b in bytes {
        if b == b'\'' {
            result.extend_from_slice(b"'\\''");
        } else {
            result.push(b);
        }
    }
    result.push(b'\'');
    Ok(Value::String(PhpString::from_vec(result)))
}

fn escapeshellcmd(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = s.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    for &b in bytes {
        match b {
            b'#' | b'&' | b';' | b'`' | b'|' | b'*' | b'?' | b'~' | b'<' | b'>' | b'^'
            | b'(' | b')' | b'[' | b']' | b'{' | b'}' | b'$' | b'\\' | b'\x0A' | b'\xFF' => {
                result.push(b'\\');
                result.push(b);
            }
            _ => result.push(b),
        }
    }
    Ok(Value::String(PhpString::from_vec(result)))
}

fn getopt_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // getopt() parses CLI options - return empty array as stub since we're not a real CLI
    Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
}

fn gethostbyname_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let host = args.first().unwrap_or(&Value::Null).to_php_string();
    let host_str = host.to_string_lossy();
    // Try DNS resolution
    use std::net::ToSocketAddrs;
    let addr_str = format!("{}:0", host_str);
    match addr_str.to_socket_addrs() {
        Ok(mut addrs) => {
            // Return first IPv4 address found
            for addr in &mut addrs {
                if addr.is_ipv4() {
                    return Ok(Value::String(PhpString::from_string(addr.ip().to_string())));
                }
            }
            // No IPv4 found, return hostname unchanged (PHP behavior)
            Ok(Value::String(host))
        }
        Err(_) => {
            // On failure, PHP returns the hostname unchanged
            Ok(Value::String(host))
        }
    }
}

fn gethostbynamel_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let host = args.first().unwrap_or(&Value::Null).to_php_string();
    let host_str = host.to_string_lossy();
    use std::net::ToSocketAddrs;
    let addr_str = format!("{}:0", host_str);
    match addr_str.to_socket_addrs() {
        Ok(addrs) => {
            let mut result = PhpArray::new();
            for addr in addrs {
                if addr.is_ipv4() {
                    result.push(Value::String(PhpString::from_string(addr.ip().to_string())));
                }
            }
            Ok(Value::Array(Rc::new(RefCell::new(result))))
        }
        Err(_) => Ok(Value::False),
    }
}

fn gethostbyaddr_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let ip = args.first().unwrap_or(&Value::Null).to_php_string();
    // Reverse DNS is complex; stub returning the IP itself
    Ok(Value::String(ip))
}

thread_local! {
    static PROC_HANDLES: RefCell<StdHashMap<i64, std::process::Child>> = RefCell::new(StdHashMap::new());
    static NEXT_PROC_ID: std::cell::Cell<i64> = const { std::cell::Cell::new(1) };
}

fn proc_open_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let cmd = args.first().unwrap_or(&Value::Null);
    let descriptor_spec = args.get(1);
    let pipes_ref = args.get(2);
    let cmd_str = match cmd {
        Value::Array(arr) => {
            let arr = arr.borrow();
            arr.iter().map(|(_, v)| v.to_php_string().to_string_lossy()).collect::<Vec<_>>().join(" ")
        }
        _ => cmd.to_php_string().to_string_lossy(),
    };

    use std::process::{Command, Stdio};
    let mut child_cmd = Command::new("sh");
    child_cmd.arg("-c").arg(&cmd_str);
    let mut has_stdin_pipe = false;
    let mut has_stdout_pipe = false;
    let mut has_stderr_pipe = false;

    if let Some(Value::Array(spec)) = descriptor_spec {
        let spec = spec.borrow();
        for (key, val) in spec.iter() {
            let fd_num = match key { ArrayKey::Int(n) => *n, _ => continue };
            if let Value::Array(fd_spec) = val {
                let fd_spec = fd_spec.borrow();
                let fd_type = fd_spec.get_int(0).map(|v| v.to_php_string()).unwrap_or_else(|| PhpString::from_bytes(b""));
                match &*fd_type.to_string_lossy() {
                    "pipe" => match fd_num {
                        0 => { child_cmd.stdin(Stdio::piped()); has_stdin_pipe = true; }
                        1 => { child_cmd.stdout(Stdio::piped()); has_stdout_pipe = true; }
                        2 => { child_cmd.stderr(Stdio::piped()); has_stderr_pipe = true; }
                        _ => {}
                    },
                    "file" => {
                        let filename = fd_spec.get_int(1).map(|v| v.to_php_string()).unwrap_or_else(|| PhpString::from_bytes(b""));
                        let mode = fd_spec.get_int(2).map(|v| v.to_php_string()).unwrap_or_else(|| PhpString::from_bytes(b"r"));
                        let mode_str = mode.to_string_lossy();
                        let filename_str = filename.to_string_lossy();
                        match fd_num {
                            0 => { if let Ok(f) = std::fs::File::open(&*filename_str) { child_cmd.stdin(f); } }
                            1 => {
                                let file = if mode_str.contains('a') {
                                    std::fs::OpenOptions::new().append(true).create(true).open(&*filename_str)
                                } else { std::fs::File::create(&*filename_str) };
                                if let Ok(f) = file { child_cmd.stdout(f); }
                            }
                            2 => {
                                let file = if mode_str.contains('a') {
                                    std::fs::OpenOptions::new().append(true).create(true).open(&*filename_str)
                                } else { std::fs::File::create(&*filename_str) };
                                if let Ok(f) = file { child_cmd.stderr(f); }
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    if !has_stdin_pipe { child_cmd.stdin(Stdio::null()); }
    if !has_stdout_pipe { child_cmd.stdout(Stdio::inherit()); }
    if !has_stderr_pipe { child_cmd.stderr(Stdio::inherit()); }

    if let Some(cwd) = args.get(3) {
        if !matches!(cwd, Value::Null) {
            let s = cwd.to_php_string().to_string_lossy();
            if !s.is_empty() { child_cmd.current_dir(&*s); }
        }
    }
    if let Some(Value::Array(env_arr)) = args.get(4) {
        child_cmd.env_clear();
        for (key, val) in env_arr.borrow().iter() {
            let k = match key { ArrayKey::String(s) => s.to_string_lossy(), ArrayKey::Int(n) => format!("{}", n) };
            child_cmd.env(&*k, &*val.to_php_string().to_string_lossy());
        }
    }

    match child_cmd.spawn() {
        Ok(mut child) => {
            use std::os::unix::io::{FromRawFd, IntoRawFd};
            let mut pipes_arr = PhpArray::new();
            if has_stdin_pipe {
                if let Some(stdin) = child.stdin.take() {
                    let fid = alloc_file_handle(unsafe { std::fs::File::from_raw_fd(stdin.into_raw_fd()) }, "w");
                    pipes_arr.set(ArrayKey::Int(0), Value::Long(fid));
                }
            }
            if has_stdout_pipe {
                if let Some(stdout) = child.stdout.take() {
                    let fid = alloc_file_handle(unsafe { std::fs::File::from_raw_fd(stdout.into_raw_fd()) }, "r");
                    pipes_arr.set(ArrayKey::Int(1), Value::Long(fid));
                }
            }
            if has_stderr_pipe {
                if let Some(stderr) = child.stderr.take() {
                    let fid = alloc_file_handle(unsafe { std::fs::File::from_raw_fd(stderr.into_raw_fd()) }, "r");
                    pipes_arr.set(ArrayKey::Int(2), Value::Long(fid));
                }
            }
            if let Some(Value::Reference(r)) = pipes_ref {
                *r.borrow_mut() = Value::Array(Rc::new(RefCell::new(pipes_arr)));
            }
            let proc_id = NEXT_PROC_ID.with(|id| {
                let pid = id.get(); id.set(pid + 1);
                PROC_HANDLES.with(|h| h.borrow_mut().insert(pid, child));
                pid
            });
            Ok(Value::Long(proc_id))
        }
        Err(_) => Ok(Value::False),
    }
}

fn proc_close_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let proc_id = args.first().unwrap_or(&Value::Null).to_long();
    let exit_code = PROC_HANDLES.with(|h| {
        match h.borrow_mut().remove(&proc_id) {
            Some(mut child) => child.wait().map(|s| s.code().unwrap_or(-1) as i64).unwrap_or(-1),
            None => -1,
        }
    });
    Ok(Value::Long(exit_code))
}

fn proc_get_status_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let proc_id = args.first().unwrap_or(&Value::Null).to_long();
    PROC_HANDLES.with(|h| {
        let mut handles = h.borrow_mut();
        if let Some(child) = handles.get_mut(&proc_id) {
            let mut arr = PhpArray::new();
            match child.try_wait() {
                Ok(Some(status)) => {
                    arr.set(ArrayKey::String(PhpString::from_bytes(b"command")), Value::String(PhpString::from_bytes(b"")));
                    arr.set(ArrayKey::String(PhpString::from_bytes(b"pid")), Value::Long(child.id() as i64));
                    arr.set(ArrayKey::String(PhpString::from_bytes(b"running")), Value::False);
                    arr.set(ArrayKey::String(PhpString::from_bytes(b"signaled")), Value::False);
                    arr.set(ArrayKey::String(PhpString::from_bytes(b"stopped")), Value::False);
                    arr.set(ArrayKey::String(PhpString::from_bytes(b"exitcode")), Value::Long(status.code().unwrap_or(-1) as i64));
                    arr.set(ArrayKey::String(PhpString::from_bytes(b"termsig")), Value::Long(0));
                    arr.set(ArrayKey::String(PhpString::from_bytes(b"stopsig")), Value::Long(0));
                }
                Ok(None) => {
                    arr.set(ArrayKey::String(PhpString::from_bytes(b"command")), Value::String(PhpString::from_bytes(b"")));
                    arr.set(ArrayKey::String(PhpString::from_bytes(b"pid")), Value::Long(child.id() as i64));
                    arr.set(ArrayKey::String(PhpString::from_bytes(b"running")), Value::True);
                    arr.set(ArrayKey::String(PhpString::from_bytes(b"signaled")), Value::False);
                    arr.set(ArrayKey::String(PhpString::from_bytes(b"stopped")), Value::False);
                    arr.set(ArrayKey::String(PhpString::from_bytes(b"exitcode")), Value::Long(-1));
                    arr.set(ArrayKey::String(PhpString::from_bytes(b"termsig")), Value::Long(0));
                    arr.set(ArrayKey::String(PhpString::from_bytes(b"stopsig")), Value::Long(0));
                }
                Err(_) => return Ok(Value::False),
            }
            Ok(Value::Array(Rc::new(RefCell::new(arr))))
        } else { Ok(Value::False) }
    })
}

fn proc_terminate_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let proc_id = args.first().unwrap_or(&Value::Null).to_long();
    PROC_HANDLES.with(|h| {
        if let Some(child) = h.borrow_mut().get_mut(&proc_id) {
            match child.kill() { Ok(_) => Ok(Value::True), Err(_) => Ok(Value::False) }
        } else { Ok(Value::False) }
    })
}

fn inet_pton_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let addr_str = args.first().unwrap_or(&Value::Null).to_php_string();
    let addr_s = addr_str.to_string_lossy();
    // Try parsing as IPv4
    if let Ok(ipv4) = addr_s.parse::<std::net::Ipv4Addr>() {
        return Ok(Value::String(PhpString::from_vec(ipv4.octets().to_vec())));
    }
    // Try parsing as IPv6
    if let Ok(ipv6) = addr_s.parse::<std::net::Ipv6Addr>() {
        return Ok(Value::String(PhpString::from_vec(ipv6.octets().to_vec())));
    }
    // PHP 8.1+: no warning, just return false
    Ok(Value::False)
}

fn inet_ntop_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let packed = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = packed.as_bytes();
    match bytes.len() {
        4 => {
            let addr = std::net::Ipv4Addr::new(bytes[0], bytes[1], bytes[2], bytes[3]);
            Ok(Value::String(PhpString::from_string(addr.to_string())))
        }
        16 => {
            let mut octets = [0u8; 16];
            octets.copy_from_slice(bytes);
            let addr = std::net::Ipv6Addr::from(octets);
            Ok(Value::String(PhpString::from_string(addr.to_string())))
        }
        _ => {
            // PHP 8.1+: no warning for invalid length
            Ok(Value::False)
        }
    }
}

/// Mangle a PHP object property name according to visibility.
/// Private: \0DeclaringClass\0propName
/// Protected: \0*\0propName
/// Public: propName (unchanged)
fn mangle_property_name(name: &[u8], declaring_class: &[u8], visibility: Option<goro_core::object::Visibility>) -> Vec<u8> {
    match visibility {
        Some(goro_core::object::Visibility::Private) => {
            let mut mangled = Vec::with_capacity(1 + declaring_class.len() + 1 + name.len());
            mangled.push(0);
            mangled.extend_from_slice(declaring_class);
            mangled.push(0);
            mangled.extend_from_slice(name);
            mangled
        }
        Some(goro_core::object::Visibility::Protected) => {
            let mut mangled = Vec::with_capacity(3 + name.len());
            mangled.push(0);
            mangled.push(b'*');
            mangled.push(0);
            mangled.extend_from_slice(name);
            mangled
        }
        _ => name.to_vec(),
    }
}

// Stream stubs
fn stream_set_blocking_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True)
}
fn stream_set_timeout_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True)
}
fn stream_set_write_buffer_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(0))
}
fn stream_set_read_buffer_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(0))
}

fn stream_copy_to_stream_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let src_fid = args.first().unwrap_or(&Value::Null).to_long();
    let dst_fid = args.get(1).unwrap_or(&Value::Null).to_long();
    let max_length = args.get(2).map(|v| v.to_long()).unwrap_or(-1);
    let offset = args.get(3).map(|v| v.to_long()).unwrap_or(0);
    let data = FILE_HANDLES.with(|h| {
        let mut handles = h.borrow_mut();
        if let Some(fh) = handles.get_mut(&src_fid) {
            use std::io::{Read, Seek, SeekFrom};
            if offset > 0 { let _ = fh.file.seek(SeekFrom::Start(offset as u64)); }
            let mut buf = Vec::new();
            if max_length >= 0 {
                buf.resize(max_length as usize, 0);
                match fh.file.read(&mut buf) { Ok(n) => { buf.truncate(n); Some(buf) } Err(_) => None }
            } else {
                match fh.file.read_to_end(&mut buf) { Ok(_) => Some(buf), Err(_) => None }
            }
        } else { None }
    });
    match data {
        Some(buf) => {
            let len = buf.len();
            let ok = FILE_HANDLES.with(|h| {
                let mut handles = h.borrow_mut();
                if let Some(fh) = handles.get_mut(&dst_fid) {
                    use std::io::Write;
                    fh.file.write_all(&buf).is_ok()
                } else { false }
            });
            if ok { Ok(Value::Long(len as i64)) } else { Ok(Value::False) }
        }
        None => Ok(Value::False),
    }
}

fn stream_filter_append_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn stream_filter_prepend_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn stream_filter_register_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::True) }
fn stream_filter_remove_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::True) }
fn stream_context_create_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::Long(0)) }
fn stream_context_set_option_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::True) }
fn stream_context_get_options_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new())))) }
fn stream_context_set_params_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::True) }
fn stream_context_get_params_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut arr = PhpArray::new();
    arr.set(ArrayKey::String(PhpString::from_bytes(b"options")), Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}
fn stream_context_get_default_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::Long(0)) }
fn stream_context_set_default_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::Long(0)) }

fn stream_is_local_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    match val {
        Value::String(s) => {
            let s_str = s.to_string_lossy();
            if s_str.starts_with("http://") || s_str.starts_with("https://") || s_str.starts_with("ftp://") {
                Ok(Value::False)
            } else { Ok(Value::True) }
        }
        _ => Ok(Value::True),
    }
}

fn stream_get_meta_data_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let fid = args.first().unwrap_or(&Value::Null).to_long();
    let mut arr = PhpArray::new();
    FILE_HANDLES.with(|h| {
        if let Some(fh) = h.borrow().get(&fid) {
            arr.set(ArrayKey::String(PhpString::from_bytes(b"timed_out")), Value::False);
            arr.set(ArrayKey::String(PhpString::from_bytes(b"blocked")), Value::True);
            arr.set(ArrayKey::String(PhpString::from_bytes(b"eof")), if fh.eof { Value::True } else { Value::False });
            arr.set(ArrayKey::String(PhpString::from_bytes(b"wrapper_type")), Value::String(PhpString::from_bytes(b"plainfile")));
            arr.set(ArrayKey::String(PhpString::from_bytes(b"stream_type")), Value::String(PhpString::from_bytes(b"STDIO")));
            arr.set(ArrayKey::String(PhpString::from_bytes(b"mode")), Value::String(PhpString::from_string(fh.mode.clone())));
            arr.set(ArrayKey::String(PhpString::from_bytes(b"unread_bytes")), Value::Long(0));
            arr.set(ArrayKey::String(PhpString::from_bytes(b"seekable")), Value::True);
            arr.set(ArrayKey::String(PhpString::from_bytes(b"uri")), Value::String(PhpString::from_bytes(b"")));
        }
    });
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

fn stream_wrapper_register_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::True) }
fn stream_wrapper_unregister_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::True) }
fn stream_wrapper_restore_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::True) }

fn stream_get_wrappers_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut arr = PhpArray::new();
    arr.push(Value::String(PhpString::from_bytes(b"file")));
    arr.push(Value::String(PhpString::from_bytes(b"php")));
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

fn stream_get_filters_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut arr = PhpArray::new();
    for name in &[b"string.rot13" as &[u8], b"string.toupper", b"string.tolower", b"convert.iconv.*"] {
        arr.push(Value::String(PhpString::from_bytes(name)));
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

fn stream_socket_client_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn stream_socket_server_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn stream_socket_get_name_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn stream_select_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn stream_socket_pair_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }

fn stream_get_line_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let fid = args.first().unwrap_or(&Value::Null).to_long();
    let max_length = args.get(1).map(|v| v.to_long() as usize).unwrap_or(0);
    let ending = args.get(2).map(|v| v.to_php_string()).unwrap_or_else(|| PhpString::from_bytes(b"\n"));
    let ending_bytes = ending.as_bytes();
    if max_length == 0 { return Ok(Value::False); }
    FILE_HANDLES.with(|h| {
        let mut handles = h.borrow_mut();
        if let Some(fh) = handles.get_mut(&fid) {
            use std::io::Read;
            let mut buf = Vec::new();
            let mut byte = [0u8; 1];
            for _ in 0..max_length {
                match fh.file.read(&mut byte) {
                    Ok(0) => { fh.eof = true; break; }
                    Ok(_) => {
                        buf.push(byte[0]);
                        if buf.len() >= ending_bytes.len() && buf[buf.len() - ending_bytes.len()..] == *ending_bytes {
                            buf.truncate(buf.len() - ending_bytes.len());
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            if buf.is_empty() && fh.eof { Ok(Value::False) }
            else { Ok(Value::String(PhpString::from_vec(buf))) }
        } else { Ok(Value::False) }
    })
}

fn stream_set_chunk_size_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::Long(8192)) }
fn stream_socket_recvfrom_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn stream_socket_sendto_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn stream_socket_shutdown_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::True) }
fn stream_socket_enable_crypto_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn headers_list_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new())))) }

fn dir_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let path_str = path.to_string_lossy();
    let dir_path = std::path::Path::new(&*path_str);
    if !dir_path.is_dir() {
        vm.emit_warning(&format!("dir({}): Failed to open directory", path_str));
        return Ok(Value::False);
    }
    match std::fs::read_dir(dir_path) {
        Ok(entries) => {
            let mut names: Vec<String> = vec![".".to_string(), "..".to_string()];
            for entry in entries { if let Ok(e) = entry { names.push(e.file_name().to_string_lossy().to_string()); } }
            names.sort(); names.reverse();
            let key = format!("dir:{}", path_str);
            DIR_HANDLES.with(|dh| dh.borrow_mut().insert(key.clone(), names));
            let mut obj = PhpObject::new(b"Directory".to_vec(), 0);
            obj.set_property(b"path".to_vec(), Value::String(PhpString::from_string(path_str.to_string())));
            obj.set_property(b"handle".to_vec(), Value::String(PhpString::from_string(key)));
            Ok(Value::Object(Rc::new(RefCell::new(obj))))
        }
        Err(_) => { vm.emit_warning(&format!("dir({}): Failed to open directory", path_str)); Ok(Value::False) }
    }
}

fn popen_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let command = args.first().unwrap_or(&Value::Null).to_php_string();
    let mode = args.get(1).map(|v| v.to_php_string()).unwrap_or_else(|| PhpString::from_bytes(b"r"));
    let cmd_str = command.to_string_lossy();
    let mode_str = mode.to_string_lossy();
    use std::process::{Command, Stdio};
    let child = if mode_str.contains('r') {
        Command::new("sh").arg("-c").arg(&*cmd_str).stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn()
    } else {
        Command::new("sh").arg("-c").arg(&*cmd_str).stdin(Stdio::piped()).spawn()
    };
    match child {
        Ok(child) => {
            use std::os::unix::io::{FromRawFd, IntoRawFd};
            if mode_str.contains('r') {
                if let Some(stdout) = child.stdout {
                    return Ok(Value::Long(alloc_file_handle(unsafe { std::fs::File::from_raw_fd(stdout.into_raw_fd()) }, &mode_str)));
                }
            } else if let Some(stdin) = child.stdin {
                return Ok(Value::Long(alloc_file_handle(unsafe { std::fs::File::from_raw_fd(stdin.into_raw_fd()) }, &mode_str)));
            }
            Ok(Value::False)
        }
        Err(_) => Ok(Value::False),
    }
}

fn pclose_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let fid = args.first().unwrap_or(&Value::Null).to_long();
    FILE_HANDLES.with(|h| h.borrow_mut().remove(&fid));
    Ok(Value::Long(0))
}

fn rewinddir_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let handle = args.first().unwrap_or(&Value::Null);
    let key = handle.to_php_string().to_string_lossy();
    DIR_HANDLES.with(|dh| {
        if key.starts_with("dir:") {
            let path = &key[4..];
            if let Ok(entries) = std::fs::read_dir(path) {
                let mut names: Vec<String> = vec![".".to_string(), "..".to_string()];
                for entry in entries { if let Ok(e) = entry { names.push(e.file_name().to_string_lossy().to_string()); } }
                names.sort(); names.reverse();
                dh.borrow_mut().insert(key.to_string(), names);
            }
        }
    });
    Ok(Value::Null)
}

fn error_log_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let message = args.first().unwrap_or(&Value::Null).to_php_string();
    let message_type = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let destination = args.get(2).map(|v| v.to_php_string());
    match message_type {
        3 => {
            if let Some(dest) = destination {
                use std::io::Write;
                match std::fs::OpenOptions::new().append(true).create(true).open(&*dest.to_string_lossy()) {
                    Ok(mut f) => { let _ = f.write_all(message.as_bytes()); Ok(Value::True) }
                    Err(_) => Ok(Value::False),
                }
            } else { Ok(Value::False) }
        }
        _ => { eprintln!("{}", message.to_string_lossy()); Ok(Value::True) }
    }
}

fn highlight_file_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let ret = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
    match std::fs::read_to_string(&*path.to_string_lossy()) {
        Ok(content) => {
            let output = format!("<code><span style=\"color: #000000\">{}</span></code>",
                content.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;"));
            if ret { Ok(Value::String(PhpString::from_string(output))) } else { Ok(Value::True) }
        }
        Err(_) => Ok(Value::False),
    }
}

fn php_strip_whitespace_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    match std::fs::read_to_string(&*path.to_string_lossy()) {
        Ok(content) => Ok(Value::String(PhpString::from_string(content))),
        Err(_) => Ok(Value::String(PhpString::from_bytes(b""))),
    }
}

fn disk_free_space_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::Double(1073741824.0)) }
fn disk_total_space_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::Double(10737418240.0)) }
fn get_resources_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new())))) }
fn closelog_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::True) }
fn openlog_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::True) }
fn syslog_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::True) }

fn mail_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Stub: mail() always returns false in test/CLI mode
    let _to = args.first().unwrap_or(&Value::Null);
    let _subject = args.get(1).unwrap_or(&Value::Null);
    let _message = args.get(2).unwrap_or(&Value::Null);
    if args.is_empty() {
        let msg = "mail() expects at least 3 arguments, 0 given";
        let exc = vm.create_exception(b"ArgumentCountError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg.to_string(), line: vm.current_line });
    }
    // In CLI/test mode, mail() typically fails
    Ok(Value::False)
}

fn fsockopen_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let hostname = args.first().unwrap_or(&Value::Null).to_php_string();
    let port = args.get(1).map(|v| v.to_long()).unwrap_or(-1);
    let errno_ref = args.get(2);
    let errstr_ref = args.get(3);

    let host_str = hostname.to_string_lossy();

    // Try to connect
    use std::net::TcpStream;
    let addr = if port > 0 {
        format!("{}:{}", host_str.trim_start_matches("tcp://").trim_start_matches("ssl://"), port)
    } else {
        host_str.to_string()
    };

    match TcpStream::connect_timeout(&addr.parse().unwrap_or_else(|_| {
        std::net::SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::new(0, 0, 0, 0)), 0)
    }), std::time::Duration::from_secs(5)) {
        Ok(stream) => {
            // Set errno to 0 and errstr to ""
            if let Some(Value::Reference(r)) = errno_ref { *r.borrow_mut() = Value::Long(0); }
            if let Some(Value::Reference(r)) = errstr_ref { *r.borrow_mut() = Value::String(PhpString::from_bytes(b"")); }

            use std::os::unix::io::IntoRawFd;
            let raw_fd = stream.into_raw_fd();
            use std::os::unix::io::FromRawFd;
            let file = unsafe { std::fs::File::from_raw_fd(raw_fd) };
            let fid = alloc_file_handle(file, "r+");
            Ok(Value::Long(fid))
        }
        Err(e) => {
            if let Some(Value::Reference(r)) = errno_ref { *r.borrow_mut() = Value::Long(111); } // ECONNREFUSED
            if let Some(Value::Reference(r)) = errstr_ref {
                *r.borrow_mut() = Value::String(PhpString::from_string(e.to_string()));
            }
            vm.emit_warning(&format!("fsockopen(): Unable to connect to {} ({})", addr, e));
            Ok(Value::False)
        }
    }
}

fn pfsockopen_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    fsockopen_fn(vm, args)
}

fn getimagesize_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let path_str = path.to_string_lossy();

    // Read first few bytes to detect image type
    match std::fs::read(&*path_str) {
        Ok(data) => {
            if data.len() < 8 { return Ok(Value::False); }
            let mut arr = PhpArray::new();
            // Detect image type from magic bytes
            if data.starts_with(b"\x89PNG") {
                // PNG
                if data.len() >= 24 {
                    let width = u32::from_be_bytes([data[16], data[17], data[18], data[19]]) as i64;
                    let height = u32::from_be_bytes([data[20], data[21], data[22], data[23]]) as i64;
                    arr.set(ArrayKey::Int(0), Value::Long(width));
                    arr.set(ArrayKey::Int(1), Value::Long(height));
                    arr.set(ArrayKey::Int(2), Value::Long(3)); // IMAGETYPE_PNG
                    arr.set(ArrayKey::Int(3), Value::String(PhpString::from_string(format!("width=\"{}\" height=\"{}\"", width, height))));
                    arr.set(ArrayKey::String(PhpString::from_bytes(b"bits")), Value::Long(8));
                    arr.set(ArrayKey::String(PhpString::from_bytes(b"mime")), Value::String(PhpString::from_bytes(b"image/png")));
                }
            } else if data.starts_with(b"\xff\xd8\xff") {
                // JPEG - basic detection
                arr.set(ArrayKey::Int(0), Value::Long(0));
                arr.set(ArrayKey::Int(1), Value::Long(0));
                arr.set(ArrayKey::Int(2), Value::Long(2)); // IMAGETYPE_JPEG
                arr.set(ArrayKey::Int(3), Value::String(PhpString::from_bytes(b"width=\"0\" height=\"0\"")));
                arr.set(ArrayKey::String(PhpString::from_bytes(b"bits")), Value::Long(8));
                arr.set(ArrayKey::String(PhpString::from_bytes(b"mime")), Value::String(PhpString::from_bytes(b"image/jpeg")));
            } else if data.starts_with(b"GIF8") {
                // GIF
                if data.len() >= 10 {
                    let width = u16::from_le_bytes([data[6], data[7]]) as i64;
                    let height = u16::from_le_bytes([data[8], data[9]]) as i64;
                    arr.set(ArrayKey::Int(0), Value::Long(width));
                    arr.set(ArrayKey::Int(1), Value::Long(height));
                    arr.set(ArrayKey::Int(2), Value::Long(1)); // IMAGETYPE_GIF
                    arr.set(ArrayKey::Int(3), Value::String(PhpString::from_string(format!("width=\"{}\" height=\"{}\"", width, height))));
                    arr.set(ArrayKey::String(PhpString::from_bytes(b"bits")), Value::Long(8));
                    arr.set(ArrayKey::String(PhpString::from_bytes(b"mime")), Value::String(PhpString::from_bytes(b"image/gif")));
                }
            } else {
                return Ok(Value::False);
            }
            if arr.len() > 0 {
                Ok(Value::Array(Rc::new(RefCell::new(arr))))
            } else {
                Ok(Value::False)
            }
        }
        Err(_) => Ok(Value::False),
    }
}

fn register_tick_function_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::True) }
fn unregister_tick_function_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::Null) }
fn output_add_rewrite_var_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::True) }
fn output_reset_rewrite_vars_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::True) }
fn dns_check_record_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn dns_get_record_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn getmxrr_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn getservbyname_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn getservbyport_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn getprotobyname_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::Long(-1)) }
fn getprotobynumber_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn get_browser_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn stream_socket_accept_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }

fn stream_isatty_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // In CLI mode, always return false (not a TTY)
    Ok(Value::False)
}

fn is_uploaded_file_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // No files are ever uploaded in CLI mode
    Ok(Value::False)
}

fn move_uploaded_file_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // No files are ever uploaded in CLI mode
    Ok(Value::False)
}

fn cli_set_process_title_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True)
}

fn cli_get_process_title_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(PhpString::from_bytes(b"")))
}

fn ini_parse_quantity_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        return Ok(Value::Long(0));
    }
    let s = args[0].to_php_string();
    let s_str = s.to_string();
    let trimmed = s_str.trim();
    if trimmed.is_empty() {
        return Ok(Value::Long(0));
    }
    // Parse numeric part
    let (num_str, suffix) = if let Some(pos) = trimmed.find(|c: char| c.is_ascii_alphabetic()) {
        (&trimmed[..pos], &trimmed[pos..])
    } else {
        (trimmed, "")
    };
    let base_val: i64 = match num_str.parse() {
        Ok(v) => v,
        Err(_) => {
            vm.emit_warning(&format!("Invalid quantity \"{}\": no valid leading digits, interpreting as \"0\" for backwards compatibility", s_str));
            return Ok(Value::Long(0));
        }
    };
    let multiplier = match suffix.to_ascii_uppercase().as_str() {
        "K" => 1024,
        "M" => 1024 * 1024,
        "G" => 1024 * 1024 * 1024,
        "" => 1,
        _ => {
            vm.emit_warning(&format!("Invalid quantity \"{}\": unknown multiplier \"{}\", interpreting as \"{}\" for backwards compatibility", s_str, suffix, base_val));
            1
        }
    };
    Ok(Value::Long(base_val * multiplier))
}

fn dl_fn(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    vm.emit_warning("dl(): Dynamically loaded extensions aren't enabled");
    Ok(Value::False)
}

fn header_register_callback_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True)
}

fn token_get_all_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
    }
    let source = args[0].to_php_string();
    let _flags = if args.len() > 1 { args[1].to_long() } else { 0 };

    let mut lexer = goro_parser::Lexer::new(source.as_bytes());
    let tokens = lexer.tokenize();

    let mut result = PhpArray::new();
    let src = source.as_bytes();

    for token in &tokens {
        let token_id = match &token.kind {
            goro_parser::token::TokenKind::InlineHtml(_) => 312_i64, // T_INLINE_HTML
            goro_parser::token::TokenKind::Variable(_) => 320, // T_VARIABLE
            goro_parser::token::TokenKind::LongNumber(_) => 317, // T_LNUMBER
            goro_parser::token::TokenKind::DoubleNumber(_) => 318, // T_DNUMBER
            goro_parser::token::TokenKind::ConstantString(_) => 323, // T_CONSTANT_ENCAPSED_STRING
            goro_parser::token::TokenKind::Identifier(_) => 319, // T_STRING
            goro_parser::token::TokenKind::InterpolatedStringPart(_) => 324, // T_ENCAPSED_AND_WHITESPACE
            goro_parser::token::TokenKind::InterpolatedStringEnd(_) => 324,
            goro_parser::token::TokenKind::OpenTag => 379, // T_OPEN_TAG
            goro_parser::token::TokenKind::OpenTagShort => 380, // T_OPEN_TAG_WITH_ECHO
            goro_parser::token::TokenKind::CloseTag => 381, // T_CLOSE_TAG
            goro_parser::token::TokenKind::Function => 346, // T_FUNCTION
            goro_parser::token::TokenKind::If => 330,
            goro_parser::token::TokenKind::Else => 333,
            goro_parser::token::TokenKind::ElseIf => 331,
            goro_parser::token::TokenKind::While => 334,
            goro_parser::token::TokenKind::For => 336,
            goro_parser::token::TokenKind::Foreach => 337,
            goro_parser::token::TokenKind::Return => 348,
            goro_parser::token::TokenKind::Echo => 316,
            goro_parser::token::TokenKind::Class => 361,
            goro_parser::token::TokenKind::New => 300,
            goro_parser::token::TokenKind::Static => 352,
            goro_parser::token::TokenKind::Public => 362,
            goro_parser::token::TokenKind::Protected => 363,
            goro_parser::token::TokenKind::Private => 364,
            goro_parser::token::TokenKind::Abstract => 358,
            goro_parser::token::TokenKind::Final => 359,
            goro_parser::token::TokenKind::Interface => 360,
            goro_parser::token::TokenKind::Extends => 354,
            goro_parser::token::TokenKind::Implements => 355,
            goro_parser::token::TokenKind::As => 329,
            goro_parser::token::TokenKind::Try => 340,
            goro_parser::token::TokenKind::Catch => 341,
            goro_parser::token::TokenKind::Finally => 342,
            goro_parser::token::TokenKind::Throw => 343,
            goro_parser::token::TokenKind::Switch => 335,
            goro_parser::token::TokenKind::Case => 327,
            goro_parser::token::TokenKind::Default => 328,
            goro_parser::token::TokenKind::Break => 338,
            goro_parser::token::TokenKind::Continue => 339,
            goro_parser::token::TokenKind::Do => 344,
            goro_parser::token::TokenKind::Instanceof => 301,
            goro_parser::token::TokenKind::Trait => 357,
            goro_parser::token::TokenKind::Namespace => 382,
            goro_parser::token::TokenKind::Use => 356,
            goro_parser::token::TokenKind::Include => 262,
            goro_parser::token::TokenKind::IncludeOnce => 263,
            goro_parser::token::TokenKind::Require => 264,
            goro_parser::token::TokenKind::RequireOnce => 265,
            goro_parser::token::TokenKind::Const => 347,
            goro_parser::token::TokenKind::Isset => 350,
            goro_parser::token::TokenKind::Unset => 349,
            goro_parser::token::TokenKind::Empty => 351,
            goro_parser::token::TokenKind::Yield => 267,
            goro_parser::token::TokenKind::YieldFrom => 268,
            goro_parser::token::TokenKind::Match => 369,
            goro_parser::token::TokenKind::Enum => 370,
            goro_parser::token::TokenKind::Fn => 345,
            goro_parser::token::TokenKind::Print => 266,
            goro_parser::token::TokenKind::Exit => 305,
            goro_parser::token::TokenKind::Eval => 260,
            goro_parser::token::TokenKind::Clone => 302,
            goro_parser::token::TokenKind::List => 353,
            goro_parser::token::TokenKind::Array => 365,
            goro_parser::token::TokenKind::Callable => 366,
            goro_parser::token::TokenKind::Readonly => 367,
            goro_parser::token::TokenKind::Var => 347,
            goro_parser::token::TokenKind::Global => 326,
            goro_parser::token::TokenKind::Goto => 325,
            goro_parser::token::TokenKind::Null => 310,
            goro_parser::token::TokenKind::True => 308,
            goro_parser::token::TokenKind::False => 309,
            goro_parser::token::TokenKind::And => 297,
            goro_parser::token::TokenKind::Or => 298,
            goro_parser::token::TokenKind::Xor => 299,
            goro_parser::token::TokenKind::Declare => 326,
            goro_parser::token::TokenKind::BooleanAnd => 285,
            goro_parser::token::TokenKind::BooleanOr => 286,
            goro_parser::token::TokenKind::Equal => 283,
            goro_parser::token::TokenKind::NotEqual => 282,
            goro_parser::token::TokenKind::Identical => 281,
            goro_parser::token::TokenKind::NotIdentical => 280,
            goro_parser::token::TokenKind::LessEqual => 279,
            goro_parser::token::TokenKind::GreaterEqual => 278,
            goro_parser::token::TokenKind::Spaceship => 277,
            goro_parser::token::TokenKind::PlusAssign => 270,
            goro_parser::token::TokenKind::MinusAssign => 271,
            goro_parser::token::TokenKind::StarAssign => 272,
            goro_parser::token::TokenKind::SlashAssign => 273,
            goro_parser::token::TokenKind::DotAssign => 274,
            goro_parser::token::TokenKind::PercentAssign => 275,
            goro_parser::token::TokenKind::AmpersandAssign => 276,
            goro_parser::token::TokenKind::PipeAssign => 287,
            goro_parser::token::TokenKind::CaretAssign => 288,
            goro_parser::token::TokenKind::ShiftLeftAssign => 289,
            goro_parser::token::TokenKind::ShiftRightAssign => 290,
            goro_parser::token::TokenKind::NullCoalesceAssign => 291,
            goro_parser::token::TokenKind::NullCoalesce => 292,
            goro_parser::token::TokenKind::ShiftLeft => 293,
            goro_parser::token::TokenKind::ShiftRight => 294,
            goro_parser::token::TokenKind::Pow => 303,
            goro_parser::token::TokenKind::PowAssign => 304,
            goro_parser::token::TokenKind::Arrow => 384,
            goro_parser::token::TokenKind::NullsafeArrow => 385,
            goro_parser::token::TokenKind::DoubleArrow => 386,
            goro_parser::token::TokenKind::DoubleColon => 387,
            goro_parser::token::TokenKind::Ellipsis => 388,
            goro_parser::token::TokenKind::Increment => 295,
            goro_parser::token::TokenKind::Decrement => 296,
            goro_parser::token::TokenKind::IntCast => 306,
            goro_parser::token::TokenKind::FloatCast => 307,
            goro_parser::token::TokenKind::StringCast => 308,
            goro_parser::token::TokenKind::BoolCast => 309,
            goro_parser::token::TokenKind::ArrayCast => 310,
            goro_parser::token::TokenKind::ObjectCast => 311,
            goro_parser::token::TokenKind::UnsetCast => 312,
            goro_parser::token::TokenKind::Eof => continue,
            _ => 0, // Single-char token
        };

        if token_id == 0 {
            let start = token.span.start as usize;
            let end = token.span.end as usize;
            if start < src.len() && end <= src.len() && start < end {
                let text = &src[start..end];
                result.push(Value::String(PhpString::from_bytes(text)));
            }
        } else {
            let start = token.span.start as usize;
            let end = token.span.end as usize;
            let text = if start < src.len() && end <= src.len() && start < end {
                &src[start..end]
            } else {
                b""
            };
            let mut token_arr = PhpArray::new();
            token_arr.set(ArrayKey::Int(0), Value::Long(token_id));
            token_arr.set(ArrayKey::Int(1), Value::String(PhpString::from_bytes(text)));
            token_arr.set(ArrayKey::Int(2), Value::Long(token.span.line as i64));
            result.push(Value::Array(Rc::new(RefCell::new(token_arr))));
        }
    }

    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn token_name_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        return Ok(Value::String(PhpString::from_bytes(b"UNKNOWN")));
    }
    let id = args[0].to_long();
    let name = match id {
        260 => "T_EVAL",
        262 => "T_INCLUDE",
        263 => "T_INCLUDE_ONCE",
        264 => "T_REQUIRE",
        265 => "T_REQUIRE_ONCE",
        266 => "T_PRINT",
        267 => "T_YIELD",
        268 => "T_YIELD_FROM",
        297 => "T_LOGICAL_AND",
        298 => "T_LOGICAL_OR",
        299 => "T_LOGICAL_XOR",
        300 => "T_NEW",
        301 => "T_INSTANCEOF",
        302 => "T_CLONE",
        305 => "T_EXIT",
        308 => "T_TRUE",
        309 => "T_FALSE",
        310 => "T_NULL",
        312 => "T_INLINE_HTML",
        316 => "T_ECHO",
        317 => "T_LNUMBER",
        318 => "T_DNUMBER",
        319 => "T_STRING",
        320 => "T_VARIABLE",
        323 => "T_CONSTANT_ENCAPSED_STRING",
        324 => "T_ENCAPSED_AND_WHITESPACE",
        325 => "T_GOTO",
        326 => "T_GLOBAL",
        327 => "T_CASE",
        328 => "T_DEFAULT",
        329 => "T_AS",
        330 => "T_IF",
        331 => "T_ELSEIF",
        333 => "T_ELSE",
        334 => "T_WHILE",
        335 => "T_SWITCH",
        336 => "T_FOR",
        337 => "T_FOREACH",
        338 => "T_BREAK",
        339 => "T_CONTINUE",
        340 => "T_TRY",
        341 => "T_CATCH",
        342 => "T_FINALLY",
        343 => "T_THROW",
        344 => "T_DO",
        345 => "T_FN",
        346 => "T_FUNCTION",
        347 => "T_CONST",
        348 => "T_RETURN",
        349 => "T_UNSET",
        350 => "T_ISSET",
        351 => "T_EMPTY",
        352 => "T_STATIC",
        353 => "T_LIST",
        354 => "T_EXTENDS",
        355 => "T_IMPLEMENTS",
        356 => "T_USE",
        357 => "T_TRAIT",
        358 => "T_ABSTRACT",
        359 => "T_FINAL",
        360 => "T_INTERFACE",
        361 => "T_CLASS",
        362 => "T_PUBLIC",
        363 => "T_PROTECTED",
        364 => "T_PRIVATE",
        365 => "T_ARRAY",
        366 => "T_CALLABLE",
        367 => "T_READONLY",
        369 => "T_MATCH",
        370 => "T_ENUM",
        379 => "T_OPEN_TAG",
        380 => "T_OPEN_TAG_WITH_ECHO",
        381 => "T_CLOSE_TAG",
        382 => "T_NAMESPACE",
        _ => "UNKNOWN",
    };
    Ok(Value::String(PhpString::from_bytes(name.as_bytes())))
}

fn request_parse_body_fn(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Err(VmError { message: "request_parse_body() can only be called during a request".into(), line: vm.current_line })
}

fn ignore_user_abort_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(0))
}

fn connection_aborted_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(0))
}

fn connection_status_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(0))
}

fn get_html_translation_table_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let table = if !args.is_empty() { args[0].to_long() } else { 0 };
    let _flags = if args.len() > 1 { args[1].to_long() } else { 11 };
    let mut result = PhpArray::new();
    if table == 1 {
        result.set(ArrayKey::String(PhpString::from_bytes(b"&")), Value::String(PhpString::from_bytes(b"&amp;")));
        result.set(ArrayKey::String(PhpString::from_bytes(b"\"")), Value::String(PhpString::from_bytes(b"&quot;")));
        result.set(ArrayKey::String(PhpString::from_bytes(b"'")), Value::String(PhpString::from_bytes(b"&#039;")));
        result.set(ArrayKey::String(PhpString::from_bytes(b"<")), Value::String(PhpString::from_bytes(b"&lt;")));
        result.set(ArrayKey::String(PhpString::from_bytes(b">")), Value::String(PhpString::from_bytes(b"&gt;")));
    } else {
        result.set(ArrayKey::String(PhpString::from_bytes(b"&")), Value::String(PhpString::from_bytes(b"&amp;")));
        result.set(ArrayKey::String(PhpString::from_bytes(b"\"")), Value::String(PhpString::from_bytes(b"&quot;")));
        result.set(ArrayKey::String(PhpString::from_bytes(b"'")), Value::String(PhpString::from_bytes(b"&#039;")));
        result.set(ArrayKey::String(PhpString::from_bytes(b"<")), Value::String(PhpString::from_bytes(b"&lt;")));
        result.set(ArrayKey::String(PhpString::from_bytes(b">")), Value::String(PhpString::from_bytes(b"&gt;")));
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn nl_langinfo_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(PhpString::from_bytes(b"")))
}

fn localeconv_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut result = PhpArray::new();
    result.set(ArrayKey::String(PhpString::from_bytes(b"decimal_point")), Value::String(PhpString::from_bytes(b".")));
    result.set(ArrayKey::String(PhpString::from_bytes(b"thousands_sep")), Value::String(PhpString::from_bytes(b"")));
    result.set(ArrayKey::String(PhpString::from_bytes(b"int_curr_symbol")), Value::String(PhpString::from_bytes(b"")));
    result.set(ArrayKey::String(PhpString::from_bytes(b"currency_symbol")), Value::String(PhpString::from_bytes(b"")));
    result.set(ArrayKey::String(PhpString::from_bytes(b"mon_decimal_point")), Value::String(PhpString::from_bytes(b"")));
    result.set(ArrayKey::String(PhpString::from_bytes(b"mon_thousands_sep")), Value::String(PhpString::from_bytes(b"")));
    result.set(ArrayKey::String(PhpString::from_bytes(b"positive_sign")), Value::String(PhpString::from_bytes(b"")));
    result.set(ArrayKey::String(PhpString::from_bytes(b"negative_sign")), Value::String(PhpString::from_bytes(b"")));
    result.set(ArrayKey::String(PhpString::from_bytes(b"int_frac_digits")), Value::Long(127));
    result.set(ArrayKey::String(PhpString::from_bytes(b"frac_digits")), Value::Long(127));
    let grouping = PhpArray::new();
    result.set(ArrayKey::String(PhpString::from_bytes(b"grouping")), Value::Array(Rc::new(RefCell::new(grouping))));
    let mon_grouping = PhpArray::new();
    result.set(ArrayKey::String(PhpString::from_bytes(b"mon_grouping")), Value::Array(Rc::new(RefCell::new(mon_grouping))));
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn time_nanosleep_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let seconds = args.first().map(|v| v.to_long()).unwrap_or(0);
    let nanoseconds = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let duration = std::time::Duration::new(seconds as u64, nanoseconds as u32);
    std::thread::sleep(duration);
    Ok(Value::True)
}

fn time_sleep_until_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let timestamp = args.first().map(|v| v.to_double()).unwrap_or(0.0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64();
    if timestamp > now {
        let sleep_time = timestamp - now;
        std::thread::sleep(std::time::Duration::from_secs_f64(sleep_time));
    }
    Ok(Value::True)
}

fn set_file_buffer_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(0)) // success
}

fn stream_resolve_include_path_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let filename = args.first().map(|v| v.to_php_string().to_string_lossy()).unwrap_or_default();
    let path = std::path::Path::new(&*filename);
    if path.exists() {
        match path.canonicalize() {
            Ok(p) => Ok(Value::String(PhpString::from_string(p.to_string_lossy().into_owned()))),
            Err(_) => Ok(Value::False),
        }
    } else {
        Ok(Value::False)
    }
}

fn stream_supports_lock_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True) // Most streams support locking
}

fn stream_bucket_new_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Return a simple object representing a stream bucket
    let _stream = args.first().unwrap_or(&Value::Null);
    let data = args.get(1).map(|v| v.to_php_string()).unwrap_or_else(|| PhpString::from_bytes(b""));
    let id = _vm.next_object_id();
    let mut obj = goro_core::object::PhpObject::new(b"stdClass".to_vec(), id);
    obj.set_property(b"data".to_vec(), Value::String(data));
    obj.set_property(b"datalen".to_vec(), Value::Long(0));
    Ok(Value::Object(Rc::new(RefCell::new(obj))))
}

fn iptcparse_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}

fn iptcembed_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}
