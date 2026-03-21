use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::PhpObject;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use std::cell::RefCell;
use std::rc::Rc;

pub fn register(vm: &mut Vm) {
    // Error handling
    vm.register_function(b"error_reporting", error_reporting);
    vm.register_function(b"set_error_handler", set_error_handler);
    vm.register_function(b"restore_error_handler", restore_error_handler);
    vm.register_function(b"error_get_last", error_get_last_fn);
    vm.register_function(b"error_clear_last", error_clear_last_fn);
    vm.register_function(b"set_exception_handler", set_exception_handler);
    vm.register_function(b"restore_exception_handler", restore_exception_handler);
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
    vm.register_function(b"sleep", sleep_fn);
    vm.register_function(b"usleep", usleep_fn);
    vm.register_function(b"uniqid", uniqid_fn);
    vm.register_function(b"sys_get_temp_dir", sys_get_temp_dir_fn);
    vm.register_function(b"tempnam", tempnam_fn);
    vm.register_function(b"getenv", getenv_fn);
    vm.register_function(b"putenv", putenv_fn);
    vm.register_function(b"spl_autoload_register", spl_autoload_register_fn);
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
    vm.register_function(b"clearstatcache", clearstatcache_fn);
    vm.register_function(b"fputcsv", fputcsv_fn);
    vm.register_function(b"fgetcsv", fgetcsv_fn);
    vm.register_function(b"fpassthru", fpassthru_fn);
    vm.register_function(b"linkinfo", linkinfo_fn);
    vm.register_function(b"parse_ini_file", parse_ini_file_fn);
    vm.register_function(b"header", header_fn);
    vm.register_function(b"headers_sent", headers_sent_fn);
    vm.register_function(b"http_response_code", http_response_code_fn);
    vm.register_function(b"spl_object_hash", spl_object_hash_fn);
    vm.register_function(b"spl_object_id", spl_object_id_fn);
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
    vm.register_function(b"iterator_to_array", iterator_to_array_fn);
    vm.register_function(b"iterator_count", iterator_count_fn);
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
    vm.register_function(b"is_link", is_link_fn);
    vm.register_function(b"stat", stat_fn);
    vm.register_function(b"is_numeric", is_numeric_fn);
    vm.register_function(b"clearstatcache", clearstatcache_fn);
    vm.register_function(b"array_walk_recursive", array_walk_recursive_fn);
    vm.register_function(b"fgetcsv", fgetcsv_fn);
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
    vm.register_function(b"class_implements", class_implements_fn);
    vm.register_function(b"class_parents", class_parents_fn);
    vm.register_function(b"class_uses", class_uses_fn);
    vm.register_function(b"str_increment", str_increment_fn);
    vm.register_function(b"str_decrement", str_decrement_fn);

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

fn set_exception_handler(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Null)
}

fn restore_exception_handler(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True)
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
            // Try user error handler first
            if vm.call_user_error_handler(256, &message, 0) {
                return Ok(Value::True);
            }
            return Err(VmError {
                message: message.to_string(),
                line: 0,
            });
        }
        512 => {
            // E_USER_WARNING
            if !vm.call_user_error_handler(512, &message, 0) {
                vm.emit_warning_raw(&message);
            }
        }
        1024 => {
            // E_USER_NOTICE
            if !vm.call_user_error_handler(1024, &message, 0) {
                vm.emit_notice_raw(&message, 0);
            }
        }
        16384 => {
            // E_USER_DEPRECATED
            if !vm.call_user_error_handler(16384, &message, 0) {
                vm.emit_deprecated_raw(&message, 0);
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
    let name = args.first().unwrap_or(&Value::Null).to_php_string();
    let value = args.get(1).cloned().unwrap_or(Value::Null);
    vm.constants.insert(name.as_bytes().to_vec(), value);
    Ok(Value::True)
}

fn constant(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let name = args.first().unwrap_or(&Value::Null).to_php_string();
    Ok(vm
        .constants
        .get(name.as_bytes())
        .cloned()
        .unwrap_or(Value::Null))
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

// === Function handling ===

fn func_num_args(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // Stub
    Ok(Value::Long(0))
}
fn func_get_args(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
}
fn func_get_arg(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}
fn function_exists(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let name = args.first().unwrap_or(&Value::Null).to_php_string();
    let name_lower: Vec<u8> = name
        .as_bytes()
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
    let val = args.first().unwrap_or(&Value::Null);
    match val {
        Value::String(s) => {
            let name_lower: Vec<u8> = s
                .as_bytes()
                .iter()
                .map(|b| b.to_ascii_lowercase())
                .collect();
            // Check for "Class::method" syntax
            if let Some(pos) = name_lower.iter().position(|&b| b == b':') {
                if pos + 1 < name_lower.len() && name_lower[pos + 1] == b':' {
                    let class_name = &name_lower[..pos];
                    let method_name = &name_lower[pos + 2..];
                    if let Some(class) = vm.classes.get(class_name) {
                        Ok(if class.get_method(method_name).is_some() {
                            Value::True
                        } else {
                            Value::False
                        })
                    } else {
                        Ok(Value::False)
                    }
                } else {
                    Ok(Value::False)
                }
            } else if vm.functions.contains_key(&name_lower)
                || vm.user_functions.contains_key(&name_lower)
            {
                Ok(Value::True)
            } else {
                Ok(Value::False)
            }
        }
        Value::Object(obj) => {
            // Check if the object has __invoke method
            let class_name_lower: Vec<u8> = obj.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
            if class_name_lower == b"closure" {
                Ok(Value::True)
            } else if let Some(class) = vm.classes.get(&class_name_lower) {
                Ok(if class.methods.contains_key(&b"__invoke".to_vec()) {
                    Value::True
                } else {
                    Value::False
                })
            } else {
                Ok(Value::True) // default to true for built-in objects
            }
        }
        Value::Array(arr) => {
            let arr = arr.borrow();
            Ok(if arr.len() == 2 {
                // Validate that the callback is actually callable
                let vals: Vec<Value> = arr.values().cloned().collect();
                let method_name = vals[1].to_php_string();
                let method_lower: Vec<u8> = method_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
                match &vals[0] {
                    Value::Object(obj) => {
                        let class_lower: Vec<u8> = obj.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                        let mut current = class_lower;
                        let mut found = false;
                        for _ in 0..50 {
                            if let Some(class) = vm.classes.get(&current) {
                                if class.methods.contains_key(&method_lower) {
                                    found = true;
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
                        if found { Value::True } else { Value::False }
                    }
                    Value::String(class_name) => {
                        let class_lower: Vec<u8> = class_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
                        if let Some(class) = vm.classes.get(&class_lower) {
                            if class.methods.contains_key(&method_lower) { Value::True } else { Value::False }
                        } else {
                            Value::False
                        }
                    }
                    _ => Value::False,
                }
            } else {
                Value::False
            })
        }
        _ => Ok(Value::False),
    }
}
fn call_user_func(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        return Ok(Value::Null);
    }

    let callback = &args[0];
    let call_args: Vec<Value> = args[1..].to_vec();

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
                    // Static method call: ['ClassName', 'method']
                    let class_lower: Vec<u8> = class_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
                    if let Some(class) = vm.classes.get(&class_lower).cloned() {
                        if let Some(method) = class.methods.get(&method_lower) {
                            let op = method.op_array.clone();
                            let mut fn_cvs = vec![Value::Undef; op.cv_names.len()];
                            for (i, arg) in call_args.iter().enumerate() {
                                if i < fn_cvs.len() {
                                    fn_cvs[i] = arg.clone();
                                }
                            }
                            return vm.execute_fn(&op, fn_cvs);
                        }
                    }
                    return Ok(Value::Null);
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
                            let op = method.op_array.clone();
                            let mut fn_cvs = vec![Value::Undef; op.cv_names.len()];
                            // Set $this
                            if let Some(this_idx) = op.cv_names.iter().position(|n| n == b"this") {
                                fn_cvs[this_idx] = Value::Object(obj.clone());
                            }
                            for (i, arg) in call_args.iter().enumerate() {
                                if i < fn_cvs.len() && op.cv_names.get(i).map(|n| n.as_slice()) != Some(b"this") {
                                    fn_cvs[i] = arg.clone();
                                }
                            }
                            return vm.execute_fn(&op, fn_cvs);
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
                let mut fn_cvs = vec![Value::Undef; user_fn.cv_names.len()];
                let mut idx = 0;
                for cv in &captured {
                    if idx < fn_cvs.len() {
                        fn_cvs[idx] = cv.clone();
                        idx += 1;
                    }
                }
                for arg in &call_args {
                    if idx < fn_cvs.len() {
                        fn_cvs[idx] = arg.clone();
                        idx += 1;
                    }
                }
                return vm.execute_fn(&user_fn, fn_cvs);
            }
        }
        return Ok(Value::Null);
    }

    // Get function name from string callback
    let func_name = match callback {
        Value::String(s) => s.as_bytes().to_vec(),
        _ => return Ok(Value::Null),
    };

    let func_lower: Vec<u8> = func_name.iter().map(|b| b.to_ascii_lowercase()).collect();

    // Try builtin first
    if let Some(builtin) = vm.functions.get(&func_lower).copied() {
        return builtin(vm, &call_args);
    }

    // Try user function
    if let Some(user_fn) = vm.user_functions.get(&func_lower).cloned() {
        let mut fn_cvs = vec![Value::Undef; user_fn.cv_names.len()];
        for (i, arg) in call_args.iter().enumerate() {
            if i < fn_cvs.len() {
                fn_cvs[i] = arg.clone();
            }
        }
        return vm.execute_fn(&user_fn, fn_cvs);
    }

    Ok(Value::Null)
}
fn call_user_func_array(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.len() < 2 {
        return Ok(Value::Null);
    }
    let callback = args[0].clone();
    let params = match &args[1] {
        Value::Array(arr) => {
            let arr = arr.borrow();
            arr.values().cloned().collect::<Vec<_>>()
        }
        _ => vec![],
    };
    let mut call_args = vec![callback];
    call_args.extend(params);
    call_user_func(vm, &call_args)
}

// === Array functions ===

fn array_push(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let mut arr = arr.borrow_mut();
        for val in &args[1..] {
            arr.push(val.clone());
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
        let arr = arr.borrow();
        let mut result = PhpArray::new();
        for (key, _) in arr.iter() {
            let key_val = match key {
                goro_core::array::ArrayKey::Int(n) => Value::Long(*n),
                goro_core::array::ArrayKey::String(s) => Value::String(s.clone()),
            };
            result.push(key_val);
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

fn array_merge(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let mut result = PhpArray::new();
    for arg in args {
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
        let arr = arr.borrow();
        let entries: Vec<_> = arr.iter().map(|(_, v)| v.clone()).collect();
        let mut result = PhpArray::new();
        for val in entries.into_iter().rev() {
            result.push(val);
        }
        Ok(Value::Array(Rc::new(RefCell::new(result))))
    } else {
        Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
    }
}

fn array_flip(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
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

fn array_unique(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let arr = arr.borrow();
        let mut result = PhpArray::new();
        let mut seen: Vec<Vec<u8>> = Vec::new();
        for (key, val) in arr.iter() {
            let s = val.to_php_string().as_bytes().to_vec();
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
        let entries: Vec<_> = arr.iter().map(|(_, v)| v.clone()).collect();
        let offset = args.get(1).map(|v| v.to_long()).unwrap_or(0);
        let length = args.get(2).map(|v| v.to_long());

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
        for val in &entries[start..end] {
            result.push(val.clone());
        }
        Ok(Value::Array(Rc::new(RefCell::new(result))))
    } else {
        Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
    }
}

fn array_splice(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
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
    if let Some(Value::Array(arr)) = args.get(1) {
        let arr = arr.borrow();
        for (key, val) in arr.iter() {
            if val.equals(needle) {
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

fn array_map(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let callback = args.first().cloned().unwrap_or(Value::Null);
    let array = args.get(1);

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

        // Get callback info
        let (func_name, captured_args) = match &callback {
            Value::String(s) => (s.as_bytes().to_vec(), vec![]),
            Value::Array(cb_arr) => {
                let cb = cb_arr.borrow();
                let vals: Vec<Value> = cb.values().cloned().collect();
                if vals.is_empty() {
                    return Ok(Value::Array(Rc::new(RefCell::new(result))));
                }
                (
                    vals[0].to_php_string().as_bytes().to_vec(),
                    vals[1..].to_vec(),
                )
            }
            Value::Null => {
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
            _ => return Ok(Value::Array(Rc::new(RefCell::new(result)))),
        };
        let func_lower: Vec<u8> = func_name.iter().map(|b| b.to_ascii_lowercase()).collect();

        for i in 0..max_len {
            let mut cb_args: Vec<Value> = captured_args.clone();
            for arr in &arrays {
                cb_args.push(arr.get(i).cloned().unwrap_or(Value::Null));
            }
            if let Some(builtin) = vm.functions.get(&func_lower).copied() {
                let mapped = builtin(vm, &cb_args)?;
                result.push(mapped);
            } else if let Some(user_fn) = vm.user_functions.get(&func_lower).cloned() {
                let mut fn_cvs = vec![Value::Undef; user_fn.cv_names.len()];
                for (j, arg) in cb_args.iter().enumerate() {
                    if j < fn_cvs.len() {
                        fn_cvs[j] = arg.clone();
                    }
                }
                let mapped = vm.execute_fn(&user_fn, fn_cvs)?;
                result.push(mapped);
            }
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
                    if let Value::String(class_name) = &vals[0] {
                        let class_lower: Vec<u8> = class_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
                        if let Some(class) = vm.classes.get(&class_lower).cloned() {
                            if let Some(method) = class.methods.get(&method_lower) {
                                let op = method.op_array.clone();
                                for (key, val) in arr.iter() {
                                    let mut fn_cvs = vec![Value::Undef; op.cv_names.len()];
                                    if !fn_cvs.is_empty() { fn_cvs[0] = val.clone(); }
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
    if let (Some(Value::Array(arr)), Some(callback)) = (args.first(), args.get(1)) {
        let entries: Vec<_> = {
            let arr_borrow = arr.borrow();
            arr_borrow
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        };
        let extra_data = args.get(2);

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
            for (key, val) in &entries {
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
                let _ = vm.execute_fn(&user_fn, fn_cvs);
                // Write modified value back to the array
                let new_val = val_ref.borrow().clone();
                arr.borrow_mut().set(key.clone(), new_val);
            }
        } else if let Some(builtin) = vm.functions.get(&func_lower).copied() {
            for (_key, val) in &entries {
                let _ = builtin(vm, &[val.clone()]);
            }
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
        return Err(VmError { message: msg, line: 0 });
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
            return Err(VmError { message: msg, line: 0 });
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
        return Err(VmError { message: msg, line: 0 });
    }
    if num > 10_000_000 {
        let msg = "array_fill(): Argument #2 ($count) is too large".to_string();
        let exc = vm.throw_type_error(msg.clone());
        if let Value::Object(obj) = &exc {
            obj.borrow_mut().class_name = b"ValueError".to_vec();
        }
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: 0 });
    }
    let val = args.get(2).cloned().unwrap_or(Value::Null);
    let mut result = PhpArray::new();
    for i in 0..num {
        result.set(goro_core::array::ArrayKey::Int(start + i), val.clone());
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn array_fill_keys(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let keys = args.first().unwrap_or(&Value::Null);
    let fill_val = args.get(1).cloned().unwrap_or(Value::Null);
    let mut result = PhpArray::new();
    if let Value::Array(arr) = keys {
        let arr = arr.borrow();
        for (_key, val) in arr.iter() {
            let k = value_to_array_key(val);
            result.set(k, fill_val.clone());
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn array_merge_recursive(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let mut result = PhpArray::new();
    for arg in args {
        if let Value::Array(arr) = arg {
            let arr = arr.borrow();
            for (key, val) in arr.iter() {
                match key {
                    goro_core::array::ArrayKey::Int(_) => {
                        result.push(val.clone());
                    }
                    goro_core::array::ArrayKey::String(s) => {
                        if let Some(existing) = result.get_str(s.as_bytes()) {
                            // Merge recursively: if both are arrays, recurse; else create array
                            let merged = match (existing, val) {
                                (Value::Array(a), Value::Array(b)) => {
                                    let mut merged_arr = a.borrow().clone();
                                    for (k, v) in b.borrow().iter() {
                                        match k {
                                            goro_core::array::ArrayKey::Int(_) => {
                                                merged_arr.push(v.clone());
                                            }
                                            _ => {
                                                merged_arr.set(k.clone(), v.clone());
                                            }
                                        }
                                    }
                                    Value::Array(Rc::new(RefCell::new(merged_arr)))
                                }
                                (existing_val, new_val) => {
                                    let mut arr = PhpArray::new();
                                    arr.push(existing_val.clone());
                                    arr.push(new_val.clone());
                                    Value::Array(Rc::new(RefCell::new(arr)))
                                }
                            };
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
            return Err(VmError { message: msg, line: 0 });
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

fn array_intersect(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.len() < 2 {
        return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
    }
    if let (Some(Value::Array(a)), Some(Value::Array(b))) = (args.first(), args.get(1)) {
        let a = a.borrow();
        let b = b.borrow();
        let b_vals: Vec<_> = b
            .values()
            .map(|v| v.to_php_string().as_bytes().to_vec())
            .collect();
        let mut result = PhpArray::new();
        for (key, val) in a.iter() {
            let s = val.to_php_string().as_bytes().to_vec();
            if b_vals.contains(&s) {
                result.set(key.clone(), val.clone());
            }
        }
        Ok(Value::Array(Rc::new(RefCell::new(result))))
    } else {
        Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
    }
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

fn sort_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
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
fn rsort_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
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
fn asort_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
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
fn arsort_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
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
fn ksort_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
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
fn krsort_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
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
fn shuffle_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // TODO: implement actual shuffling (needs random number generator)
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
        if arr.pointer > 0 {
            arr.pointer -= 1;
        }
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
        err_obj.set_property(b"line".to_vec(), Value::Long(0));
        err_obj.set_property(b"previous".to_vec(), Value::Null);
        let exc = Value::Object(Rc::new(RefCell::new(err_obj)));
        vm.current_exception = Some(exc);
        Err(VmError {
            message: format!("assert(): {} failed", msg),
            line: 0,
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
    );
    Ok(if is_builtin || vm.classes.contains_key(&name_lower) {
        Value::True
    } else {
        Value::False
    })
}
fn get_class(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
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
        _ => Ok(Value::False),
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
            obj.class_name
                .iter()
                .map(|c| c.to_ascii_lowercase())
                .collect()
        }
        Value::String(s) => s
            .as_bytes()
            .iter()
            .map(|c| c.to_ascii_lowercase())
            .collect(),
        _ => return Ok(Value::False),
    };

    // Walk parent chain to check for method
    let mut current = class_lower.clone();
    for _ in 0..50 {
        if let Some(class) = vm.classes.get(&current) {
            if class.methods.contains_key(&method_lower) {
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
    Ok(match args.first() {
        Some(Value::Object(_)) | Some(Value::Generator(_)) => Value::True,
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
        Value::False | Value::Null => goro_core::array::ArrayKey::Int(0),
        Value::Double(f) => goro_core::array::ArrayKey::Int(*f as i64),
        _ => goro_core::array::ArrayKey::String(val.to_php_string()),
    }
}
fn array_count_values(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
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
fn array_rand(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(0))
}

// === String extras ===

fn str_split(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let len = args.get(1).map(|v| v.to_long()).unwrap_or(1).max(1) as usize;
    let bytes = s.as_bytes();
    let mut result = PhpArray::new();
    for chunk in bytes.chunks(len) {
        result.push(Value::String(PhpString::from_vec(chunk.to_vec())));
    }
    if result.is_empty() {
        result.push(Value::String(PhpString::empty()));
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn number_format(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    let decimals_raw = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let decimals = if decimals_raw < 0 { 0usize } else { decimals_raw.min(100000) as usize };
    let dec_point = match args.get(2).map(|v| v.deref()) {
        Some(Value::Null) | Some(Value::Undef) | None => ".".to_string(),
        Some(v) => v.to_php_string().to_string_lossy(),
    };
    let thousands_sep = match args.get(3).map(|v| v.deref()) {
        Some(Value::Null) | Some(Value::Undef) | None => ",".to_string(),
        Some(v) => v.to_php_string().to_string_lossy(),
    };

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
            let s = if decimals > 0 { format!("NAN{}{}", dec_point, "0".repeat(decimals)) } else { "NAN".to_string() };
            return Ok(Value::String(PhpString::from_string(s)));
        }
        if num.is_infinite() {
            let prefix = if num < 0.0 { "-" } else { "" };
            let s = if decimals > 0 { format!("{}INF{}{}", prefix, dec_point, "0".repeat(decimals)) } else { format!("{}INF", prefix) };
            return Ok(Value::String(PhpString::from_string(s)));
        }
        let neg = num < 0.0;
        let abs_num = num.abs();
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
        // Check valid padding length
        match content_len % 4 {
            0 => if pad_len != 0 { return Ok(Value::False); },
            2 => if pad_len != 2 { return Ok(Value::False); },
            3 => if pad_len != 1 { return Ok(Value::False); },
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
fn urldecode(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(PhpString::empty()))
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
fn rawurldecode(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(PhpString::empty()))
}
fn htmlspecialchars(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let flags = args.get(1).map(|v| v.to_long()).unwrap_or(11); // ENT_QUOTES | ENT_SUBSTITUTE (default)
    let _double_encode = args.get(3).map(|v| v.is_truthy()).unwrap_or(true);
    let ent_compat = flags & 2 != 0;  // ENT_COMPAT
    let ent_quotes = flags & 3 == 3;  // ENT_QUOTES (both single and double)
    let mut result = Vec::new();
    for &b in s.as_bytes() {
        match b {
            b'&' => result.extend_from_slice(b"&amp;"),
            b'"' if ent_compat || ent_quotes => result.extend_from_slice(b"&quot;"),
            b'\'' if ent_quotes => result.extend_from_slice(b"&#039;"),
            b'<' => result.extend_from_slice(b"&lt;"),
            b'>' => result.extend_from_slice(b"&gt;"),
            _ => result.push(b),
        }
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
fn wordwrap(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(PhpString::empty()))
}
fn printf(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Use the sprintf implementation from strings module
    let formatted = crate::strings::do_sprintf(args);
    let len = formatted.len();
    vm.write_output(formatted.as_bytes());
    Ok(Value::Long(len as i64))
}
fn fprintf_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // fprintf($handle, $format, ...$args) - write formatted to file handle
    // Simplified: just return 0 (we don't support file handles properly)
    Ok(Value::Long(0))
}

fn vfprintf_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // vfprintf($handle, $format, $args_array)
    // Simplified: just return 0
    Ok(Value::Long(0))
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
        let (key, val) = if let Some(eq_pos) = pair.find('=') {
            (&pair[..eq_pos], &pair[eq_pos + 1..])
        } else {
            (pair, "")
        };
        result.set(
            goro_core::array::ArrayKey::String(PhpString::from_string(key.to_string())),
            Value::String(PhpString::from_string(val.to_string())),
        );
    }
    // PHP 8 requires the second parameter (result variable)
    // For now, just return the parsed array
    Ok(Value::Array(Rc::new(RefCell::new(result))))
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
    let separator = args
        .get(2)
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_else(|| "&".to_string());
    let mut parts = Vec::new();
    if let Value::Array(arr) = data {
        for (key, val) in arr.borrow().iter() {
            let k = match key {
                goro_core::array::ArrayKey::Int(n) => n.to_string(),
                goro_core::array::ArrayKey::String(s) => s.to_string_lossy(),
            };
            let v = val.to_php_string().to_string_lossy();
            parts.push(format!("{}={}", k, v));
        }
    }
    Ok(Value::String(PhpString::from_string(
        parts.join(&separator),
    )))
}

// JSON functions moved to goro-ext-json

// === File stubs ===
fn file_exists_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    Ok(if std::path::Path::new(&path.to_string_lossy()).exists() {
        Value::True
    } else {
        Value::False
    })
}
fn is_file_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    Ok(if std::path::Path::new(&path.to_string_lossy()).is_file() {
        Value::True
    } else {
        Value::False
    })
}
fn is_dir_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
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
fn file_get_contents_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    match std::fs::read(&*path.to_string_lossy() as &str) {
        Ok(data) => Ok(Value::String(PhpString::from_vec(data))),
        Err(_) => Ok(Value::False),
    }
}
fn file_put_contents_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let data = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let flags = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    let append = flags & 8 != 0; // FILE_APPEND
    let result = if append {
        use std::io::Write;
        std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&*path.to_string_lossy() as &str)
            .and_then(|mut f| f.write_all(data.as_bytes()).map(|_| data.len()))
    } else {
        std::fs::write(&*path.to_string_lossy() as &str, data.as_bytes()).map(|_| data.len())
    };
    match result {
        Ok(len) => Ok(Value::Long(len as i64)),
        Err(_) => Ok(Value::False),
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
fn filesize_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    match std::fs::metadata(&*path.to_string_lossy() as &str) {
        Ok(m) => Ok(Value::Long(m.len() as i64)),
        Err(_) => Ok(Value::False),
    }
}
fn touch_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let path_str = path.to_string_lossy();
    let p = std::path::Path::new(&*path_str);
    if !p.exists() {
        std::fs::write(p, b"").ok();
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
    match std::fs::metadata(&*path.to_string_lossy() as &str) {
        Ok(m) => {
            let mut result = PhpArray::new();
            result.set(
                goro_core::array::ArrayKey::String(PhpString::from_bytes(b"size")),
                Value::Long(m.len() as i64),
            );
            result.set(
                goro_core::array::ArrayKey::Int(7),
                Value::Long(m.len() as i64),
            );
            Ok(Value::Array(Rc::new(RefCell::new(result))))
        }
        Err(_) => Ok(Value::False),
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
        return Err(VmError { message: msg, line: 0 });
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
fn register_shutdown_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
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
    Ok(
        if is_builtin
            || vm
                .classes
                .get(&name_lower)
                .map(|c| c.is_interface)
                .unwrap_or(false)
        {
            Value::True
        } else {
            Value::False
        },
    )
}
fn trait_exists_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let name = args.first().unwrap_or(&Value::Null).to_php_string();
    let name_lower: Vec<u8> = name
        .as_bytes()
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect();
    Ok(
        if vm
            .classes
            .get(&name_lower)
            .map(|c| c.is_trait)
            .unwrap_or(false)
        {
            Value::True
        } else {
            Value::False
        },
    )
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
fn get_object_vars_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Object(obj)) = args.first() {
        let obj = obj.borrow();
        let mut arr = PhpArray::new();
        for (name, val) in &obj.properties {
            arr.set(
                goro_core::array::ArrayKey::String(PhpString::from_vec(name.clone())),
                val.clone(),
            );
        }
        Ok(Value::Array(Rc::new(RefCell::new(arr))))
    } else {
        Ok(Value::False)
    }
}
fn get_class_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Object(obj)) = args.first() {
        let obj = obj.borrow();
        Ok(Value::String(PhpString::from_vec(obj.class_name.clone())))
    } else if let Some(Value::Generator(_)) = args.first() {
        Ok(Value::String(PhpString::from_bytes(b"Generator")))
    } else {
        Ok(Value::False)
    }
}
fn serialize_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    let s = serialize_value_depth(val, 0);
    Ok(Value::String(PhpString::from_string(s)))
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
            let class_name = String::from_utf8_lossy(&obj.class_name);
            let prop_count = obj.properties.len();
            let mut result = format!("O:{}:\"{}\":{}:{{", class_name.len(), class_name, prop_count);
            for (name, val) in &obj.properties {
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
            vm.emit_warning_at(&msg, 0);
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

            for _ in 0..prop_count {
                let key = unserialize_value(data, pos, vm)?;
                let value = unserialize_value(data, pos, vm)?;
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
            if *pos < data.len() && data[*pos] == b'}' {
                *pos += 1;
            }

            // Call __unserialize or __wakeup if they exist
            let obj_val = Value::Object(Rc::new(RefCell::new(obj)));
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
        b'C' => {
            // C:n:"ClassName":n:{...} (custom serializable)
            // Skip for now - parse but return stdClass
            *pos += 1;
            if *pos < data.len() && data[*pos] == b':' {
                *pos += 1;
            }
            // Skip class name length
            while *pos < data.len() && data[*pos] != b':' {
                *pos += 1;
            }
            if *pos < data.len() {
                *pos += 1;
            }
            // Skip class name
            if *pos < data.len() && data[*pos] == b'"' {
                *pos += 1;
                while *pos < data.len() && data[*pos] != b'"' {
                    *pos += 1;
                }
                if *pos < data.len() {
                    *pos += 1;
                }
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
    Ok(Value::Long(1024 * 1024)) // stub: 1MB
}
fn memory_get_peak_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(2 * 1024 * 1024)) // stub: 2MB
}
fn sleep_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(0))
}
fn usleep_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Null)
}
fn uniqid_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    use std::time::SystemTime;
    let t = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    Ok(Value::String(PhpString::from_string(format!(
        "{:x}{:05x}",
        t.as_secs(),
        t.subsec_micros()
    ))))
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
        unsafe { std::env::set_var(key, value); }
        Ok(Value::True)
    } else {
        unsafe { std::env::remove_var(&*s); }
        Ok(Value::True)
    }
}
fn spl_autoload_register_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True)
}
fn class_alias_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let original = args.first().unwrap_or(&Value::Null).to_php_string();
    let alias = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let original_lower: Vec<u8> = original.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
    let alias_lower: Vec<u8> = alias.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
    if let Some(class) = vm.classes.get(&original_lower).cloned() {
        vm.classes.insert(alias_lower, class);
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
                return Err(VmError { message: msg, line: 0 });
            }
            // For string values, throw TypeError too
            if let Value::String(s) = case_val {
                if s.to_string_lossy().parse::<i64>().is_err() {
                    let msg = "array_change_key_case(): Argument #2 ($case) must be of type int, string given".to_string();
                    let exc = vm.throw_type_error(msg.clone());
                    vm.current_exception = Some(exc);
                    return Err(VmError { message: msg, line: 0 });
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

fn array_multisort_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True) // stub
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

fn fopen_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False) // stub
}
fn fclose_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True)
}
fn fread_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}
fn fwrite_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}
fn fgets_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}
fn feof_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True)
}
fn rewind_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True)
}
fn fseek_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(0))
}
fn ftell_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(0))
}
fn fflush_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True)
}
fn unlink_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}
fn rename_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}
fn copy_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}
fn mkdir_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}
fn rmdir_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}
fn glob_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
}
fn scandir_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}
fn header_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Null)
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
            return Err(VmError { message: msg, line: 0 });
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
            let msg = format!("array_diff_assoc(): Argument #{} must be of type array, {} given", i + 1, type_name);
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: 0 });
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
            let msg = format!("array_intersect_key(): Argument #{} must be of type array, {} given", i + 1, type_name);
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: 0 });
        }
    }
    if args.len() < 2 {
        return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
    }
    if let (Some(Value::Array(a)), Some(Value::Array(b))) = (args.first(), args.get(1)) {
        let a = a.borrow();
        let b = b.borrow();
        let b_keys: Vec<_> = b.keys().cloned().collect();
        let mut result = PhpArray::new();
        for (key, val) in a.iter() {
            if b_keys.contains(key) {
                result.set(key.clone(), val.clone());
            }
        }
        Ok(Value::Array(Rc::new(RefCell::new(result))))
    } else {
        Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
    }
}

fn array_intersect_assoc_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    for (i, arg) in args.iter().enumerate() {
        let a = if let Value::Reference(r) = arg { r.borrow().clone() } else { arg.clone() };
        if !matches!(a, Value::Array(_)) {
            let type_name = Vm::value_type_name(&a);
            let msg = format!("array_intersect_assoc(): Argument #{} must be of type array, {} given", i + 1, type_name);
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: 0 });
        }
    }
    if args.len() < 2 {
        return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
    }
    if let (Some(Value::Array(a)), Some(Value::Array(b))) = (args.first(), args.get(1)) {
        let a = a.borrow();
        let b = b.borrow();
        let mut result = PhpArray::new();
        for (key, val) in a.iter() {
            if let Some(b_val) = b.get(key)
                && b_val.equals(val)
            {
                result.set(key.clone(), val.clone());
            }
        }
        Ok(Value::Array(Rc::new(RefCell::new(result))))
    } else {
        Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
    }
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

fn spl_object_hash_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Object(obj)) = args.first() {
        let id = obj.borrow().object_id;
        Ok(Value::String(PhpString::from_string(format!(
            "{:032x}",
            id
        ))))
    } else {
        Err(VmError {
            message: "spl_object_hash() expects an object".into(),
            line: 0,
        })
    }
}
fn spl_object_id_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Object(obj)) = args.first() {
        Ok(Value::Long(obj.borrow().object_id as i64))
    } else {
        Err(VmError {
            message: "spl_object_id() expects an object".into(),
            line: 0,
        })
    }
}
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
        let mut arr_mut = arr.borrow_mut();
        let mut entries: Vec<Value> = arr_mut.values().cloned().collect();

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
            entries.sort_by(|a, b| {
                let mut fn_cvs = vec![Value::Undef; user_fn.cv_names.len()];
                let mut idx = 0;
                for cv in &captured {
                    if idx < fn_cvs.len() {
                        fn_cvs[idx] = cv.clone();
                        idx += 1;
                    }
                }
                if idx < fn_cvs.len() {
                    fn_cvs[idx] = a.clone();
                    idx += 1;
                }
                if idx < fn_cvs.len() {
                    fn_cvs[idx] = b.clone();
                }
                let result = vm.execute_fn(&user_fn, fn_cvs).unwrap_or(Value::Long(0));
                let cmp = result.to_long();
                if cmp < 0 {
                    std::cmp::Ordering::Less
                } else if cmp > 0 {
                    std::cmp::Ordering::Greater
                } else {
                    std::cmp::Ordering::Equal
                }
            });
        }

        let mut new_arr = PhpArray::new();
        for val in entries {
            new_arr.push(val);
        }
        *arr_mut = new_arr;
    }
    Ok(Value::True)
}

fn uasort_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True)
}
fn uksort_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True)
}

fn array_reduce_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let arr = match args.first() {
        Some(Value::Array(a)) => a.borrow(),
        _ => return Ok(Value::Null),
    };
    let callback = args.get(1);
    let initial = args.get(2).cloned().unwrap_or(Value::Null);

    let entries: Vec<Value> = arr.values().cloned().collect();
    drop(arr);

    let (func_name, captured) = match callback {
        Some(Value::String(s)) => (s.as_bytes().to_vec(), vec![]),
        Some(Value::Array(cb)) => {
            let cb = cb.borrow();
            let vals: Vec<Value> = cb.values().cloned().collect();
            if vals.is_empty() {
                return Ok(initial);
            }
            (
                vals[0].to_php_string().as_bytes().to_vec(),
                vals[1..].to_vec(),
            )
        }
        _ => return Ok(initial),
    };
    let func_lower: Vec<u8> = func_name.iter().map(|b| b.to_ascii_lowercase()).collect();

    let mut carry = initial;
    if let Some(user_fn) = vm.user_functions.get(&func_lower).cloned() {
        for val in entries {
            let mut fn_cvs = vec![Value::Undef; user_fn.cv_names.len()];
            let mut idx = 0;
            for cv in &captured {
                if idx < fn_cvs.len() {
                    fn_cvs[idx] = cv.clone();
                    idx += 1;
                }
            }
            if idx < fn_cvs.len() {
                fn_cvs[idx] = carry;
                idx += 1;
            }
            if idx < fn_cvs.len() {
                fn_cvs[idx] = val;
            }
            carry = vm.execute_fn(&user_fn, fn_cvs)?;
        }
    } else if let Some(builtin) = vm.functions.get(&func_lower).copied() {
        for val in entries {
            carry = builtin(vm, &[carry, val])?;
        }
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
            Err(VmError { message: msg, line: 0 })
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
    // Similar to var_dump but shows refcount
    for arg in args {
        let s = format!("{:?}\n", arg);
        vm.write_output(s.as_bytes());
    }
    Ok(Value::Null)
}

#[allow(dead_code)]
fn var_dump_direct(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // This overrides the one in output.rs - but output.rs registers first
    // So this won't actually be called. Let's skip.
    Ok(Value::Null)
}

fn get_class_methods_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let class_name = match args.first() {
        Some(Value::String(s)) => s.as_bytes().to_vec(),
        Some(Value::Object(obj)) => obj.borrow().class_name.clone(),
        _ => return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new())))),
    };
    let class_lower: Vec<u8> = class_name.iter().map(|b| b.to_ascii_lowercase()).collect();

    let mut result = PhpArray::new();
    if let Some(class) = vm.classes.get(&class_lower) {
        // Collect methods with original names, filter by visibility from current scope
        let mut methods: Vec<_> = class
            .methods
            .values()
            .filter(|m| {
                matches!(
                    m.visibility,
                    goro_core::object::Visibility::Public
                )
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

fn iterator_to_array_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // For arrays, just return them. For generators, collect values.
    match args.first() {
        Some(Value::Array(arr)) => Ok(Value::Array(arr.clone())),
        Some(Value::Generator(_)) => {
            // TODO: iterate generator
            Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
        }
        _ => Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new())))),
    }
}

fn iterator_count_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    match args.first() {
        Some(Value::Array(arr)) => Ok(Value::Long(arr.borrow().len() as i64)),
        Some(Value::Generator(_)) => {
            Ok(Value::Long(0)) // TODO
        }
        _ => Ok(Value::Long(0)),
    }
}

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
        for prop in &class.properties {
            if !prop.is_static {
                result.set(
                    goro_core::array::ArrayKey::String(PhpString::from_vec(prop.name.clone())),
                    prop.default.clone(),
                );
            }
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn opendir_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    if std::path::Path::new(&*path.to_string_lossy()).is_dir() {
        Ok(Value::String(PhpString::from_string(format!(
            "dir:{}",
            path.to_string_lossy()
        ))))
    } else {
        Ok(Value::False)
    }
}

fn closedir_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Null)
}

fn readdir_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}

fn chmod_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
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

fn debug_backtrace_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
}

fn debug_print_backtrace_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Null)
}

fn array_key_exists_fn2(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let key = args.first().unwrap_or(&Value::Null);
    let arr = args.get(1).unwrap_or(&Value::Null);
    if let Value::Array(a) = arr {
        let arr_key = goro_core::vm::Vm::value_to_array_key(key.clone());
        Ok(if a.borrow().contains_key(&arr_key) {
            Value::True
        } else {
            Value::False
        })
    } else {
        Ok(Value::False)
    }
}

fn clearstatcache_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Null) // No-op - we don't cache stat results
}

fn array_walk_recursive_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
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
                let _ = vm.execute_fn(&user_fn, fn_cvs);
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

fn fgetcsv_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False) // Stub - needs file handle support
}

fn fileperms_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match std::fs::metadata(&*path.to_string_lossy()) {
            Ok(m) => Ok(Value::Long(m.mode() as i64)),
            Err(_) => Ok(Value::False),
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

fn get_resource_type_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(PhpString::from_bytes(b"Unknown")))
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
    match std::fs::read_to_string(&*path.to_string_lossy()) {
        Ok(content) => {
            let mut result = PhpArray::new();
            for line in content.split('\n') {
                let mut l = line.to_string();
                if flags & 2 == 0 {
                    // FILE_IGNORE_NEW_LINES not set - include newline
                    l.push('\n');
                }
                if flags & 4 != 0 && l.trim().is_empty() {
                    // FILE_SKIP_EMPTY_LINES
                    continue;
                }
                result.push(Value::String(PhpString::from_string(l)));
            }
            Ok(Value::Array(Rc::new(RefCell::new(result))))
        }
        Err(_) => Ok(Value::False),
    }
}

fn lstat_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Same as stat for now
    stat_fn(_vm, args)
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
            _ => {
                let n = value.to_long();
                if is_float {
                    product_float *= n as f64;
                } else {
                    product_int = product_int.wrapping_mul(n);
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
                    sum_int = sum_int.wrapping_add(n);
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
    array_u_op(vm, args, true, true, false)
}
fn array_intersect_uassoc_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    array_u_op(vm, args, true, true, true)
}
fn array_diff_ukey_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    array_u_op(vm, args, false, true, false)
}
fn array_diff_uassoc_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    array_u_op(vm, args, false, true, true)
}
fn array_udiff_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    array_u_op(vm, args, false, false, false)
}
fn array_udiff_assoc_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // udiff_assoc: compare values with callback, keys with ==
    array_udiff_with_key_check(vm, args, false)
}
fn array_udiff_uassoc_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // udiff_uassoc: two callbacks - value compare and key compare
    array_udiff_uassoc_impl(vm, args)
}
fn array_uintersect_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    array_u_op(vm, args, true, false, false)
}
fn array_uintersect_assoc_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // uintersect_assoc: compare values with callback, keys with ==
    array_udiff_with_key_check(vm, args, true)
}
fn array_uintersect_uassoc_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    array_uintersect_uassoc_impl(vm, args)
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
) -> Result<Value, VmError> {
    if args.len() < 3 {
        return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
    }
    let callback = args.last().unwrap().clone();
    let first = match &args[0] {
        Value::Array(arr) => arr.borrow().clone(),
        _ => return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new())))),
    };
    let other_arrays: Vec<PhpArray> = args[1..args.len() - 1]
        .iter()
        .filter_map(|v| {
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
) -> Result<Value, VmError> {
    if args.len() < 3 {
        return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
    }
    let callback = args.last().unwrap().clone();
    let first = match &args[0] {
        Value::Array(arr) => arr.borrow().clone(),
        _ => return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new())))),
    };
    let other_arrays: Vec<PhpArray> = args[1..args.len() - 1]
        .iter()
        .filter_map(|v| {
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

fn array_udiff_uassoc_impl(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.len() < 4 {
        return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
    }
    let key_callback = args[args.len() - 1].clone();
    let val_callback = args[args.len() - 2].clone();
    let first = match &args[0] {
        Value::Array(arr) => arr.borrow().clone(),
        _ => return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new())))),
    };
    let other_arrays: Vec<PhpArray> = args[1..args.len() - 2]
        .iter()
        .filter_map(|v| {
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

fn array_uintersect_uassoc_impl(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.len() < 4 {
        return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
    }
    let key_callback = args[args.len() - 1].clone();
    let val_callback = args[args.len() - 2].clone();
    let first = match &args[0] {
        Value::Array(arr) => arr.borrow().clone(),
        _ => return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new())))),
    };
    let other_arrays: Vec<PhpArray> = args[1..args.len() - 2]
        .iter()
        .filter_map(|v| {
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
            let val_str = trimmed[eq_pos+1..].trim();
            // Remove surrounding quotes
            let val_str = if (val_str.starts_with('"') && val_str.ends_with('"'))
                || (val_str.starts_with('\'') && val_str.ends_with('\''))
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
    // Return previous value for the option
    match option {
        1 => Ok(Value::Long(1)),  // ASSERT_ACTIVE
        2 => Ok(Value::Long(0)),  // ASSERT_WARNING (deprecated)
        3 => Ok(Value::Long(0)),  // ASSERT_BAIL (deprecated)
        4 => Ok(Value::Long(0)),  // ASSERT_QUIET_EVAL (deprecated)
        5 => Ok(Value::Null),     // ASSERT_CALLBACK (deprecated)
        6 => Ok(Value::Long(1)),  // ASSERT_EXCEPTION
        _ => Ok(Value::False),
    }
}

fn ftruncate_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // ftruncate($handle, $size) - stub
    Ok(Value::True)
}

fn tmpfile_fn(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // Return a file handle to a temp file - simplified stub
    // In PHP this returns a resource; we'll return false for now
    Ok(Value::False)
}

fn filemtime_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    match std::fs::metadata(&*path.to_string_lossy()) {
        Ok(meta) => {
            if let Ok(modified) = meta.modified() {
                let secs = modified.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
                Ok(Value::Long(secs as i64))
            } else {
                Ok(Value::False)
            }
        }
        Err(_) => Ok(Value::False),
    }
}

fn fileatime_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    match std::fs::metadata(&*path.to_string_lossy()) {
        Ok(meta) => {
            if let Ok(accessed) = meta.accessed() {
                let secs = accessed.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
                Ok(Value::Long(secs as i64))
            } else {
                Ok(Value::False)
            }
        }
        Err(_) => Ok(Value::False),
    }
}

fn filectime_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    filemtime_fn(vm, args)
}

fn fileinode_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match std::fs::metadata(&*path.to_string_lossy()) {
            Ok(meta) => Ok(Value::Long(meta.ino() as i64)),
            Err(_) => Ok(Value::False),
        }
    }
    #[cfg(not(unix))]
    Ok(Value::False)
}

fn fileowner_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match std::fs::metadata(&*path.to_string_lossy()) {
            Ok(meta) => Ok(Value::Long(meta.uid() as i64)),
            Err(_) => Ok(Value::False),
        }
    }
    #[cfg(not(unix))]
    Ok(Value::Long(0))
}

fn filegroup_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match std::fs::metadata(&*path.to_string_lossy()) {
            Ok(meta) => Ok(Value::Long(meta.gid() as i64)),
            Err(_) => Ok(Value::False),
        }
    }
    #[cfg(not(unix))]
    Ok(Value::Long(0))
}

fn chown_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}

fn fputcsv_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}

fn fpassthru_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
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

fn defined_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let name = args.first().unwrap_or(&Value::Null).to_php_string();
    let name_bytes = name.as_bytes();
    if vm.constants.contains_key(name_bytes) {
        Ok(Value::True)
    } else {
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

fn class_implements_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let class_name = match args.first().unwrap_or(&Value::Null) {
        Value::String(s) => s.as_bytes().to_vec(),
        Value::Object(obj) => obj.borrow().class_name.clone(),
        _ => return Ok(Value::False),
    };
    let class_lower: Vec<u8> = class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
    let mut result = PhpArray::new();

    // Get interfaces from the class definition
    if let Some(class) = vm.classes.get(&class_lower) {
        for iface in &class.interfaces {
            let iface_str = PhpString::from_vec(iface.clone());
            result.set(ArrayKey::String(iface_str.clone()), Value::String(iface_str));
        }
    }

    // Also check built-in interface implementations
    let builtins = goro_core::vm::get_builtin_interfaces(&class_lower);
    for iface in builtins {
        let iface_str = PhpString::from_vec(iface.clone());
        result.set(ArrayKey::String(iface_str.clone()), Value::String(iface_str));
    }

    // Walk parent chain for inherited interfaces
    let mut current = class_lower.clone();
    for _ in 0..50 {
        let parent = if let Some(class) = vm.classes.get(&current) {
            class.parent.clone()
        } else {
            None
        };
        if let Some(parent_name) = parent {
            let parent_lower: Vec<u8> = parent_name.iter().map(|b| b.to_ascii_lowercase()).collect();
            if let Some(parent_class) = vm.classes.get(&parent_lower) {
                for iface in &parent_class.interfaces {
                    let iface_str = PhpString::from_vec(iface.clone());
                    result.set(ArrayKey::String(iface_str.clone()), Value::String(iface_str));
                }
            }
            let parent_builtins = goro_core::vm::get_builtin_interfaces(&parent_lower);
            for iface in parent_builtins {
                let iface_str = PhpString::from_vec(iface.clone());
                result.set(ArrayKey::String(iface_str.clone()), Value::String(iface_str));
            }
            current = parent_lower;
        } else {
            break;
        }
    }

    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn class_parents_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let class_name = match args.first().unwrap_or(&Value::Null) {
        Value::String(s) => s.as_bytes().to_vec(),
        Value::Object(obj) => obj.borrow().class_name.clone(),
        _ => return Ok(Value::False),
    };
    let class_lower: Vec<u8> = class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
    let mut result = PhpArray::new();

    let mut current = class_lower;
    for _ in 0..50 {
        let parent = if let Some(class) = vm.classes.get(&current) {
            class.parent.clone()
        } else {
            // Check built-in parent chains
            let bp = goro_core::vm::get_builtin_parent(&current);
            bp.map(|p| p.to_vec())
        };
        if let Some(parent_name) = parent {
            let parent_lower: Vec<u8> = parent_name.iter().map(|b| b.to_ascii_lowercase()).collect();
            let display_name = if let Some(class) = vm.classes.get(&parent_lower) {
                class.name.clone()
            } else {
                // Canonicalize built-in class names
                goro_core::vm::canonicalize_class_name(&parent_lower)
            };
            let name_str = PhpString::from_vec(display_name);
            result.set(ArrayKey::String(name_str.clone()), Value::String(name_str));
            current = parent_lower;
        } else {
            break;
        }
    }

    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn class_uses_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let class_name = match args.first().unwrap_or(&Value::Null) {
        Value::String(s) => s.as_bytes().to_vec(),
        Value::Object(obj) => obj.borrow().class_name.clone(),
        _ => return Ok(Value::False),
    };
    let class_lower: Vec<u8> = class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
    let mut result = PhpArray::new();

    if let Some(class) = vm.classes.get(&class_lower) {
        for trait_name in &class.traits {
            let trait_str = PhpString::from_vec(trait_name.clone());
            result.set(ArrayKey::String(trait_str.clone()), Value::String(trait_str));
        }
    }

    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn str_increment_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return Err(VmError { message: "str_increment(): Argument #1 ($string) must not be empty".to_string(), line: 0 });
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
                return Err(VmError { message: "str_increment(): Argument #1 ($string) must be composed only of alphanumeric ASCII characters".to_string(), line: 0 });
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

fn str_decrement_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return Err(VmError { message: "str_decrement(): Argument #1 ($string) must not be empty".to_string(), line: 0 });
    }
    // Cannot decrement 'a', 'A', or '0'
    if bytes.len() == 1 && (bytes[0] == b'a' || bytes[0] == b'A' || bytes[0] == b'0') {
        return Err(VmError { message: "str_decrement(): Argument #1 ($string) \"".to_string() + &String::from_utf8_lossy(bytes) + "\" is out of decrement range", line: 0 });
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
                return Err(VmError { message: "str_decrement(): Argument #1 ($string) must be composed only of alphanumeric ASCII characters".to_string(), line: 0 });
            }
        }
    }
    // Remove leading zero/a/A if result starts with it and has more chars
    if result.len() > 1 && (result[0] == b'0' || result[0] == b'a' || result[0] == b'A') && borrow {
        result.remove(0);
    }
    Ok(Value::String(PhpString::from_vec(result)))
}

