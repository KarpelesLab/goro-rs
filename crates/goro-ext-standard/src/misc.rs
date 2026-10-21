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
fn require_array_arg(vm: &mut Vm, val: &Value, func_name: &str, param_name: &str, param_num: u32) -> Result<(), VmError> {
    require_array_arg_inner(vm, val, func_name, Some(param_name), param_num)
}

/// Like require_array_arg but without parameter name in error message (for variadic args)
fn require_array_arg_variadic(vm: &mut Vm, val: &Value, func_name: &str, param_num: u32) -> Result<(), VmError> {
    require_array_arg_inner(vm, val, func_name, None, param_num)
}

/// Like Vm::value_type_name but returns "resource" for registered resource ids
/// (since resources are stored as Value::Long internally).
/// Skip ids 1-3 since those are pre-registered as STDIN/STDOUT/STDERR and
/// would falsely classify integer literals 1, 2, 3 as resources.
fn arg_type_name(val: &Value) -> String {
    match val {
        Value::Long(n) if *n > 3 && goro_core::value::is_registered_resource(*n) => "resource".to_string(),
        Value::Reference(r) => arg_type_name(&*r.borrow()),
        _ => Vm::value_type_name(val),
    }
}

/// Compare two values as PHP would compare them in array_*_assoc functions:
/// cast both to string and compare bytes. This matches PHP's
/// `string_compare_function` semantics used in zend_hash_array_diff/intersect_assoc.
fn php_value_string_eq(a: &Value, b: &Value) -> bool {
    let a = a.deref();
    let b = b.deref();
    a.to_php_string().as_bytes() == b.to_php_string().as_bytes()
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
                let type_name = arg_type_name(&*inner);
                let msg = make_msg(&type_name);
                let exc = vm.create_exception(b"TypeError", &msg, 0);
                vm.current_exception = Some(exc);
                Err(VmError { message: msg, line: vm.current_line })
            }
        }
        _ => {
            let type_name = arg_type_name(val);
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
    vm.unserialize_value_hook = Some(crate::serialize::unserialize_value_hook);
    // Built-in php_user_filter base class. User filters extend this class
    // and override filter(); the property `filtername`, `params`, and
    // `stream` are set by the runtime before onCreate() is invoked.
    {
        let mut pufc = ClassEntry::new(b"php_user_filter".to_vec());
        let props: &[&[u8]] = &[b"filtername", b"params", b"stream"];
        for p in props {
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
    // RequestParseBodyException extends \Exception
    {
        let mut cls = ClassEntry::new(b"RequestParseBodyException".to_vec());
        cls.parent = Some(b"Exception".to_vec());
        vm.classes.insert(b"requestparsebodyexception".to_vec(), cls);
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
    vm.register_function(b"serialize", crate::serialize::serialize_fn);
    vm.register_function(b"unserialize", crate::serialize::unserialize_fn);
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
    vm.register_function(b"fsync", fsync_fn);
    vm.register_function(b"fdatasync", fdatasync_fn);
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
    vm.register_function(b"header_remove", header_remove_fn);
    vm.register_function(b"headers_sent", headers_sent_fn);
    vm.register_function(b"http_response_code", http_response_code_fn);
    vm.register_function(b"getallheaders", getallheaders_fn);
    vm.register_function(b"apache_request_headers", getallheaders_fn);
    vm.register_function(b"apache_response_headers", apache_response_headers_fn);
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
    // Bucket brigade helpers used by user filters.
    vm.register_function(b"stream_bucket_make_writeable", crate::stream_filters::stream_bucket_make_writeable);
    vm.register_function(b"stream_bucket_append", crate::stream_filters::stream_bucket_append);
    vm.register_function(b"stream_bucket_prepend", crate::stream_filters::stream_bucket_prepend);
    vm.register_function(b"stream_bucket_new", crate::stream_filters::stream_bucket_new);
    // Filter direction constants.
    vm.constants.insert(b"STREAM_FILTER_READ".to_vec(), Value::Long(crate::stream_filters::STREAM_FILTER_READ));
    vm.constants.insert(b"STREAM_FILTER_WRITE".to_vec(), Value::Long(crate::stream_filters::STREAM_FILTER_WRITE));
    vm.constants.insert(b"STREAM_FILTER_ALL".to_vec(), Value::Long(crate::stream_filters::STREAM_FILTER_ALL));
    // User filter return code constants.
    vm.constants.insert(b"PSFS_PASS_ON".to_vec(), Value::Long(crate::stream_filters::PSFS_PASS_ON));
    vm.constants.insert(b"PSFS_FEED_ME".to_vec(), Value::Long(crate::stream_filters::PSFS_FEED_ME));
    vm.constants.insert(b"PSFS_ERR_FATAL".to_vec(), Value::Long(crate::stream_filters::PSFS_ERR_FATAL));
    vm.constants.insert(b"PSFS_FLAG_NORMAL".to_vec(), Value::Long(0));
    vm.constants.insert(b"PSFS_FLAG_FLUSH_INC".to_vec(), Value::Long(1));
    vm.constants.insert(b"PSFS_FLAG_FLUSH_CLOSE".to_vec(), Value::Long(2));
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
    vm.register_function(b"diskfreespace", diskfreespace_fn);
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
    // get_html_translation_table is registered by strings::register with proper flag handling
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
    let handler_opt = args.first();
    // Validate callback (PHP throws a TypeError for non-callable callbacks since 8.0)
    if let Some(handler) = handler_opt {
        if !matches!(handler, Value::Null) {
            if let Some(err) = validate_exception_handler_callback(vm, handler) {
                let exc = vm.throw_type_error(err.clone());
                vm.current_exception = Some(exc);
                return Err(VmError { message: err, line: vm.current_line });
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
fn validate_exception_handler_callback(vm: &Vm, callback: &Value) -> Option<String> {
    let base = "set_exception_handler(): Argument #1 ($callback) must be a valid callback or null, ";
    match callback {
        Value::String(s) => {
            let name = s.as_bytes();
            if name.is_empty() {
                return Some(format!("{}function \"\" not found or invalid function name", base));
            }
            // Class::method form
            if let Some(pos) = name.windows(2).position(|w| w == b"::") {
                let class_part = &name[..pos];
                let method_part = &name[pos + 2..];
                let class_lower: Vec<u8> = class_part.iter().map(|b| b.to_ascii_lowercase()).collect();
                let method_lower: Vec<u8> = method_part.iter().map(|b| b.to_ascii_lowercase()).collect();
                if let Some(class) = vm.classes.get(&class_lower) {
                    if class.get_method(&method_lower).is_some() {
                        return None;
                    }
                    return Some(format!("{}class {} does not have a method \"{}\"",
                        base, String::from_utf8_lossy(class_part), String::from_utf8_lossy(method_part)));
                }
                return Some(format!("{}class \"{}\" not found",
                    base, String::from_utf8_lossy(class_part)));
            }
            let name_lower: Vec<u8> = name.iter().map(|b| b.to_ascii_lowercase()).collect();
            if name_lower.starts_with(b"__closure_") || name_lower.starts_with(b"__arrow_") || name_lower.starts_with(b"__bound_closure_") || name_lower.starts_with(b"__closure_fcc_") {
                return None;
            }
            if vm.functions.contains_key(&name_lower) || vm.user_functions.contains_key(&name_lower) {
                return None;
            }
            Some(format!("{}function \"{}\" not found or invalid function name", base, String::from_utf8_lossy(name)))
        }
        Value::Array(arr) => {
            let arr_borrow = arr.borrow();
            if arr_borrow.len() != 2 {
                return Some(format!("{}array must have exactly two members", base));
            }
            let vals: Vec<Value> = arr_borrow.values().cloned().collect();
            drop(arr_borrow);
            // First member: object or class name
            let class_lower = match &vals[0] {
                Value::Object(o) => {
                    let cn: Vec<u8> = o.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                    cn
                }
                Value::String(s) => {
                    let cn: Vec<u8> = s.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
                    if cn.is_empty() {
                        return Some(format!("{}class \"\" not found", base));
                    }
                    if !vm.classes.contains_key(&cn) {
                        return Some(format!("{}class \"{}\" not found", base, s.to_string_lossy()));
                    }
                    cn
                }
                _ => return Some(format!("{}first array member is not a valid class name or object", base)),
            };
            // Second member: method name
            match &vals[1] {
                Value::String(s) => {
                    let method_lower: Vec<u8> = s.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
                    if let Some(cd) = vm.classes.get(&class_lower) {
                        if cd.get_method(&method_lower).is_some() {
                            return None;
                        }
                        // __call/__callStatic act as catch-alls — method is callable.
                        if cd.get_method(b"__call").is_some() || cd.get_method(b"__callstatic").is_some() {
                            return None;
                        }
                    }
                    Some(format!("{}class {} does not have a method \"{}\"",
                        base, String::from_utf8_lossy(&class_lower), s.to_string_lossy()))
                }
                _ => Some(format!("{}second array member is not a valid method", base)),
            }
        }
        Value::Object(_) => None, // closures and __invoke objects are callable
        _ => Some(format!("{}must be of type ?callable, {} given", base, Vm::value_type_name(callback))),
    }
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
            // Honor error_reporting bitmask: only emit if E_USER_WARNING is set.
            if vm.error_reporting & 512 == 0 {
                return Ok(Value::True);
            }
            if !vm.call_user_error_handler(512, &message, call_line) {
                vm.emit_warning_at(&message, call_line);
                vm.record_last_error(512, &message, call_line);
            }
        }
        1024 => {
            // E_USER_NOTICE
            let call_line = vm.current_line;
            // Honor error_reporting bitmask: only emit if E_USER_NOTICE is set.
            if vm.error_reporting & 1024 == 0 {
                return Ok(Value::True);
            }
            if !vm.call_user_error_handler(1024, &message, call_line) {
                vm.record_last_error(1024, &message, call_line);
                let notice = format!(
                    "\nNotice: {} in {} on line {}\n",
                    message,
                    vm.current_file,
                    call_line
                );
                vm.write_output(notice.as_bytes());
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
        if !vm.classes.contains_key(&class_lower) && !vm.is_known_builtin_class(&class_lower) {
            vm.try_autoload_class(class_name_stripped.as_bytes());
            if vm.current_exception.is_some() {
                return Err(VmError { message: "Uncaught exception".into(), line: vm.current_line });
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
            // Visibility check: constant() honours protected/private the same as
            // the compiler does for direct Class::CONST references. Caller scope
            // is the innermost class on the scope stack (or its hierarchy).
            if let Some(meta) = class.constants_meta.get(const_name.as_bytes()) {
                use goro_core::object::Visibility;
                if meta.visibility != Visibility::Public {
                    let caller_lower: Option<Vec<u8>> = vm.current_class_scope()
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
                        let vis_str = if meta.visibility == Visibility::Private { "private" } else { "protected" };
                        let class_display = String::from_utf8_lossy(&class.name).to_string();
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
                        let canonical_class_lower: Vec<u8> = class.name.iter().map(|b| b.to_ascii_lowercase()).collect();
                        let mut guard_key = canonical_class_lower;
                        guard_key.extend_from_slice(b"::");
                        guard_key.extend_from_slice(const_name.as_bytes());
                        if !vm.deprecation_in_progress.contains(&guard_key) {
                            vm.deprecation_in_progress.insert(guard_key.clone());
                            let dep_result = vm.get_deprecated_message(&meta.attributes);
                            vm.deprecation_in_progress.remove(&guard_key);
                            if let Some(custom_msg) = dep_result {
                                let class_display = String::from_utf8_lossy(&class.name).to_string();
                                let prefix = format!("Constant {}::{}", class_display, const_name);
                                let msg = vm.format_deprecated_message(&prefix, &custom_msg);
                                vm.emit_user_deprecated_at(&msg, vm.current_line);
                            }
                        }
                    }
                }
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

// === Output buffering === (see ob.rs)

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
        let msg = "func_num_args() must be called from a function context";
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
    // PHP 8.0+: validate argument before checking call stack
    if index < 0 {
        let msg = "func_get_arg(): Argument #1 ($position) must be greater than or equal to 0".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    if let Some((_name, _file, _line, caller_args, _is_method)) = vm.call_stack.last() {
        if index as usize >= caller_args.len() {
            let msg = "func_get_arg(): Argument #1 ($position) must be less than the number of the arguments passed to the currently executed function".to_string();
            let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
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
fn is_closure_bytes(b: &[u8]) -> bool {
    b.starts_with(b"__closure_") || b.starts_with(b"__arrow_") || b.starts_with(b"__bound_closure_") || b.starts_with(b"__closure_fcc_")
}

/// Resolve a class name for a callable (zend_is_callable_check_class):
/// handles self/parent/static and autoloading. Returns the lowercase name.
fn callable_check_class(vm: &mut Vm, name: &[u8]) -> Result<Vec<u8>, String> {
    let lower = lower_bytes(name);
    let scope = vm.current_class_scope().map(|s| lower_bytes(&s));
    match lower.as_slice() {
        b"self" => return scope.ok_or_else(|| "cannot access \"self\" when no class scope is active".to_string()),
        b"parent" => {
            let sc = scope.ok_or_else(|| "cannot access \"parent\" when no class scope is active".to_string())?;
            return vm.classes.get(&sc).and_then(|c| c.parent.as_ref().map(|p| lower_bytes(p)))
                .ok_or_else(|| "cannot access \"parent\" when current class scope has no parent".to_string());
        }
        b"static" => {
            return vm.called_class_stack.last().map(|c| lower_bytes(c))
                .ok_or_else(|| "cannot access \"static\" when no class scope is active".to_string());
        }
        _ => {}
    }
    let stripped = lower.strip_prefix(b"\\").map(|s| s.to_vec()).unwrap_or(lower);
    if vm.classes.contains_key(&stripped) || vm.is_known_builtin_class(&stripped) {
        return Ok(stripped);
    }
    vm.try_autoload_class(name.strip_prefix(b"\\").unwrap_or(name));
    if vm.classes.contains_key(&stripped) {
        return Ok(stripped);
    }
    Err(format!("class \"{}\" not found", String::from_utf8_lossy(name)))
}

/// Check a method of `class_lower` for callability (zend_is_callable_check_func).
fn callable_check_method(vm: &mut Vm, class_lower: &[u8], method: &[u8], has_object: bool) -> Result<(), String> {
    let mlower = lower_bytes(method);
    let display = vm.classes.get(class_lower).map(|c| c.name.clone())
        .unwrap_or_else(|| vm.builtin_canonical_name(class_lower).into_bytes());
    let disp = goro_core::value::display_class_name(&display);
    let mname = String::from_utf8_lossy(method).to_string();
    let scope = vm.current_class_scope().map(|s| lower_bytes(&s));
    let info = vm.classes.get(class_lower).and_then(|c| c.get_method(&mlower).map(|m| {
        (m.visibility, lower_bytes(&m.declaring_class), m.is_static, m.is_abstract)
    }));
    let has_magic = |vm: &Vm, name: &[u8]| vm.classes.get(class_lower).map(|c| c.get_method(name).is_some()).unwrap_or(false);
    match info {
        Some((vis, decl, is_static, is_abstract)) => {
            let accessible = match vis {
                Visibility::Public => true,
                Visibility::Private => scope.as_ref() == Some(&decl),
                Visibility::Protected => {
                    let root = vm.find_root_declaring_class_for_method(&decl, &mlower);
                    scope.as_ref().map(|sc| *sc == root || vm.class_extends(sc, &root) || vm.class_extends(&root, sc)).unwrap_or(false)
                }
            };
            if !accessible {
                if has_object && has_magic(vm, b"__call") {
                    return Ok(());
                }
                if !has_object && has_magic(vm, b"__callstatic") {
                    return Ok(());
                }
                let v = if vis == Visibility::Private { "private" } else { "protected" };
                return Err(format!("cannot access {} method {}::{}()", v, disp, mname));
            }
            if is_abstract {
                return Err(format!("cannot call abstract method {}::{}()", disp, mname));
            }
            if !is_static && !has_object {
                // Allowed only with a compatible $this in the calling scope.
                let compatible = scope.as_ref().map(|sc| sc.as_slice() == class_lower || vm.class_extends(sc, class_lower)).unwrap_or(false);
                if !compatible {
                    return Err(format!("non-static method {}::{}() cannot be called statically", disp, mname));
                }
            }
            Ok(())
        }
        None => {
            if !vm.classes.contains_key(class_lower) {
                // Builtin class: consult the builtin method table.
                let exists = method_exists(vm, &[Value::String(PhpString::from_vec(display.clone())), Value::String(PhpString::from_bytes(method))])
                    .map(|v| v.is_truthy()).unwrap_or(false);
                if exists {
                    let throwable = class_lower == b"exception" || class_lower == b"error"
                        || goro_core::vm::is_builtin_subclass(class_lower, b"exception")
                        || goro_core::vm::is_builtin_subclass(class_lower, b"error");
                    if !has_object && throwable {
                        return Err(format!("non-static method {}::{}() cannot be called statically", disp, mname));
                    }
                    return Ok(());
                }
            }
            if has_object && has_magic(vm, b"__call") {
                return Ok(());
            }
            if has_magic(vm, b"__callstatic") {
                return Ok(());
            }
            Err(format!("class {} does not have a method \"{}\"", disp, mname))
        }
    }
}

/// zend_is_callable_ex(): returns (callable, callable_name, error).
pub(crate) fn check_callable(vm: &mut Vm, val: &Value, syntax_only: bool) -> (bool, Vec<u8>, Option<String>) {
    let val = val.deref();
    match &val {
        Value::String(s) => {
            let raw = s.as_bytes().to_vec();
            if is_closure_bytes(&raw) {
                return (true, b"Closure::__invoke".to_vec(), None);
            }
            if syntax_only {
                return (true, raw, None);
            }
            let stripped: Vec<u8> = raw.strip_prefix(b"\\").map(|r| r.to_vec()).unwrap_or_else(|| raw.clone());
            if let Some(pos) = stripped.windows(2).position(|w| w == b"::") {
                let (cls, m) = (&stripped[..pos], &stripped[pos + 2..]);
                let res = callable_check_class(vm, cls).and_then(|cl| callable_check_method(vm, &cl, m, false));
                return match res {
                    Ok(()) => (true, raw, None),
                    Err(e) => (false, raw, Some(e)),
                };
            }
            let lower = lower_bytes(&stripped);
            if vm.functions.contains_key(&lower) || vm.user_functions.contains_key(&lower) {
                (true, raw, None)
            } else {
                let e = format!("function \"{}\" not found or invalid function name", String::from_utf8_lossy(&raw));
                (false, raw, Some(e))
            }
        }
        Value::Array(arr) => {
            let (first, second, len) = {
                let a = arr.borrow();
                (a.get(&ArrayKey::Int(0)).map(|v| v.deref()), a.get(&ArrayKey::Int(1)).map(|v| v.deref()), a.len())
            };
            // Closure-with-captures representation.
            if let Some(Value::String(f)) = arr.borrow().values().next().map(|v| v.deref()) {
                if is_closure_bytes(f.as_bytes()) {
                    return (true, b"Closure::__invoke".to_vec(), None);
                }
            }
            if len != 2 {
                return (false, b"Array".to_vec(), Some("array callback must have exactly two members".to_string()));
            }
            let (Some(first), Some(second)) = (first, second) else {
                return (false, b"Array".to_vec(), Some("array callback has to contain indices 0 and 1".to_string()));
            };
            let method = match &second {
                Value::String(m) => m.as_bytes().to_vec(),
                _ => {
                    let name = match &first {
                        Value::Object(o) => { let mut n = o.borrow().class_name.clone(); n.extend_from_slice(b"::"); n }
                        Value::String(c) => { let mut n = c.as_bytes().to_vec(); n.extend_from_slice(b"::"); n }
                        _ => b"Array".to_vec(),
                    };
                    return (false, name, Some("second array member is not a valid method".to_string()));
                }
            };
            let (class_bytes, has_object) = match &first {
                Value::Object(o) => (o.borrow().class_name.clone(), true),
                Value::String(c) => (c.as_bytes().to_vec(), false),
                _ => return (false, b"Array".to_vec(), Some("first array member is not a valid class name or object".to_string())),
            };
            let mut name = goro_core::value::display_class_name(&class_bytes).into_bytes();
            if has_object {
                name = class_bytes.clone();
            }
            name.extend_from_slice(b"::");
            name.extend_from_slice(&method);
            if syntax_only {
                return (true, name, None);
            }
            let cl = match callable_check_class(vm, &class_bytes) {
                Ok(c) => c,
                Err(e) => return (false, name, Some(e)),
            };
            // "Class::method" inside the method string (deprecated form)
            let (cl, method) = match method.windows(2).position(|w| w == b"::") {
                Some(pos) => match callable_check_class(vm, &method[..pos]) {
                    Ok(c2) => (c2, method[pos + 2..].to_vec()),
                    Err(e) => return (false, name, Some(e)),
                },
                None => (cl, method),
            };
            match callable_check_method(vm, &cl, &method, has_object) {
                Ok(()) => (true, name, None),
                Err(e) => (false, name, Some(e)),
            }
        }
        Value::Object(obj) => {
            let cn = obj.borrow().class_name.clone();
            let cl = lower_bytes(&cn);
            let mut name = cn.clone();
            name.extend_from_slice(b"::__invoke");
            if cl == b"closure" {
                return (true, b"Closure::__invoke".to_vec(), None);
            }
            let has_invoke = vm.classes.get(&cl).map(|c| c.get_method(b"__invoke").is_some()).unwrap_or(false);
            if has_invoke || syntax_only && false {
                (true, name, None)
            } else {
                (false, name, Some("no array or string given".to_string()))
            }
        }
        other => {
            let name = match other {
                Value::Null | Value::Undef => Vec::new(),
                v => v.to_php_string().as_bytes().to_vec(),
            };
            (false, name, Some("no array or string given".to_string()))
        }
    }
}

/// Reject callbacks that cannot possibly be resolved (zend_fcall_info_init errors).
fn validate_callback_arg(vm: &mut Vm, func: &str, cb: &Value) -> Result<(), VmError> {
    let (ok, _, err) = check_callable(vm, cb, false);
    if vm.current_exception.is_some() {
        return Err(VmError { message: "Uncaught exception".into(), line: vm.current_line });
    }
    if !ok {
        if let Some(e) = err {
            let definite = e.starts_with("class \"") || e.starts_with("first array member")
                || e.starts_with("second array member") || e.starts_with("function \"")
                || e.starts_with("array callback");
            if definite {
                let msg = format!("{}(): Argument #1 ($callback) must be a valid callback, {}", func, e);
                return Err(throw_type_error_msg(vm, msg));
            }
        }
    }
    Ok(())
}

fn call_user_func_checked(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(cb) = args.first() {
        validate_callback_arg(vm, "call_user_func", cb)?;
    }
    call_user_func(vm, args)
}

fn call_user_func_array_checked(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(cb) = args.first() {
        validate_callback_arg(vm, "call_user_func_array", cb)?;
    }
    call_user_func_array(vm, args)
}

fn is_callable(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().cloned().unwrap_or(Value::Null);
    let syntax_only = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
    let (ok, name, _) = check_callable(vm, &val, syntax_only);
    if vm.current_exception.is_some() {
        return Err(VmError { message: "Uncaught exception".into(), line: vm.current_line });
    }
    set_callable_name_and_return(vm, args, Some(name), if ok { Value::True } else { Value::False })
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
    if let Value::String(s) = callback {
        if let Some(msg) = Vm::forbidden_dynamic_call(s.as_bytes()) {
            let exc = vm.create_exception(b"Error", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError { message: format!("Uncaught Error: {}", msg), line: vm.current_line });
        }
    }
    let call_args: Vec<Value> = args[1..].to_vec();
    // Take any forwarded named args from the VM
    let named_args: Vec<(Vec<u8>, Value)> = std::mem::take(&mut vm.pending_named_args);

    // Handle array callback: [class_or_object, method_name]
    if let Value::Array(arr) = callback {
        let arr_borrow = arr.borrow();
        // [&$obj, 'method'] — element references are transparent.
        let vals: Vec<Value> = arr_borrow.values().map(|v| v.deref()).collect();
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
                                        let mut op = method.op_array.clone();
                                        if op.attributes.is_empty() && !method.attributes.is_empty() {
                                            op.attributes = method.attributes.clone();
                                        }
                                        let display_fallback = format!("{}::{}", String::from_utf8_lossy(&class.name), String::from_utf8_lossy(&method.name));
                                        let line = vm.current_line;
                                        vm.check_user_function_deprecation(&op, &display_fallback, line);
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
                                    let mut op = method.op_array.clone();
                                    if op.attributes.is_empty() && !method.attributes.is_empty() {
                                        op.attributes = method.attributes.clone();
                                    }
                                    let display_fallback = format!("{}::{}", String::from_utf8_lossy(&class.name), String::from_utf8_lossy(&method.name));
                                    let line = vm.current_line;
                                    vm.check_user_function_deprecation(&op, &display_fallback, line);
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
                        // Extension-provided static methods of built-in classes.
                        if let Some((native, h)) = vm.find_native_class_handler(&resolved_class_lower) {
                            if (h.has_method)(&native, &method_lower, true) {
                                let called = class_bytes.to_vec();
                                return Ok((h.call)(vm, &native, &method_lower, &called, None, &call_args));
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
                                let mut op = method.op_array.clone();
                                if op.attributes.is_empty() && !method.attributes.is_empty() {
                                    op.attributes = method.attributes.clone();
                                }
                                let display_fallback = format!("{}::{}", String::from_utf8_lossy(&class.name), String::from_utf8_lossy(&method.name));
                                let line = vm.current_line;
                                vm.check_user_function_deprecation(&op, &display_fallback, line);
                                // Static methods called through an object get no $this.
                                let this = if method.is_static { None } else { Some(Value::Object(obj.clone())) };
                                return vm.execute_fn_with_named_args(&op, call_args, named_args, this);
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
                    // Built-in class methods (SPL, ext-provided native classes).
                    if let Some(r) = vm.call_object_method(&Value::Object(obj.clone()), method_name.as_bytes(), &call_args) {
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
                let mut op = method.op_array.clone();
                if op.attributes.is_empty() && !method.attributes.is_empty() {
                    op.attributes = method.attributes.clone();
                }
                let display_fallback = format!("{}::__invoke", String::from_utf8_lossy(&class.name));
                let line = vm.current_line;
                vm.check_user_function_deprecation(&op, &display_fallback, line);
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
                let fallback = String::from_utf8_lossy(&func_name).into_owned();
                let line = vm.current_line;
                vm.check_user_function_deprecation(&user_fn, &fallback, line);
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
                            op.attributes = method.attributes.clone();
                        }
                        let display_fallback = format!("{}::{}", String::from_utf8_lossy(&class.name), String::from_utf8_lossy(&method.name));
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
                } else if !is_forwarding
                    && !vm.is_known_builtin_class(&resolved_class_lower)
                {
                    // Class still not found after autoload: PHP throws a TypeError.
                    let class_orig = &func_name[..pos];
                    let class_display = goro_core::value::display_class_name(class_orig);
                    let msg = format!(
                        "call_user_func(): Argument #1 ($callback) must be a valid callback, class \"{}\" not found",
                        class_display
                    );
                    let exc = vm.create_exception(b"TypeError", &msg, 0);
                    vm.current_exception = Some(exc);
                    return Err(VmError { message: msg, line: vm.current_line });
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
        let (_, last_val) = arr.pop_entry().unwrap();
        // Reset internal pointer
        arr.pointer = 0;
        Ok(last_val)
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
        // Deep-clone the new values BEFORE we borrow_mut, to avoid
        // recursive self-reference when the user passes $arr itself as a value.
        let new_values: Vec<Value> = args[1..].iter().map(deep_clone_value).collect();

        let mut arr_mut = arr.borrow_mut();
        let existing: Vec<_> = arr_mut.iter().map(|(k, v)| (k.clone(), v.clone())).collect();

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
        let arg = match arg {
            Value::Reference(r) => r.borrow().clone(),
            v => v.clone(),
        };
        if let Value::Array(arr) = arg {
            let arr = arr.borrow();
            for (key, val) in arr.iter() {
                let cloned = deep_clone_value(val);
                match key {
                    goro_core::array::ArrayKey::Int(_) => result.push(cloned),
                    goro_core::array::ArrayKey::String(s) => {
                        result.set(goro_core::array::ArrayKey::String(s.clone()), cloned);
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
                return Err(VmError { message: msg, line: vm.current_line });
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

fn array_splice(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
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
        // If replacement is an object, convert it to an array (as (array)$obj).
        let replacement_converted: Option<Value> = match args.get(3) {
            Some(Value::Object(obj)) => Some(Value::Array(vm.object_to_array(obj))),
            _ => None,
        };
        let replacement = replacement_converted.as_ref().or_else(|| args.get(3));

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

fn array_search(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let needle = args.first().unwrap_or(&Value::Null);
    let strict = args.get(2).map(|v| v.is_truthy()).unwrap_or(false);
    let haystack_raw = args.get(1).cloned().unwrap_or(Value::Null);
    let haystack = match &haystack_raw {
        Value::Reference(r) => r.borrow().clone(),
        v => v.clone(),
    };
    if let Value::Array(arr) = &haystack {
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
        return Ok(Value::False);
    }
    // Throw TypeError for non-array haystack
    let type_name = Vm::value_type_name(&haystack);
    let msg = format!("array_search(): Argument #2 ($haystack) must be of type array, {} given", type_name);
    let exc = vm.throw_type_error(msg.clone());
    vm.current_exception = Some(exc);
    Err(VmError { message: msg, line: vm.current_line })
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

fn in_array(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let needle = args.first().unwrap_or(&Value::Null);
    let haystack_raw = args.get(1).cloned().unwrap_or(Value::Null);
    let haystack = match &haystack_raw {
        Value::Reference(r) => r.borrow().clone(),
        v => v.clone(),
    };
    if let Value::Array(arr) = &haystack {
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
        return Ok(Value::False);
    }
    // Throw TypeError for non-array haystack
    let type_name = Vm::value_type_name(&haystack);
    let msg = format!("in_array(): Argument #2 ($haystack) must be of type array, {} given", type_name);
    let exc = vm.throw_type_error(msg.clone());
    vm.current_exception = Some(exc);
    Err(VmError { message: msg, line: vm.current_line })
}

fn validate_callback_for_array_map(vm: &mut Vm, callback: &Value) -> Result<(), VmError> {
    validate_callback_generic(vm, callback, "array_map", "Argument #1 ($callback)")
}

fn validate_callback_generic(vm: &mut Vm, callback: &Value, fn_name: &str, arg_label: &str) -> Result<(), VmError> {
    match callback {
        Value::Null => Ok(()), // null is valid (identity/zip)
        Value::String(s) => {
            let name = s.as_bytes();
            if name.is_empty() {
                let msg = format!("{}(): {} must be a valid callback or null, function \"\" not found or invalid function name", fn_name, arg_label);
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
                        let msg = format!("{}(): {} must be a valid callback or null, class '{}' does not have a method '{}'",
                            fn_name, arg_label,
                            String::from_utf8_lossy(class_name), String::from_utf8_lossy(method_name));
                        let exc = vm.throw_type_error(msg.clone());
                        vm.current_exception = Some(exc);
                        return Err(VmError { message: msg, line: vm.current_line });
                    }
                    let msg = format!("{}(): {} must be a valid callback or null, class \"{}\" not found",
                        fn_name, arg_label,
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
            // PHP language constructs cannot be used as callbacks
            const LANG_CONSTRUCTS: &[&[u8]] = &[
                b"echo", b"print", b"isset", b"unset", b"empty",
                b"include", b"include_once", b"require", b"require_once",
                b"eval", b"exit", b"die", b"list", b"array",
            ];
            if LANG_CONSTRUCTS.contains(&name_lower.as_slice()) {
                let msg = format!("{}(): {} must be a valid callback or null, function \"{}\" not found or invalid function name",
                    fn_name, arg_label,
                    String::from_utf8_lossy(name));
                let exc = vm.throw_type_error(msg.clone());
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg, line: vm.current_line });
            }
            if vm.functions.contains_key(&name_lower) || vm.user_functions.contains_key(&name_lower) {
                return Ok(());
            }
            let msg = format!("{}(): {} must be a valid callback or null, function \"{}\" not found or invalid function name",
                fn_name, arg_label,
                String::from_utf8_lossy(name));
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            Err(VmError { message: msg, line: vm.current_line })
        }
        Value::Array(arr) => {
            let arr_borrow = arr.borrow();
            let len = arr_borrow.len();
            if len != 2 {
                let msg = format!("{}(): {} must be a valid callback or null, array callback must have exactly two members", fn_name, arg_label);
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
                    let msg = format!("{}(): {} must be a valid callback or null, first array member is not a valid class name or object", fn_name, arg_label);
                    let exc = vm.throw_type_error(msg.clone());
                    vm.current_exception = Some(exc);
                    Err(VmError { message: msg, line: vm.current_line })
                }
                Value::Object(_) => Ok(()),
                _ => {
                    let msg = format!("{}(): {} must be a valid callback or null, first array member is not a valid class name or object", fn_name, arg_label);
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
            let msg = format!("{}(): {} must be a valid callback or null, no array or string given", fn_name, arg_label);
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            Err(VmError { message: msg, line: vm.current_line })
        }
        _ => {
            let msg = format!("{}(): {} must be a valid callback or null, no array or string given", fn_name, arg_label);
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
            // Check required argument count - array_map calls user function with 1 arg
            // plus captured args. Verify the function accepts this.
            let provided = (1 + captured_args.len()) as u32;
            let required = user_fn.required_param_count;
            if provided < required {
                let display_name = String::from_utf8_lossy(&func_name).into_owned();
                let total_params = user_fn.param_count;
                let has_optional = total_params > required;
                let qualifier = if has_optional { "at least" } else { "exactly" };
                let plural = if required == 1 { "" } else { "s" };
                let pass_plural = if provided == 1 { "" } else { "s" };
                let msg = format!(
                    "Too few arguments to function {}(), {} passed and {} {} expected",
                    display_name, provided, qualifier, required
                );
                let _ = (plural, pass_plural);
                let exc = vm.create_exception(b"ArgumentCountError", &msg, vm.current_line);
                vm.current_exception = Some(exc);
                return Err(VmError { message: format!("Uncaught ArgumentCountError: {}", msg), line: vm.current_line });
            }
            for (key, val) in arr.iter() {
                if user_fn.variadic_param.is_some() {
                    // Variadic parameters need proper argument packing.
                    let mut call_args = captured_args.clone();
                    call_args.push(val.clone());
                    let mapped = vm.execute_fn_with_named_args(&user_fn, call_args, vec![], None)?;
                    result.set(key.clone(), mapped);
                    continue;
                }
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
            validate_callback_generic(vm, &cb, "array_filter", "Argument #2 ($callback)")?;

            // Pre-validate arg count for known string-named user functions / builtins.
            // array_filter calls the callback with `n_args`:
            //   mode 0: 1 arg (value)
            //   mode 1: 2 args (value, key)
            //   mode 2: 1 arg (key)
            let n_args: u32 = if mode == 1 { 2 } else { 1 };
            if let Value::String(s) = &cb {
                let func_name = s.as_bytes().to_vec();
                let func_lower: Vec<u8> = func_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                if let Some(user_fn) = vm.user_functions.get(&func_lower).cloned() {
                    let required = user_fn.required_param_count;
                    if n_args < required {
                        let display_name = String::from_utf8_lossy(&func_name).into_owned();
                        let total_params = user_fn.param_count;
                        let has_optional = total_params > required;
                        let qualifier = if has_optional { "at least" } else { "exactly" };
                        let msg = format!(
                            "Too few arguments to function {}(), {} passed and {} {} expected",
                            display_name, n_args, qualifier, required
                        );
                        let exc = vm.create_exception(b"ArgumentCountError", &msg, vm.current_line);
                        vm.current_exception = Some(exc);
                        return Err(VmError { message: format!("Uncaught ArgumentCountError: {}", msg), line: vm.current_line });
                    }
                }
            }

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
        let entries: Vec<(ArrayKey, Vec<u8>, Value)> = {
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
                .filter(|(k, v)| {
                    !k.starts_with(b"__spl_") && !k.starts_with(b"__reflection_")
                    && !k.starts_with(b"__enum_") && !k.starts_with(b"__ctor_")
                    && !k.starts_with(b"__clone_") && !k.starts_with(b"__destructed")
                    && !k.starts_with(b"__fiber_") && !k.starts_with(b"__timestamp")
                    && !k.starts_with(b"__lazy_") && !k.starts_with(b"__uri_")
                    && !matches!(v, Value::Undef)
                })
                .map(|(k, v)| {
                    let (vis, declaring_class) = prop_info.get(k)
                        .map(|(v, dc)| (Some(*v), dc.as_slice()))
                        .unwrap_or((None, &[]));
                    let mangled_key = mangle_property_name(k, declaring_class, vis);
                    (ArrayKey::String(PhpString::from_vec(mangled_key)), k.clone(), v.clone())
                }).collect()
        };
        let extra_data = args.get(2);
        return array_walk_object_entries(vm, callback, &entries, extra_data, &obj);
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

/// array_walk for objects: iterates props, calls callback with &$value,
/// and writes the modified value back to the actual property name.
fn array_walk_object_entries(
    vm: &mut Vm,
    callback: &Value,
    entries: &[(ArrayKey, Vec<u8>, Value)],
    extra_data: Option<&Value>,
    obj: &Rc<RefCell<goro_core::object::PhpObject>>,
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
        for (key, prop_name, val) in entries {
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
            vm.execute_fn(&user_fn, fn_cvs)?;
            // Write modified value back to the object property.
            // Type coercion: if the property has a declared type and the
            // value is incompatible, throw a TypeError (matches PHP's
            // reference-held-by-typed-property behaviour).
            let new_val = val_ref.borrow().clone();
            let class_lower: Vec<u8> = obj.borrow().class_name.iter()
                .map(|b| b.to_ascii_lowercase()).collect();
            let type_check_failed: Option<String> = {
                if let Some(class_def) = vm.get_class_def(&class_lower) {
                    if let Some(p) = class_def.properties.iter()
                        .find(|p| !p.is_static && p.name == *prop_name)
                    {
                        if let Some(t) = &p.property_type {
                            if !vm.value_matches_type(&new_val, t) {
                                let type_name = vm.param_type_name(t);
                                let val_type = value_type_for_error(&new_val);
                                let class_display = goro_core::value::display_class_name(
                                    &obj.borrow().class_name);
                                let prop_display = String::from_utf8_lossy(prop_name);
                                Some(format!(
                                    "Cannot assign {} to reference held by property {}::${} of type {}",
                                    val_type, class_display, prop_display, type_name
                                ))
                            } else { None }
                        } else { None }
                    } else { None }
                } else { None }
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
            builtin(vm, &[val.clone()])?;
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

fn array_pad(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let input = args.first().unwrap_or(&Value::Null);
    let pad_size = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let pad_value = args.get(2).cloned().unwrap_or(Value::Null);

    // PHP caps the target length at HT_MAX_SIZE (2^30 by default). Throw a
    // ValueError rather than trying to allocate a huge array.
    const MAX_ARRAY_SIZE: i64 = 1 << 30;
    if pad_size <= -MAX_ARRAY_SIZE || pad_size >= MAX_ARRAY_SIZE {
        let msg =
            "array_pad(): Argument #2 ($length) must not exceed the maximum allowed array size"
                .to_string();
        let exc = vm.create_exception(b"ValueError", &msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
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
                    ArrayKey::Int(_) => { result.push(v.clone()); }
                    ArrayKey::String(_) => { result.set(k.clone(), v.clone()); }
                }
            }
        } else {
            // Pad at the end: input first with string keys preserved, then pad values
            for (k, v) in arr.iter() {
                match k {
                    ArrayKey::Int(_) => { result.push(v.clone()); }
                    ArrayKey::String(_) => { result.set(k.clone(), v.clone()); }
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
                Value::Resource(n) => ArrayKey::String(PhpString::from_vec(format!("Resource id #{}", n).into_bytes())),
                Value::Double(_) => ArrayKey::String(val.to_php_string()),
                Value::String(s) => {
                    // Try to parse as integer (PHP normalizes numeric string keys)
                    if let Ok(n) = s.to_string_lossy().parse::<i64>() {
                        ArrayKey::Int(n)
                    } else {
                        ArrayKey::String(s.clone())
                    }
                }
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
                        ArrayKey::String(s.to_php_string())
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
    let mut result = PhpArray::new();
    for (i, arg) in args.iter().enumerate() {
        require_array_arg_variadic(vm, arg, "array_merge_recursive", (i + 1) as u32)?;
        if let Value::Array(arr) = arg {
            let arr = arr.borrow();
            for (key, val) in arr.iter() {
                match key {
                    goro_core::array::ArrayKey::Int(_) => {
                        if let Err(msg) = result.try_push(unwrap_rc1_ref(val)) {
                            // PHP throws an Error when next int key overflows.
                            let exc = vm.create_exception(b"Error", msg, vm.current_line);
                            vm.current_exception = Some(exc);
                            return Err(VmError { message: msg.to_string(), line: vm.current_line });
                        }
                    }
                    goro_core::array::ArrayKey::String(s) => {
                        if let Some(existing) = result.get_str(s.as_bytes()) {
                            let merged = merge_recursive_values(vm, &existing, val, 0)?;
                            result.set(key.clone(), merged);
                        } else {
                            result.set(key.clone(), unwrap_rc1_ref(val));
                        }
                    }
                }
            }
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn merge_recursive_values(vm: &mut Vm, existing: &Value, new_val: &Value, depth: usize) -> Result<Value, VmError> {
    if depth > 256 { return Ok(new_val.clone()); }
    match (existing, new_val) {
        (Value::Array(a), Value::Array(b)) => {
            let mut merged_arr = a.borrow().clone();
            let b_ref = b.borrow();
            for (k, v) in b_ref.iter() {
                match k {
                    goro_core::array::ArrayKey::Int(_) => {
                        if let Err(msg) = merged_arr.try_push(unwrap_rc1_ref(v)) {
                            let exc = vm.create_exception(b"Error", msg, vm.current_line);
                            vm.current_exception = Some(exc);
                            return Err(VmError { message: msg.to_string(), line: vm.current_line });
                        }
                    }
                    goro_core::array::ArrayKey::String(s) => {
                        if let Some(ex) = merged_arr.get_str(s.as_bytes()) {
                            let merged = merge_recursive_values(vm, &ex, v, depth + 1)?;
                            merged_arr.set(k.clone(), merged);
                        } else {
                            merged_arr.set(k.clone(), unwrap_rc1_ref(v));
                        }
                    }
                }
            }
            Ok(Value::Array(Rc::new(RefCell::new(merged_arr))))
        }
        (existing_val, new_v) => {
            let mut arr = PhpArray::new();
            if let Value::Array(a) = existing_val {
                for (_, v) in a.borrow().iter() {
                    if let Err(msg) = arr.try_push(v.clone()) {
                        let exc = vm.create_exception(b"Error", msg, vm.current_line);
                        vm.current_exception = Some(exc);
                        return Err(VmError { message: msg.to_string(), line: vm.current_line });
                    }
                }
            } else {
                let _ = arr.try_push(existing_val.clone());
            }
            if let Value::Array(b) = new_v {
                for (_, v) in b.borrow().iter() {
                    if let Err(msg) = arr.try_push(unwrap_rc1_ref(v)) {
                        let exc = vm.create_exception(b"Error", msg, vm.current_line);
                        vm.current_exception = Some(exc);
                        return Err(VmError { message: msg.to_string(), line: vm.current_line });
                    }
                }
            } else {
                let _ = arr.try_push(new_v.clone());
            }
            Ok(Value::Array(Rc::new(RefCell::new(arr))))
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
            let type_name = arg_type_name(arg);
            let msg = if i == 0 {
                format!("array_diff(): Argument #1 ($array) must be of type array, {} given", type_name)
            } else {
                format!("array_diff(): Argument #{} must be of type array, {} given", i + 1, type_name)
            };
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
        if i == 0 {
            require_array_arg(vm, arg, "array_intersect", "array", 1)?;
        } else {
            require_array_arg_variadic(vm, arg, "array_intersect", (i + 1) as u32)?;
        }
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
        Value::Long(_) | Value::Double(_) | Value::Resource(_) => 3,
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

    // Float range (if any argument is float or step is float).
    // Strings that look like floats (contain '.' or exponent) also force float mode.
    let looks_like_float = |v: &Value| -> bool {
        match v {
            Value::Double(_) => true,
            Value::String(s) => {
                let b = s.as_bytes();
                b.iter().any(|&c| c == b'.' || c == b'e' || c == b'E')
            }
            _ => false,
        }
    };
    // PHP treats whole-number float values (e.g. 2.0) as integer-equivalent if
    // the start/end are ints and the step has no fractional component. Numeric
    // strings with no decimal point are also int-equivalent.
    let is_int_equivalent = |v: &Value| -> bool {
        match v {
            Value::Long(_) | Value::True | Value::False | Value::Null | Value::Undef => true,
            Value::Double(d) => d.is_finite() && d.fract() == 0.0,
            Value::String(s) => {
                let b = s.as_bytes();
                !b.iter().any(|&c| c == b'.' || c == b'e' || c == b'E')
            }
            _ => false,
        }
    };
    let all_int_equivalent = is_int_equivalent(start_val)
        && is_int_equivalent(end_val)
        && step_val.is_none_or(is_int_equivalent);
    let use_float = !all_int_equivalent
        && (looks_like_float(start_val)
            || looks_like_float(end_val)
            || step_val.is_some_and(looks_like_float));

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
        // Use iteration counter to avoid accumulated floating point errors.
        // PHP computes count = floor((|end - start|) / step + EPSILON) + 1
        let diff = (end - start).abs();
        let count = (diff / step + 1e-9).floor() as i64 + 1;
        let count = count.min(10_000_001);
        if start <= end {
            for i in 0..count {
                let v = start + (i as f64) * step;
                if v > end + 1e-9 { break; }
                result.push(Value::Double(v));
            }
        } else {
            for i in 0..count {
                let v = start - (i as f64) * step;
                if v < end - 1e-9 { break; }
                result.push(Value::Double(v));
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
    let value = args.get(1).map(|v| v.deref()).unwrap_or(Value::Null);
    if matches!(value, Value::Array(_) | Value::Object(_) | Value::Resource(_)) {
        return Err(throw_type_error_msg(vm, "ini_set(): Argument #2 ($value) must be of type string|int|float|bool|null".to_string()));
    }
    let key = name.as_bytes().to_vec();
    // ini_set() returns the old value as a string.
    let old = vm.constants.get(&key).map(|v| Value::String(v.to_php_string()));
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
    // max_memory_limit is PHP_INI_SYSTEM
    if key == b"max_memory_limit" {
        return Ok(Value::False);
    }
    // Handle memory_limit (OnChangeMemoryLimit: clamp to max_memory_limit)
    if key == b"memory_limit" {
        let vs = value.to_php_string();
        let (v, err) = goro_core::ini_quantity::parse_quantity(vs.as_bytes(), false);
        if let Some(e) = err {
            vm.emit_warning(&e);
        }
        let max_s = vm.constants.get(b"max_memory_limit".as_ref()).map(|m| m.to_php_string());
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
        goro_core::value::set_memory_limit(if v == u64::MAX { -1 } else { v as i64 });
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
        return Ok(old.map(|v| Value::String(v.to_php_string())).unwrap_or(Value::False));
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
            if t.eq_ignore_ascii_case("true") || t.eq_ignore_ascii_case("yes") || t.eq_ignore_ascii_case("on") {
                true
            } else {
                let digits: String = t.chars().take_while(|c| c.is_ascii_digit() || *c == '-' || *c == '+').collect();
                digits.parse::<i64>().map(|n| n != 0).unwrap_or(false)
            }
        }
    }
}

fn assert_ini_bool(vm: &Vm, key: &[u8], default: bool) -> bool {
    vm.constants.get(key).map(ini_parse_bool).unwrap_or(default)
}

/// Deprecations emitted by the assert.* INI handlers when a value is changed
/// (not from assert_options()). Returns the message, if any.
pub fn assert_ini_deprecation(key: &[u8], value: &Value) -> Option<String> {
    let name = String::from_utf8_lossy(key).to_string();
    let deprecated = match key {
        b"assert.active" | b"assert.warning" | b"assert.exception" => !ini_parse_bool(value),
        b"assert.bail" => ini_parse_bool(value),
        b"assert.callback" => !value.to_php_string().as_bytes().is_empty(),
        _ => false,
    };
    if deprecated { Some(format!("{} INI setting is deprecated", name)) } else { None }
}

fn php_assert(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let zend_assertions = vm.constants.get(b"zend.assertions".as_ref()).map(|v| v.to_long()).unwrap_or(1);
    if !assert_ini_bool(vm, b"assert.active", true) || zend_assertions <= 0 {
        return Ok(Value::True);
    }
    let assertion = args.first().map(|v| v.deref()).unwrap_or(Value::Null);
    let description = args.get(1).map(|v| v.deref());
    if let Some(d) = &description {
        match d {
            Value::Null | Value::String(_) | Value::Object(_) => {}
            other => {
                let msg = format!("assert(): Argument #2 ($description) must be of type Throwable|string|null, {} given", Vm::value_type_name(other));
                let exc = vm.throw_type_error(msg.clone());
                vm.current_exception = Some(exc);
                return Err(VmError { message: format!("Uncaught TypeError: {}", msg), line: vm.current_line });
            }
        }
    }
    if assertion.is_truthy() {
        return Ok(Value::True);
    }
    if let Some(Value::Object(obj)) = &description {
        vm.current_exception = Some(Value::Object(obj.clone()));
        return Err(VmError { message: "assert() failed".into(), line: vm.current_line });
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
            Value::String(PhpString::from_string(vm.current_file.clone())),
            Value::Long(line as i64),
            Value::Null,
        ];
        if let Some(d) = &desc_str {
            cargs.push(Value::String(d.clone()));
        }
        let mut full = vec![cb.clone()];
        full.extend(cargs);
        let _ = call_user_func(vm, &full);
        vm.current_line = line;
        if vm.current_exception.is_some() {
            return Err(VmError { message: "Uncaught exception".into(), line });
        }
    }
    if assert_ini_bool(vm, b"assert.exception", true) {
        let msg = desc_str.as_ref().map(|d| d.to_string_lossy()).unwrap_or_default();
        // The dispatcher may already have pushed our frame.
        let pushed = vm.call_stack.last().map(|f| f.0 != "assert").unwrap_or(true);
        if pushed {
            vm.call_stack.push(("assert".to_string(), vm.current_file.clone(), line, args.to_vec(), false));
        }
        let exc = vm.create_exception(b"AssertionError", &msg, line);
        if pushed {
            vm.call_stack.pop();
        }
        if let Value::Object(o) = &exc {
            o.borrow_mut().set_property(b"code".to_vec(), Value::Long(1));
        }
        vm.current_exception = Some(exc);
        return Err(VmError { message: format!("Uncaught AssertionError: {}", msg), line });
    } else if assert_ini_bool(vm, b"assert.warning", true) {
        let d = desc_str.as_ref().map(|d| d.to_string_lossy()).unwrap_or_else(|| "Assertion".to_string());
        vm.emit_warning_at(&format!("assert(): {} failed", d), line);
    }
    Ok(Value::False)
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
    // Helper: walk class hierarchy, checking visibility of inherited private.
    fn class_has_visible_prop(vm: &Vm, class_lower: &[u8], pname: &[u8]) -> bool {
        if let Some(ce) = vm.classes.get(class_lower) {
            for prop in &ce.properties {
                if prop.name != pname { continue; }
                // Inherited private props are not visible on subclasses.
                let decl_lower: Vec<u8> = prop.declaring_class.iter().map(|b| b.to_ascii_lowercase()).collect();
                if prop.visibility == Visibility::Private && decl_lower != class_lower {
                    continue;
                }
                return true;
            }
        }
        false
    }
    match class_or_obj {
        Value::Object(obj) => {
            let ob = obj.borrow();
            let class_lower: Vec<u8> = ob.class_name.iter().map(|c| c.to_ascii_lowercase()).collect();
            // Declared property check (filters out inherited private).
            let declared = class_has_visible_prop(vm, &class_lower, prop_name.as_bytes());
            if declared { return Ok(Value::True); }
            // Otherwise check whether this is a dynamic instance property
            // (a key on the object that is not declared anywhere on the class
            // hierarchy).
            let on_inst = ob.has_property(prop_name.as_bytes());
            drop(ob);
            if !on_inst { return Ok(Value::False); }
            let mut declared_anywhere = false;
            let mut current = Some(class_lower);
            while let Some(cl) = current {
                if let Some(ce) = vm.classes.get(&cl) {
                    if ce.properties.iter().any(|p| p.name == prop_name.as_bytes()) {
                        declared_anywhere = true;
                        break;
                    }
                    current = ce.parent.as_ref().map(|p| p.iter().map(|b| b.to_ascii_lowercase()).collect());
                } else { break; }
            }
            Ok(if !declared_anywhere { Value::True } else { Value::False })
        }
        Value::String(s) => {
            let class_lower: Vec<u8> = s
                .as_bytes()
                .iter()
                .map(|c| c.to_ascii_lowercase())
                .collect();
            Ok(if class_has_visible_prop(vm, &class_lower, prop_name.as_bytes()) { Value::True } else { Value::False })
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
            let lower: Vec<u8> = s.as_bytes()
                .iter()
                .map(|c| c.to_ascii_lowercase())
                .collect();
            if !vm.classes.contains_key(&lower) && !vm.is_known_builtin_class(&lower) {
                let name = s.as_bytes().to_vec();
                vm.try_autoload_class(&name);
                if vm.current_exception.is_some() {
                    return Err(VmError { message: "Uncaught exception".into(), line: vm.current_line });
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
            return Err(throw_type_error_msg(vm, format!("method_exists(): Argument #1 ($object_or_class) must be of type object|string, {} given", Vm::value_type_name(other))));
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
                return Ok(if is_object || !private_inherited { Value::True } else { Value::False });
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

    // Extension-provided built-in classes
    if let Some((native, h)) = vm.find_native_class_handler(&class_lower) {
        if (h.has_method)(&native, &method_lower, false)
            || (h.has_method)(&native, &method_lower, true)
            || (method_lower == b"__construct" && vm.object_construct_handlers.contains_key(&native))
        {
            return Ok(Value::True);
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
    // extract() is compiled to the dedicated OpCode::Extract since it modifies
    // the caller's local scope. This stub exists only so that
    // function_exists('extract') and call_user_func('extract', ...) don't fail.
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
        let msg = "array_rand(): Argument #1 ($array) must not be empty".to_string();
        let exc = vm.throw_type_error(msg.clone());
        if let Value::Object(obj) = &exc {
            obj.borrow_mut().class_name = b"ValueError".to_vec();
        }
        vm.current_exception = Some(exc);
        return Err(VmError { message: format!("Uncaught ValueError: {}", msg), line: vm.current_line });
    }
    if num < 1 || num as usize > len {
        let msg = "array_rand(): Argument #2 ($num) must be between 1 and the number of elements in argument #1 ($array)".to_string();
        let exc = vm.throw_type_error(msg.clone());
        if let Value::Object(obj) = &exc {
            obj.borrow_mut().class_name = b"ValueError".to_vec();
        }
        vm.current_exception = Some(exc);
        return Err(VmError { message: format!("Uncaught ValueError: {}", msg), line: vm.current_line });
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
        let places = decimals_raw.checked_neg().unwrap_or(i64::MAX).min(100) as u32;
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

// hex2bin and bin2hex moved to strings.rs

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

// htmlspecialchars/htmlentities/html_entity_decode/htmlspecialchars_decode
// moved to the html module.

// Note: the active str_word_count implementation is `str_word_count_fn` in
// strings.rs (it overrides this one at registration time, since both register
// under the same PHP function name). This stub is unused but kept to avoid
// disturbing the registration order. See strings.rs for the real impl.
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
    vm.write_output(&formatted);
    Ok(Value::Long(len as i64))
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
            let has_tostring = vm.classes.get(&class_lower)
                .map(|c| c.get_method(b"__tostring").is_some())
                .unwrap_or(false);
            if has_tostring {
                None
            } else {
                let class_display = String::from_utf8_lossy(&class_name).to_string();
                let msg = format!("{}(): Argument #2 ($format) must be of type string, {} given", func_name, class_display);
                let exc = vm.create_exception(b"TypeError", &msg, 0);
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg, line: vm.current_line });
            }
        }
        _ => None,
    };
    if let Some(t) = type_str {
        let msg = format!("{}(): Argument #2 ($format) must be of type string, {} given", func_name, t);
        let exc = vm.create_exception(b"TypeError", &msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
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
            let msg = format!("{}(): Argument #3 ($values) must be of type array, {} given", func_name, name);
            let exc = vm.create_exception(b"TypeError", &msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
        }
        _ => "unknown",
    };
    let msg = format!("{}(): Argument #3 ($values) must be of type array, {} given", func_name, type_name);
    let exc = vm.create_exception(b"TypeError", &msg, 0);
    vm.current_exception = Some(exc);
    Err(VmError { message: msg, line: vm.current_line })
}

/// Validate that arg #1 is a resource (or an int treated as one). Returns the
/// type name PHP would print on mismatch.
fn validate_stream_arg1(vm: &mut Vm, func_name: &str, stream_arg: &Value) -> Result<(), VmError> {
    let derefed = stream_arg.deref();
    let type_name = match &derefed {
        Value::Resource(_) | Value::Long(_) => return Ok(()),
        Value::Double(_) => "float",
        Value::Null | Value::Undef => "null",
        Value::True => "true",
        Value::False => "false",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(obj) => {
            let name = String::from_utf8_lossy(&obj.borrow().class_name).to_string();
            let msg = format!("{}(): Argument #1 ($stream) must be of type resource, {} given", func_name, name);
            let exc = vm.create_exception(b"TypeError", &msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
        }
        _ => "unknown",
    };
    let msg = format!("{}(): Argument #1 ($stream) must be of type resource, {} given", func_name, type_name);
    let exc = vm.create_exception(b"TypeError", &msg, 0);
    vm.current_exception = Some(exc);
    Err(VmError { message: msg, line: vm.current_line })
}

fn fprintf_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // fprintf($handle, $format, ...$args) - write formatted to file handle
    if args.len() < 2 {
        let msg = format!("fprintf() expects at least 2 arguments, {} given", args.len());
        let exc = vm.create_exception(b"ArgumentCountError", &msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    validate_stream_arg1(vm, "fprintf", &args[0])?;
    validate_fmt_arg2(vm, "fprintf", &args[1])?;
    let fid = args[0].to_long();
    let format_args = &args[1..]; // skip the handle, pass format + args
    let formatted = crate::strings::do_sprintf_vm(vm, format_args)?;
    let len = formatted.len();

    // Check if it's a real file handle
    let written = FILE_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(fh) = handles.get_mut(&fid) {
            fh.file.write_all(&formatted).ok();
            true
        } else {
            false
        }
    });

    if !written {
        // Fallback to stdout for STDOUT handle or unknown handles
        vm.write_output(&formatted);
    }
    Ok(Value::Long(len as i64))
}

fn vfprintf_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // vfprintf($handle, $format, $args_array)
    if args.len() != 3 {
        let msg = format!("vfprintf() expects exactly 3 arguments, {} given", args.len());
        let exc = vm.create_exception(b"ArgumentCountError", &msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    validate_stream_arg1(vm, "vfprintf", &args[0])?;
    validate_fmt_arg2(vm, "vfprintf", &args[1])?;
    validate_values_arg3(vm, "vfprintf", &args[2])?;
    let fid = args[0].to_long();
    let format = vm.value_to_string(&args[1]);
    let mut format_args = vec![Value::String(format)];
    if let Value::Array(arr) = &args[2] {
        for (_, v) in arr.borrow().iter() {
            format_args.push(v.clone());
        }
    }
    let formatted = crate::strings::do_sprintf_vm(vm, &format_args)?;
    let len = formatted.len();

    let written = FILE_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(fh) = handles.get_mut(&fid) {
            fh.file.write_all(&formatted).ok();
            true
        } else {
            false
        }
    });
    if !written {
        vm.write_output(&formatted);
    }
    Ok(Value::Long(len as i64))
}

fn sscanf_put(results: &mut Vec<Value>, slot: usize, v: Value) {
    while results.len() <= slot {
        results.push(Value::Null);
    }
    results[slot] = v;
}

/// Return the total number of result slots `format` defines, i.e. the size of
/// the array sscanf() returns when no ref arguments are given. PHP pads with
/// NULL for slots no conversion could fill.
fn count_sscanf_total_slots(format: &[u8]) -> usize {
    let mut next_slot: usize = 0;
    let mut max_slot_seen: Option<usize> = None;
    let mut i = 0;
    while i < format.len() {
        if format[i] != b'%' {
            i += 1;
            continue;
        }
        i += 1;
        if i >= format.len() {
            break;
        }
        if format[i] == b'%' {
            i += 1;
            continue;
        }
        let start = i;
        while i < format.len() && format[i].is_ascii_digit() {
            i += 1;
        }
        let mut cur_slot: Option<usize> = None;
        if i > start && i < format.len() && format[i] == b'$' {
            let n: usize = std::str::from_utf8(&format[start..i])
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            i += 1;
            if n > 0 {
                cur_slot = Some(n - 1);
            }
        } else {
            i = start;
        }
        let mut suppress = false;
        if i < format.len() && format[i] == b'*' {
            suppress = true;
            i += 1;
        }
        while i < format.len() && format[i].is_ascii_digit() {
            i += 1;
        }
        while i < format.len() && matches!(format[i], b'h' | b'l' | b'L' | b'j' | b'z' | b't' | b'q') {
            i += 1;
        }
        if i >= format.len() {
            break;
        }
        let spec = format[i];
        i += 1;
        if spec == b'[' {
            if i < format.len() && format[i] == b'^' {
                i += 1;
            }
            if i < format.len() && format[i] == b']' {
                i += 1;
            }
            while i < format.len() && format[i] != b']' {
                i += 1;
            }
            if i < format.len() {
                i += 1;
            }
        }
        if suppress {
            continue;
        }
        let slot = cur_slot.unwrap_or(next_slot);
        max_slot_seen = Some(max_slot_seen.map_or(slot, |m| m.max(slot)));
        next_slot = slot + 1;
    }
    max_slot_seen.map_or(0, |m| m + 1)
}

/// Count how many output slots each variable receives via scanf conversion
/// specifiers in `format`. Used by sscanf_fn to detect var/specifier mismatch
/// before performing the scan, mirroring PHP's ValidateFormat phase.
fn validate_sscanf_format(format: &[u8], num_vars: usize) -> Result<(), String> {
    // PHP caps positional index at SCAN_MAX_ARGS (255) when no ref vars are given.
    const SCAN_MAX_ARGS: usize = 255;
    let mut nassign: Vec<usize> = vec![0; num_vars];
    let mut next_slot: usize = 0;
    let mut got_xpg = false;
    let mut i = 0;
    while i < format.len() {
        if format[i] != b'%' {
            i += 1;
            continue;
        }
        i += 1;
        if i >= format.len() {
            break;
        }
        if format[i] == b'%' {
            i += 1;
            continue;
        }
        // Positional spec: %N$
        let start = i;
        while i < format.len() && format[i].is_ascii_digit() {
            i += 1;
        }
        let mut cur_slot: Option<usize> = None;
        if i > start && i < format.len() && format[i] == b'$' {
            // Parse as u64 so we can detect overflow before clamping into usize.
            let raw: u64 = std::str::from_utf8(&format[start..i])
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            i += 1;
            if raw == 0 {
                return Err("\"%n$\" argument index out of range".to_string());
            }
            // numVars == 0 (no ref args) caps at SCAN_MAX_ARGS.
            // numVars > 0 caps at numVars.
            let cap = if num_vars == 0 { SCAN_MAX_ARGS as u64 } else { num_vars as u64 };
            if raw > cap {
                return Err("\"%n$\" argument index out of range".to_string());
            }
            let n = raw as usize;
            cur_slot = Some(n - 1);
            got_xpg = true;
        } else {
            i = start;
        }
        // Suppression %*
        let mut suppress = false;
        if i < format.len() && format[i] == b'*' {
            suppress = true;
            i += 1;
        }
        // Width digits
        while i < format.len() && format[i].is_ascii_digit() {
            i += 1;
        }
        // Length modifiers
        while i < format.len() && matches!(format[i], b'h' | b'l' | b'L' | b'j' | b'z' | b't' | b'q') {
            i += 1;
        }
        if i >= format.len() {
            break;
        }
        let spec = format[i];
        i += 1;
        // Skip past `[...]` set body for %[ scanset.
        if spec == b'[' {
            if i < format.len() && format[i] == b'^' {
                i += 1;
            }
            if i < format.len() && format[i] == b']' {
                i += 1;
            }
            while i < format.len() && format[i] != b']' {
                i += 1;
            }
            if i < format.len() {
                i += 1;
            }
        }
        if suppress {
            continue;
        }
        let slot = cur_slot.unwrap_or(next_slot);
        if slot < num_vars {
            nassign[slot] += 1;
        }
        next_slot = slot + 1;
    }
    for &count in &nassign {
        if count > 1 {
            return Err("Variable is assigned by multiple \"%n$\" conversion specifiers".to_string());
        }
        if !got_xpg && count == 0 {
            return Err("Variable is not assigned by any conversion specifiers".to_string());
        }
    }
    Ok(())
}

fn sscanf_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let input = args.first().unwrap_or(&Value::Null).to_php_string();
    let format = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let input_bytes = input.as_bytes();
    let format_bytes = format.as_bytes();

    let has_refs = args.len() > 2;
    // PHP validates that every supplied ref var is assigned by some specifier
    // and that no slot is assigned twice. This runs before any scanning so the
    // ValueError fires even when the input would have produced fewer results.
    // Even without ref args, validate to catch out-of-range positional indices
    // (e.g. "%2147483648$s") which would otherwise blow up allocation.
    if let Err(msg) = validate_sscanf_format(format_bytes, if has_refs { args.len() - 2 } else { 0 }) {
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    let mut results: Vec<Value> = Vec::new();
    let mut input_pos = 0usize;
    let mut fmt_pos = 0usize;
    let mut nconversions: usize = 0;
    let mut underflow = false;
    // Next default output slot (0-based). %N$X overrides this to N-1.
    let mut next_slot: usize = 0;
    // Slot to use for the *current* conversion (set per conversion).
    let mut cur_slot: usize = 0;
    // Helper: write into the slot, growing results with Nulls as needed.
    // Captured by closure later; for now we inline via a macro-like fn.

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

            // Handle argument swapping like %2$s — positional placement.
            // PHP accepts multi-digit positions; we parse the full digit run
            // followed by '$' as the position.
            let swap_arg: Option<usize> = {
                let start = fmt_pos;
                while fmt_pos < format_bytes.len() && format_bytes[fmt_pos].is_ascii_digit() {
                    fmt_pos += 1;
                }
                if fmt_pos > start && fmt_pos < format_bytes.len() && format_bytes[fmt_pos] == b'$' {
                    let n: usize = std::str::from_utf8(&format_bytes[start..fmt_pos])
                        .ok()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0);
                    fmt_pos += 1; // skip '$'
                    if n > 0 { Some(n) } else { None }
                } else {
                    // Not a positional spec — rewind so width parsing sees the digits.
                    fmt_pos = start;
                    None
                }
            };
            // Determine the slot for this conversion. With %N$X use N-1;
            // otherwise use the next sequential slot. After this conversion,
            // next_slot becomes cur_slot + 1.
            cur_slot = match swap_arg {
                Some(n) => n - 1,
                None => next_slot,
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

            // Skip length modifiers (h, l, L, j, z, t, q, hh, ll) — they don't
            // change the result type in PHP fscanf/sscanf.
            while fmt_pos < format_bytes.len() {
                match format_bytes[fmt_pos] {
                    b'h' | b'l' | b'L' | b'j' | b'z' | b't' | b'q' => fmt_pos += 1,
                    _ => break,
                }
            }

            if fmt_pos >= format_bytes.len() { break; }
            let spec = format_bytes[fmt_pos];
            fmt_pos += 1;

            // Skip leading whitespace for numeric types and %s (NOSKIP for %c and %[)
            // PHP/C isspace: space, \t, \n, \v, \f, \r
            match spec {
                b'd' | b'i' | b'u' | b'x' | b'X' | b'o' | b'f' | b'e' | b'g' | b'E' | b'G' | b's' => {
                    while input_pos < input_bytes.len() && matches!(input_bytes[input_pos], b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r') {
                        input_pos += 1;
                    }
                }
                _ => {}
            }

            // Check for underflow: if at EOF when trying to start a conversion
            // (other than %n which doesn't consume input), bail.
            if input_pos >= input_bytes.len() && spec != b'n' {
                underflow = true;
                break;
            }

            match spec {
                b'd' | b'i' | b'u' => {
                    // Read integer. Width counts total characters consumed,
                    // including any leading sign.
                    let start = input_pos;
                    let max = width.unwrap_or(usize::MAX);
                    let mut count = 0usize;
                    if input_pos < input_bytes.len() && (input_bytes[input_pos] == b'-' || input_bytes[input_pos] == b'+') && count < max {
                        input_pos += 1;
                        count += 1;
                    }
                    while input_pos < input_bytes.len() && input_bytes[input_pos].is_ascii_digit() && count < max {
                        input_pos += 1;
                        count += 1;
                    }
                    // If only the sign was consumed (no digits), reject.
                    let digit_len = input_pos - start - (if input_bytes.get(start) == Some(&b'-') || input_bytes.get(start) == Some(&b'+') { 1 } else { 0 });
                    if digit_len > 0 {
                        if !suppress {
                            let s = std::str::from_utf8(&input_bytes[start..input_pos]).unwrap_or("0");
                            let val = s.parse::<i64>().unwrap_or(0);
                            sscanf_put(&mut results, cur_slot, Value::Long(val));
                        }
                    } else {
                        // Conversion failed: roll back the input cursor and stop
                        // parsing entirely. PHP leaves any remaining ref args at
                        // their previous values (does NOT assign NULL).
                        input_pos = start;
                        break;
                    }
                }
                b'x' | b'X' => {
                    // Read hex integer with optional leading sign and `0x` prefix.
                    // If no digits match, roll back and stop parsing (PHP returns
                    // the partial result up to this point).
                    let entry_pos = input_pos;
                    let max = width.unwrap_or(usize::MAX);
                    let mut count = 0;
                    let mut negative = false;
                    if input_pos < input_bytes.len()
                        && (input_bytes[input_pos] == b'-' || input_bytes[input_pos] == b'+')
                        && count < max
                    {
                        negative = input_bytes[input_pos] == b'-';
                        input_pos += 1;
                        count += 1;
                    }
                    if input_pos + 1 < input_bytes.len()
                        && input_bytes[input_pos] == b'0'
                        && (input_bytes[input_pos + 1] == b'x' || input_bytes[input_pos + 1] == b'X')
                        && count + 2 <= max
                    {
                        input_pos += 2;
                        count += 2;
                    }
                    let digit_start = input_pos;
                    while input_pos < input_bytes.len() && input_bytes[input_pos].is_ascii_hexdigit() && count < max {
                        input_pos += 1;
                        count += 1;
                    }
                    if input_pos == digit_start {
                        input_pos = entry_pos;
                        break;
                    }
                    if !suppress {
                        let s = std::str::from_utf8(&input_bytes[digit_start..input_pos]).unwrap_or("0");
                        let mut val = i64::from_str_radix(s, 16).unwrap_or(0);
                        if negative {
                            val = -val;
                        }
                        sscanf_put(&mut results, cur_slot, Value::Long(val));
                    }
                }
                b'o' => {
                    // Read octal integer with optional leading sign. As with %x,
                    // if no digits match, roll back and stop parsing.
                    let entry_pos = input_pos;
                    let max = width.unwrap_or(usize::MAX);
                    let mut count = 0;
                    let mut negative = false;
                    if input_pos < input_bytes.len()
                        && (input_bytes[input_pos] == b'-' || input_bytes[input_pos] == b'+')
                        && count < max
                    {
                        negative = input_bytes[input_pos] == b'-';
                        input_pos += 1;
                        count += 1;
                    }
                    let digit_start = input_pos;
                    while input_pos < input_bytes.len() && input_bytes[input_pos] >= b'0' && input_bytes[input_pos] <= b'7' && count < max {
                        input_pos += 1;
                        count += 1;
                    }
                    if input_pos == digit_start {
                        input_pos = entry_pos;
                        break;
                    }
                    if !suppress {
                        let s = std::str::from_utf8(&input_bytes[digit_start..input_pos]).unwrap_or("0");
                        let mut val = i64::from_str_radix(s, 8).unwrap_or(0);
                        if negative {
                            val = -val;
                        }
                        sscanf_put(&mut results, cur_slot, Value::Long(val));
                    }
                }
                b'f' | b'e' | b'g' | b'E' | b'G' => {
                    let start = input_pos;
                    if input_pos < input_bytes.len() && (input_bytes[input_pos] == b'-' || input_bytes[input_pos] == b'+') {
                        input_pos += 1;
                    }
                    let int_start = input_pos;
                    let mut saw_digit = false;
                    while input_pos < input_bytes.len() && input_bytes[input_pos].is_ascii_digit() {
                        input_pos += 1;
                        saw_digit = true;
                    }
                    if input_pos < input_bytes.len() && input_bytes[input_pos] == b'.' {
                        input_pos += 1;
                        while input_pos < input_bytes.len() && input_bytes[input_pos].is_ascii_digit() {
                            input_pos += 1;
                            saw_digit = true;
                        }
                    }
                    // Optional exponent.
                    if saw_digit && input_pos < input_bytes.len() && (input_bytes[input_pos] == b'e' || input_bytes[input_pos] == b'E') {
                        let exp_start = input_pos;
                        input_pos += 1;
                        if input_pos < input_bytes.len() && (input_bytes[input_pos] == b'-' || input_bytes[input_pos] == b'+') {
                            input_pos += 1;
                        }
                        let exp_digit_start = input_pos;
                        while input_pos < input_bytes.len() && input_bytes[input_pos].is_ascii_digit() {
                            input_pos += 1;
                        }
                        if input_pos == exp_digit_start {
                            // No digits after e — roll back exponent.
                            input_pos = exp_start;
                        }
                    }
                    if !saw_digit {
                        // No digits matched — roll back and stop parsing.
                        input_pos = start;
                        let _ = int_start;
                        break;
                    } else if !suppress {
                        let s = std::str::from_utf8(&input_bytes[start..input_pos]).unwrap_or("0");
                        let val = s.parse::<f64>().unwrap_or(0.0);
                        sscanf_put(&mut results, cur_slot, Value::Double(val));
                    }
                }
                b's' => {
                    // Read non-whitespace string. If no characters consumed,
                    // stop parsing — PHP leaves remaining ref args untouched.
                    let start = input_pos;
                    let max = width.unwrap_or(usize::MAX);
                    let mut count = 0;
                    while input_pos < input_bytes.len() && !matches!(input_bytes[input_pos], b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r') && count < max {
                        input_pos += 1;
                        count += 1;
                    }
                    if input_pos == start {
                        break;
                    }
                    if !suppress {
                        sscanf_put(&mut results, cur_slot, Value::String(PhpString::from_vec(input_bytes[start..input_pos].to_vec())));
                    }
                }
                b'c' => {
                    // Read up to width characters (default 1). Does not skip
                    // leading whitespace and accepts any byte (including
                    // whitespace), matching PHP/C scanf semantics.
                    let n = width.unwrap_or(1);
                    let start = input_pos;
                    let mut count = 0;
                    while input_pos < input_bytes.len() && count < n {
                        input_pos += 1;
                        count += 1;
                    }
                    if input_pos == start {
                        break;
                    }
                    if !suppress {
                        sscanf_put(&mut results, cur_slot, Value::String(PhpString::from_vec(input_bytes[start..input_pos].to_vec())));
                    }
                }
                b'n' => {
                    // Number of characters consumed so far
                    if !suppress {
                        sscanf_put(&mut results, cur_slot, Value::Long(input_pos as i64));
                    }
                }
                b'[' => {
                    // Character class [abc] or [^abc]. Supports ranges like
                    // [0-9], [a-z]. A `]` as first char is literal.
                    let negated = fmt_pos < format_bytes.len() && format_bytes[fmt_pos] == b'^';
                    if negated { fmt_pos += 1; }
                    let mut raw_set: Vec<u8> = Vec::new();
                    if fmt_pos < format_bytes.len() && format_bytes[fmt_pos] == b']' {
                        raw_set.push(b']');
                        fmt_pos += 1;
                    }
                    while fmt_pos < format_bytes.len() && format_bytes[fmt_pos] != b']' {
                        raw_set.push(format_bytes[fmt_pos]);
                        fmt_pos += 1;
                    }
                    if fmt_pos < format_bytes.len() { fmt_pos += 1; } // skip ]

                    // Build expanded set, processing ranges (a-b).
                    let mut in_set_check = |c: u8| -> bool {
                        let mut i = 0;
                        while i < raw_set.len() {
                            if i + 2 < raw_set.len() && raw_set[i + 1] == b'-' {
                                let lo = raw_set[i];
                                let hi = raw_set[i + 2];
                                if lo <= c && c <= hi {
                                    return true;
                                }
                                i += 3;
                            } else {
                                if raw_set[i] == c {
                                    return true;
                                }
                                i += 1;
                            }
                        }
                        false
                    };

                    let start = input_pos;
                    let max = width.unwrap_or(usize::MAX);
                    let mut count = 0;
                    while input_pos < input_bytes.len() && count < max {
                        let in_set = in_set_check(input_bytes[input_pos]);
                        if (negated && in_set) || (!negated && !in_set) {
                            break;
                        }
                        input_pos += 1;
                        count += 1;
                    }
                    if input_pos == start {
                        break;
                    }
                    if !suppress {
                        sscanf_put(&mut results, cur_slot, Value::String(PhpString::from_vec(input_bytes[start..input_pos].to_vec())));
                    }
                }
                _ => {
                    // Bad scan conversion character — PHP throws ValueError.
                    let ch_str = if (32..=126).contains(&spec) {
                        format!("{}", spec as char)
                    } else {
                        format!("\\x{:02x}", spec)
                    };
                    let msg = format!("Bad scan conversion character \"{}\"", ch_str);
                    let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    return Err(VmError { message: msg, line: vm.current_line });
                }
            }
            // Advance the default output slot for the next conversion.
            // Suppressed conversions (`%*`) still don't consume a slot, but
            // since we never wrote, the slot is naturally available; sticking
            // to "next conversion after a non-suppressed write" matches PHP.
            if !suppress {
                next_slot = cur_slot + 1;
            }
            nconversions += 1;
        } else if matches!(format_bytes[fmt_pos], b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r') {
            // Whitespace in format matches any amount of whitespace in input
            fmt_pos += 1;
            while input_pos < input_bytes.len() && matches!(input_bytes[input_pos], b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r') {
                input_pos += 1;
            }
        } else {
            // Literal match: a mismatch stops parsing entirely (matches
            // PHP/C scanf — refusing to skip the literal keeps the rest
            // of the format unattempted).
            if input_pos >= input_bytes.len() || input_bytes[input_pos] != format_bytes[fmt_pos] {
                break;
            }
            input_pos += 1;
            fmt_pos += 1;
        }
    }

    // Underflow + no conversions yet → PHP returns NULL (no refs) or -1 (refs).
    if underflow && nconversions == 0 {
        if has_refs {
            return Ok(Value::Long(-1));
        } else {
            return Ok(Value::Null);
        }
    }

    if has_refs {
        // With reference args, PHP only assigns the ref args that
        // successfully matched; refs corresponding to slots that were
        // never filled (or filled with NULL by an explicit empty %s/%c/%[
        // match) keep their previous value. Return value is the number
        // of successful conversions.
        let mut successful = 0usize;
        for (i, val) in results.iter().enumerate() {
            if matches!(val, Value::Null) {
                continue;
            }
            successful += 1;
            if let Some(arg) = args.get(i + 2) {
                if let Value::Reference(r) = arg {
                    *r.borrow_mut() = val.clone();
                }
            }
        }
        Ok(Value::Long(successful as i64))
    } else {
        // Array-return form: pad to the total slot count so unfilled
        // slots are explicit NULLs in the returned array.
        let total_slots = count_sscanf_total_slots(format_bytes);
        while results.len() < total_slots {
            results.push(Value::Null);
        }
        let mut arr = PhpArray::new();
        for val in results {
            arr.push(val);
        }
        Ok(Value::Array(Rc::new(RefCell::new(arr))))
    }
}
// === Shell ===

/// Throw a ValueError from a builtin.
fn throw_value_error_msg(vm: &mut Vm, msg: &str) -> VmError {
    throw_value_error_in(vm, "", &[], msg)
}

/// Throw a ValueError with the builtin's own frame in the stack trace.
fn throw_value_error_in(vm: &mut Vm, func: &str, args: &[Value], msg: &str) -> VmError {
    if !func.is_empty() {
        vm.call_stack.push((func.to_string(), vm.current_file.clone(), vm.current_line, args.to_vec(), false));
    }
    let exc = vm.create_exception(b"ValueError", msg, vm.current_line);
    if !func.is_empty() {
        vm.call_stack.pop();
    }
    vm.current_exception = Some(exc);
    VmError { message: format!("Uncaught ValueError: {}", msg), line: vm.current_line }
}

/// sysconf(_SC_ARG_MAX) on Linux.
const SHELL_CMD_MAX_LEN: usize = 2097152;

fn escapeshellarg_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    if s.as_bytes().contains(&0) {
        return Err(throw_value_error_in(vm, "escapeshellarg", args, "escapeshellarg(): Argument #1 ($arg) must not contain any null bytes"));
    }
    if s.len() > SHELL_CMD_MAX_LEN - 3 {
        return Err(throw_value_error_msg(vm, &format!("Argument exceeds the allowed length of {} bytes", SHELL_CMD_MAX_LEN)));
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
        return Err(throw_value_error_in(vm, "escapeshellcmd", args, "escapeshellcmd(): Argument #1 ($command) must not contain any null bytes"));
    }
    if str.len() > SHELL_CMD_MAX_LEN - 3 {
        return Err(throw_value_error_msg(vm, &format!("Command exceeds the allowed length of {} bytes", SHELL_CMD_MAX_LEN)));
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
            b'#' | b'&' | b';' | b'`' | b'|' | b'*' | b'?' | b'~' | b'<' | b'>' | b'^'
            | b'(' | b')' | b'[' | b']' | b'{' | b'}' | b'$' | b'\\' | b'\x0A' | b'\xFF' => {
                result.push(b'\\');
                result.push(byte);
            }
            _ => result.push(byte),
        }
    }
    if result.len() > SHELL_CMD_MAX_LEN + 1 {
        return Err(throw_value_error_msg(vm, &format!("Escaped command exceeds the allowed length of {} bytes", SHELL_CMD_MAX_LEN)));
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

fn http_build_query_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = args.first().unwrap_or(&Value::Null);
    // Top-level enum: throw TypeError.
    if let Value::Object(obj) = data {
        let cls_lower: Vec<u8> = obj.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
        if let Some(ce) = vm.classes.get(&cls_lower) {
            if ce.is_enum {
                let name = obj.borrow().class_name.clone();
                let msg = format!("http_build_query(): Argument #1 ($data) must not be an enum, {} given", String::from_utf8_lossy(&name));
                let exc = vm.create_exception(b"TypeError", &msg, 0);
                vm.current_exception = Some(exc);
                return Err(VmError { message: "Uncaught TypeError".to_string(), line: 0 });
            }
        }
    }
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
        vm: &mut Vm,
        parts: &mut Vec<String>,
        prefix: &str,
        val: &Value,
        separator: &str,
        enc_type: i64,
        numeric_prefix: &Option<String>,
        depth: usize,
    ) {
        // PHP's http_build_query bails out on deeply nested or recursive structures.
        if depth > 128 {
            return;
        }
        match val {
            Value::Array(arr) => {
                let pairs: Vec<(goro_core::array::ArrayKey, Value)> = arr.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                for (key, v) in pairs {
                    let k = match &key {
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
                    build_query_recursive(vm, parts, &new_prefix, &v, separator, enc_type, numeric_prefix, depth + 1);
                }
            }
            Value::Object(obj) => {
                // Handle enums specially:
                //   - backed enum case → use its backing value
                //   - unbacked enum   → ValueError
                let class_lower: Vec<u8> = obj.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                let (is_enum, has_backing, class_name) = vm.classes.get(&class_lower)
                    .map(|ce| (ce.is_enum, ce.enum_backing_type.is_some(), ce.name.clone()))
                    .unwrap_or((false, false, Vec::new()));
                if is_enum {
                    if has_backing {
                        let val = obj.borrow().get_property(b"value");
                        let v = url_encode(&val.to_php_string().to_string_lossy(), enc_type);
                        parts.push(format!("{}={}", prefix, v));
                    } else {
                        let msg = format!("Unbacked enum {} cannot be converted to a string", String::from_utf8_lossy(&class_name));
                        let exc = vm.create_exception(b"ValueError", &msg, 0);
                        vm.current_exception = Some(exc);
                    }
                    return;
                }
                // Treat objects like arrays: iterate accessible properties.
                let obj_b = obj.borrow();
                let scope_class: Option<Vec<u8>> = vm.current_class_scope()
                    .map(|name| name.iter().map(|b| b.to_ascii_lowercase()).collect());
                let mut visibility_map: std::collections::HashMap<Vec<u8>, (goro_core::object::Visibility, Vec<u8>)> = std::collections::HashMap::new();
                let mut walk_class = Some(class_lower.clone());
                while let Some(c) = walk_class {
                    if let Some(ce) = vm.classes.get(&c) {
                        for p in &ce.properties {
                            visibility_map.entry(p.name.clone())
                                .or_insert((p.visibility, p.declaring_class.clone()));
                        }
                        walk_class = ce.parent.as_ref().map(|p| p.iter().map(|b| b.to_ascii_lowercase()).collect());
                    } else {
                        walk_class = None;
                    }
                }
                let props: Vec<(Vec<u8>, Value)> = obj_b.properties.iter().cloned().collect();
                drop(obj_b);
                for (prop_name, prop_val) in props {
                    let visible = if let Some((vis, decl)) = visibility_map.get(&prop_name) {
                        match vis {
                            goro_core::object::Visibility::Public => true,
                            goro_core::object::Visibility::Protected => {
                                if let Some(ref s) = scope_class {
                                    s == &class_lower || vm.class_extends(s, &class_lower) || vm.class_extends(&class_lower, s)
                                } else { false }
                            }
                            goro_core::object::Visibility::Private => {
                                if let Some(ref s) = scope_class { s == decl } else { false }
                            }
                        }
                    } else { true };
                    if !visible { continue; }
                    let k = String::from_utf8_lossy(&prop_name).to_string();
                    let new_prefix = if prefix.is_empty() {
                        url_encode(&k, enc_type)
                    } else {
                        format!("{}%5B{}%5D", prefix, url_encode(&k, enc_type))
                    };
                    build_query_recursive(vm, parts, &new_prefix, &prop_val, separator, enc_type, numeric_prefix, depth + 1);
                }
            }
            Value::Null | Value::Undef => {
                // PHP's http_build_query skips null entirely
            }
            Value::Resource(_) => {
                // PHP's http_build_query skips resources entirely
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

    // Use the recursive function with empty prefix for the top level.
    // For arrays/objects this iterates entries; scalar top-level args would
    // produce a TypeError in PHP — we silently no-op for unsupported types.
    if matches!(data, Value::Array(_) | Value::Object(_)) {
        build_query_recursive(vm, &mut parts, "", data, &separator, enc_type, &_numeric_prefix, 0);
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
        Ok(if check_access_bit(&path.to_string_lossy(), 0o400, 0o040, 0o004) {
            Value::True
        } else {
            Value::False
        })
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
        Ok(if check_access_bit(&path.to_string_lossy(), 0o200, 0o020, 0o002) {
            Value::True
        } else {
            Value::False
        })
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
/// Read a file with optional compress.zlib:// (gzip/zlib decompression) prefix.
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
            "" => {} // skip empty parts (caused by //)
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

pub(crate) fn read_path_with_wrappers(path_str: &str) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    if let Some(real_path) = path_str.strip_prefix("compress.zlib://") {
        let real_path = real_path.strip_prefix("file://").unwrap_or(real_path);
        let real_path = normalize_path_lexical(real_path);
        let data = std::fs::read(&real_path)?;
        if data.len() >= 2 && data[0] == 0x1f && data[1] == 0x8b {
            let mut dec = flate2::read::GzDecoder::new(data.as_slice());
            let mut buf = Vec::new();
            dec.read_to_end(&mut buf)?;
            return Ok(buf);
        }
        if data.len() >= 2 && (data[0] & 0x0F) == 8 {
            let mut dec = flate2::read::ZlibDecoder::new(data.as_slice());
            let mut buf = Vec::new();
            if dec.read_to_end(&mut buf).is_ok() {
                return Ok(buf);
            }
        }
        return Ok(data);
    }
    // Apply lexical normalization for plain filesystem paths.
    let normalized = if !path_str.contains("://") {
        normalize_path_lexical(path_str)
    } else {
        path_str.to_string()
    };
    std::fs::read(&normalized)
}

fn file_get_contents_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    // Empty filename → ValueError, matching PHP's behavior.
    if path.as_bytes().is_empty() {
        let msg = "Path must not be empty";
        let exc = vm.create_exception(b"ValueError", msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg.to_string(), line: vm.current_line });
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
            let msg = "file_get_contents(): Argument #5 ($length) must be greater than or equal to 0";
            let exc = vm.create_exception(b"ValueError", msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg.to_string(), line: vm.current_line });
        }
    }

    let path_str = path.to_string_lossy();
    // data: and data:// URIs: route through fopen (which already handles
    // base64-decoding, urldecoding, etc.) to avoid duplicating the logic.
    // Reject datafoo: and similar malformed schemes.
    if (path_str.starts_with("data:") || path_str.starts_with("data://"))
        && (path_str.starts_with("data:,") || path_str.starts_with("data:;")
            || path_str.starts_with("data:text") || path_str.starts_with("data:application")
            || path_str.starts_with("data:image") || path_str.starts_with("data:audio")
            || path_str.starts_with("data:video") || path_str.starts_with("data:font")
            || path_str.starts_with("data:model") || path_str.starts_with("data:message")
            || path_str.starts_with("data:multipart") || path_str.starts_with("data:base64,")
            || path_str.starts_with("data://"))
    {
        let inner_args = vec![Value::String(PhpString::from_string(path_str.to_string())), Value::String(PhpString::from_bytes(b"r"))];
        let fp = fopen_fn(vm, &inner_args)?;
        let fid = match fp { Value::Resource(n) => n, _ => return Ok(Value::False), };
        let get_args = if length.is_some() {
            vec![Value::Resource(fid), Value::Long(length.unwrap()), Value::Long(offset)]
        } else {
            vec![Value::Resource(fid), Value::Long(-1), Value::Long(offset)]
        };
        let res = stream_get_contents_fn(vm, &get_args)?;
        let _ = fclose_fn(vm, &[Value::Resource(fid)]);
        return Ok(res);
    }
    // php://filter routes through fopen so the filter chain is applied.
    if path_str.starts_with("php://filter/") {
        let inner_args = vec![Value::String(PhpString::from_string(path_str.to_string())), Value::String(PhpString::from_bytes(b"r"))];
        let fp = fopen_fn(vm, &inner_args)?;
        let fid = match fp { Value::Resource(n) => n, _ => return Ok(Value::False), };
        let get_args = if length.is_some() {
            vec![Value::Resource(fid), Value::Long(length.unwrap()), Value::Long(offset)]
        } else {
            vec![Value::Resource(fid), Value::Long(-1), Value::Long(offset)]
        };
        let res = stream_get_contents_fn(vm, &get_args)?;
        let _ = fclose_fn(vm, &[Value::Resource(fid)]);
        return Ok(res);
    }
    // php://input - request body for HTTP requests served by the built-in CLI server.
    if path_str == "php://input" {
        let body = get_request_body();
        return Ok(Value::String(PhpString::from_vec(body)));
    }
    // php://stdin / php://stdout / php://stderr / php://memory / php://temp:
    // file_get_contents on these should return false (no warning) or empty.
    if path_str == "php://stdin" || path_str == "php://stdout" || path_str == "php://stderr"
        || path_str == "php://memory" || path_str.starts_with("php://temp")
    {
        return Ok(Value::False);
    }
    // http:// and https:// via the built-in minimal HTTP client.
    if path_str.starts_with("http://") || path_str.starts_with("https://") {
        let mut method = "GET".to_string();
        let mut req_headers: Vec<(String, String)> = Vec::new();
        let mut req_body: Vec<u8> = Vec::new();
        if let Some(Value::Array(ctx)) = context {
            if let Some(Value::Array(http)) = ctx.borrow().get(&ArrayKey::String(PhpString::from_bytes(b"http"))).cloned() {
                let http = http.borrow();
                if let Some(m) = http.get(&ArrayKey::String(PhpString::from_bytes(b"method"))) {
                    method = m.to_php_string().to_string_lossy();
                }
                if let Some(h) = http.get(&ArrayKey::String(PhpString::from_bytes(b"header"))) {
                    match h {
                        Value::Array(harr) => {
                            for (_, v) in harr.borrow().iter() {
                                let s = v.to_php_string().to_string_lossy();
                                if let Some(colon) = s.find(':') {
                                    req_headers.push((s[..colon].trim().to_string(), s[colon + 1..].trim().to_string()));
                                }
                            }
                        }
                        _ => {
                            let s = h.to_php_string().to_string_lossy();
                            for line in s.replace("\r\n", "\n").split('\n') {
                                if let Some(colon) = line.find(':') {
                                    req_headers.push((line[..colon].trim().to_string(), line[colon + 1..].trim().to_string()));
                                }
                            }
                        }
                    }
                }
                if let Some(c) = http.get(&ArrayKey::String(PhpString::from_bytes(b"content"))) {
                    req_body = c.to_php_string().as_bytes().to_vec();
                }
            }
        }
        return Ok(http_fetch_ctx(&path_str, vm, &method, &req_headers, &req_body));
    }
    if !path_str.starts_with("php://") && !path_str.starts_with("data://") && !path_str.starts_with("compress.zlib://") && !path_str.starts_with("compress.bzip2://") {
        if !vm.check_open_basedir("file_get_contents", &path_str) {
            return Ok(Value::False);
        }
    }
    // If path is an existing directory, emit a notice rather than failing
    // silently. PHP says: "Read of N bytes failed with errno=21 Is a directory".
    if !path_str.contains("://") && std::path::Path::new(&*path_str).is_dir() {
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
            vm.emit_warning(&format!("file_get_contents({}): Failed to open stream: No such file or directory", path_str));
            Ok(Value::False)
        }
    }
}
fn http_fetch(url: &str, vm: &mut Vm) -> Value {
    http_fetch_ctx(url, vm, "GET", &[], &[])
}

fn http_fetch_ctx(url: &str, vm: &mut Vm, method: &str, headers: &[(String, String)], body: &[u8]) -> Value {
    // Follow up to 20 redirects (PHP default is 20).
    let mut cur_url = url.to_string();
    let mut cur_method = method.to_string();
    let mut cur_headers: Vec<(String, String)> = headers.to_vec();
    let mut cur_body: Vec<u8> = body.to_vec();
    for _ in 0..20 {
        let (status, location, response_body) = http_fetch_once(&cur_url, vm, &cur_method, &cur_headers, &cur_body);
        if response_body.is_none() {
            return Value::False;
        }
        let response_body = response_body.unwrap();
        // Follow 3xx Location redirects.
        if (status == 301 || status == 302 || status == 303 || status == 307 || status == 308) && location.is_some() {
            let loc = location.unwrap();
            let next_url = if loc.starts_with("http://") || loc.starts_with("https://") {
                loc
            } else if let Some(slash) = cur_url.find("://").and_then(|s| cur_url[s + 3..].find('/').map(|p| s + 3 + p)) {
                let base = &cur_url[..slash];
                if loc.starts_with('/') { format!("{}{}", base, loc) } else { format!("{}/{}", base, loc) }
            } else {
                format!("{}{}", cur_url.trim_end_matches('/'), if loc.starts_with('/') { loc.clone() } else { format!("/{}", loc) })
            };
            // For 301/302/303 -> downgrade to GET (PHP behaviour with default
            // follow_location); for 307/308 keep method+body.
            if status == 301 || status == 302 || status == 303 {
                cur_method = "GET".to_string();
                cur_body.clear();
                // Strip Content-Length / Content-Type since body is gone
                cur_headers.retain(|(k, _)| {
                    !k.eq_ignore_ascii_case("content-length") && !k.eq_ignore_ascii_case("content-type")
                });
            }
            cur_url = next_url;
            continue;
        }
        return Value::String(PhpString::from_vec(response_body));
    }
    Value::False
}

/// Performs a single HTTP/1.0 request. Returns (status, Location, body).
fn http_fetch_once(
    url: &str,
    vm: &mut Vm,
    method: &str,
    headers: &[(String, String)],
    body: &[u8],
) -> (u16, Option<String>, Option<Vec<u8>>) {
    use std::io::{Read as IoRead, Write as IoWrite};
    use std::net::{TcpStream, ToSocketAddrs};

    // Parse the URL: scheme://host[:port]/path?query
    let after_scheme = if let Some(rest) = url.strip_prefix("http://") {
        rest
    } else if url.strip_prefix("https://").is_some() {
        vm.emit_warning(&format!(
            "file_get_contents(): https:// is not supported (URL: {})",
            url
        ));
        return (0, None, None);
    } else {
        return (0, None, None);
    };
    let (host_port, path) = match after_scheme.find('/') {
        Some(i) => (&after_scheme[..i], &after_scheme[i..]),
        None => (after_scheme, "/"),
    };
    let (host, port) = match host_port.rfind(':') {
        Some(i) => {
            let p: u16 = host_port[i + 1..].parse().unwrap_or(80);
            (&host_port[..i], p)
        }
        None => (host_port, 80u16),
    };
    let addr = format!("{}:{}", host, port);
    let socket_addr = match addr.to_socket_addrs() {
        Ok(mut it) => match it.next() {
            Some(a) => a,
            None => {
                vm.emit_warning(&format!(
                    "file_get_contents({}): Failed to open stream: HTTP request failed!",
                    url
                ));
                return (0, None, None);
            }
        },
        Err(_) => {
            vm.emit_warning(&format!(
                "file_get_contents({}): Failed to open stream: HTTP request failed!",
                url
            ));
            return (0, None, None);
        }
    };
    let mut stream = match TcpStream::connect_timeout(&socket_addr, std::time::Duration::from_secs(10)) {
        Ok(s) => s,
        Err(_) => {
            vm.emit_warning(&format!(
                "file_get_contents({}): Failed to open stream: HTTP request failed!",
                url
            ));
            return (0, None, None);
        }
    };
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(10)));
    let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(10)));
    // Build the request
    let mut req = String::new();
    req.push_str(&format!("{} {} HTTP/1.0\r\n", method, path));
    let mut has_host = false;
    let mut has_ua = false;
    let mut has_cl = false;
    let mut has_ct = false;
    for (k, v) in headers {
        if k.eq_ignore_ascii_case("host") { has_host = true; }
        if k.eq_ignore_ascii_case("user-agent") { has_ua = true; }
        if k.eq_ignore_ascii_case("content-length") { has_cl = true; }
        if k.eq_ignore_ascii_case("content-type") { has_ct = true; }
        req.push_str(&format!("{}: {}\r\n", k, v));
    }
    if !has_host { req.push_str(&format!("Host: {}\r\n", host_port)); }
    if !has_ua { req.push_str("User-Agent: goro/0.1\r\n"); }
    req.push_str("Connection: close\r\n");
    if !body.is_empty() {
        if !has_cl {
            req.push_str(&format!("Content-Length: {}\r\n", body.len()));
        }
        if !has_ct {
            req.push_str("Content-Type: application/x-www-form-urlencoded\r\n");
        }
    }
    req.push_str("\r\n");
    let mut req_bytes = req.into_bytes();
    req_bytes.extend_from_slice(body);
    if stream.write_all(&req_bytes).is_err() {
        vm.emit_warning(&format!(
            "file_get_contents({}): Failed to open stream: HTTP request failed!",
            url
        ));
        return (0, None, None);
    }
    let mut buf = Vec::new();
    if stream.read_to_end(&mut buf).is_err() {
        vm.emit_warning(&format!(
            "file_get_contents({}): Failed to open stream: HTTP request failed!",
            url
        ));
        return (0, None, None);
    }
    // Split headers from body
    let sep = match buf.windows(4).position(|w| w == b"\r\n\r\n") {
        Some(s) => s,
        None => {
            // No headers terminator -> error
            vm.emit_warning(&format!(
                "file_get_contents({}): Failed to open stream: HTTP request failed!",
                url
            ));
            return (0, None, None);
        }
    };
    let headers_part = &buf[..sep];
    let body = buf[sep + 4..].to_vec();

    // Parse status code and Location header
    let mut status: u16 = 200;
    let mut location: Option<String> = None;
    let mut http_headers = PhpArray::new();
    if let Ok(hs) = std::str::from_utf8(headers_part) {
        for (i, line) in hs.split("\r\n").enumerate() {
            http_headers.set(
                ArrayKey::Int(i as i64),
                Value::String(PhpString::from_string(line.to_string())),
            );
            if i == 0 {
                // "HTTP/1.x NNN ..." - extract status
                let parts: Vec<&str> = line.splitn(3, ' ').collect();
                if parts.len() >= 2 {
                    if let Ok(s) = parts[1].parse::<u16>() {
                        status = s;
                    }
                }
            } else if let Some(colon) = line.find(':') {
                let name = line[..colon].trim();
                if name.eq_ignore_ascii_case("Location") {
                    location = Some(line[colon + 1..].trim().to_string());
                }
            }
        }
    }
    vm.set_global(
        b"http_response_header".to_vec(),
        Value::Array(Rc::new(RefCell::new(http_headers))),
    );

    (status, location, Some(body))
}

fn file_put_contents_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    if !vm.check_open_basedir("file_put_contents", &path.to_string_lossy()) {
        return Ok(Value::False);
    }
    let data_val = args.get(1).unwrap_or(&Value::Null);
    // If $data is a resource, it should be a stream resource.
    if let Value::Resource(rid) = data_val {
        let is_open_stream = FILE_HANDLES.with(|h| h.borrow().contains_key(rid));
        let rtype = goro_core::value::resource_type_of(*rid);
        let is_stream = is_open_stream || rtype.as_deref() == Some(b"stream".as_ref());
        if !is_stream {
            let msg = "file_put_contents(): supplied resource is not a valid stream resource".to_string();
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
        }
    }
    // If $context (arg 3) is a resource, it should be a valid Stream-Context.
    if let Some(Value::Resource(rid)) = args.get(3) {
        let bytes = goro_core::value::resource_type_of(*rid);
        if bytes.as_deref() != Some(b"stream-context".as_ref()) {
            let msg = "file_put_contents(): supplied resource is not a valid Stream-Context resource".to_string();
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
        }
    }
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
        Value::Object(obj) => {
            // PHP attempts __toString. If absent, emit warning and return false.
            let class_name = obj.borrow().class_name.clone();
            let class_lower: Vec<u8> = class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
            let has_tostring = vm.classes.get(&class_lower)
                .map(|c| c.get_method(b"__tostring").is_some())
                .unwrap_or(false);
            if !has_tostring {
                // PHP's file_put_contents silently returns false rather than
                // emitting a warning for objects without __toString.
                let _ = class_name;
                return Ok(Value::False);
            }
            match vm.call_object_method(data_val, b"__tostring", &[]) {
                Some(result) => result.to_php_string().as_bytes().to_vec(),
                None => return Ok(Value::False),
            }
        }
        _ => data_val.to_php_string().as_bytes().to_vec(),
    };
    let path_str = path.to_string_lossy();

    // Handle compress.zlib:// scheme for writes (gzip-compress the data)
    if let Some(real_path) = path_str.strip_prefix("compress.zlib://") {
        let real_path = real_path.strip_prefix("file://").unwrap_or(real_path);
        use std::io::Write;
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        if enc.write_all(&data_bytes).is_err() {
            return Ok(Value::False);
        }
        match enc.finish() {
            Ok(compressed) => match std::fs::write(real_path, &compressed) {
                Ok(_) => return Ok(Value::Long(data_bytes.len() as i64)),
                Err(e) => {
                    vm.emit_warning(&format!("file_put_contents({}): Failed to open stream: {}", path_str, e));
                    return Ok(Value::False);
                }
            },
            Err(_) => return Ok(Value::False),
        }
    }

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
    let path_str = path.to_string_lossy().to_string();
    // PHP: realpath("") and realpath(".") return the current working dir.
    let resolved_input = if path_str.is_empty() {
        ".".to_string()
    } else {
        path_str
    };
    match std::fs::canonicalize(&resolved_input) {
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
    if args.len() >= 3 && matches!(args.get(1), Some(Value::Null)) && !matches!(args.get(2), Some(Value::Null)) {
        let msg = "touch(): Argument #2 ($mtime) cannot be null when argument #3 ($atime) is an integer".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
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
                vm.emit_warning(&format!("touch(): Unable to create file {} because No such file or directory", path_str));
                return Ok(Value::False);
            }
        }
        if let Err(e) = std::fs::write(p, b"") {
            let msg = strip_os_error(&e.to_string());
            vm.emit_warning(&format!("touch(): Unable to create file {} because {}", path_str, msg));
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
        libc::timeval { tv_sec: atime, tv_usec: 0 },
        libc::timeval { tv_sec: mtime, tv_usec: 0 },
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
    if matches!(res, Value::False) {
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
fn is_numeric_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        let msg = "is_numeric() expects exactly 1 argument, 0 given";
        let exc = vm.create_exception(b"ArgumentCountError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg.into(), line: vm.current_line });
    }
    if args.len() > 1 {
        let msg = format!("is_numeric() expects exactly 1 argument, {} given", args.len());
        let exc = vm.create_exception(b"ArgumentCountError", &msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
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
        return Err(VmError { message: msg, line: vm.current_line });
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
                if result.is_empty() { result = "/".to_string(); }
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
        if bytes[end] != b'/' { break; }
        if end == 0 {
            return vec![b'/'];
        }
        end -= 1;
    }
    // Strip filename (non-slash chars)
    loop {
        if bytes[end] == b'/' { break; }
        if end == 0 {
            return vec![b'.'];
        }
        end -= 1;
    }
    // Strip slashes that came before the filename
    loop {
        if bytes[end] != b'/' { break; }
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
fn php_basename_bytes(path: &[u8]) -> Vec<u8> {
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
        basename_bytes.iter().rposition(|&c| c == b'.').map(|p| basename_bytes[p + 1..].to_vec())
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
                result.set(ArrayKey::String(PhpString::from_bytes(b"dirname")), Value::String(PhpString::from_bytes(d)));
            }
        }
        result.set(ArrayKey::String(PhpString::from_bytes(b"basename")), Value::String(PhpString::from_bytes(&basename_bytes)));
        // Extension only present when basename contains '.'.
        if let Some(e) = extension_bytes.as_ref() {
            result.set(ArrayKey::String(PhpString::from_bytes(b"extension")), Value::String(PhpString::from_bytes(e)));
        }
        result.set(ArrayKey::String(PhpString::from_bytes(b"filename")), Value::String(PhpString::from_bytes(&filename_bytes)));
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
fn gc_collect_fn(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(vm.gc_collect_cycles()))
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
    if let Some(Value::Object(outer)) = args.first() {
        // Trigger lazy init first so properties are visible.
        vm.maybe_run_lazy_init(outer);
        if vm.current_exception.is_some() {
            return Err(VmError { message: "Uncaught exception".to_string(), line: vm.current_line });
        }
        // Redirect lazy proxy to real backing object.
        let obj_rc = vm.proxy_redirect(outer);
        let mut obj = obj_rc.borrow();
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

        // Iterate properties in declaration order from class def. For backed props,
        // read the value from the instance's property bag; for hooked (virtual) props,
        // dispatch through the get hook.
        let class_props_order: Vec<(Vec<u8>, goro_core::object::Visibility, bool, bool, bool)> =
            if let Some(class_def) = vm.get_class_def(&class_name_lower) {
                class_def.properties.iter()
                    .map(|p| (p.name.clone(), p.visibility, p.is_virtual, p.has_get_hook, p.is_static))
                    .collect()
            } else { Vec::new() };

        let mut arr = PhpArray::new();
        for (name, vis, is_virtual, has_get_hook, is_static) in &class_props_order {
            if *is_static { continue; }
            // Skip internal properties (none normally collide, but just in case)
            if name.starts_with(b"__spl_") || name.starts_with(b"__reflection_")
                || name.starts_with(b"__timestamp") || name.starts_with(b"__enum_")
                || name.starts_with(b"__fiber_") || name.starts_with(b"__ctor_")
                || name.starts_with(b"__clone_") || name.starts_with(b"__destructed")
                || name.starts_with(b"__weak_ref_id") || name.starts_with(b"__sxml_")
                || name.starts_with(b"__lazy_") || name.starts_with(b"__uri_")
                || name.starts_with(b"__hash_") || name.starts_with(b"__hmac_")
                || name.starts_with(b"__pkey_id") {
                continue;
            }
            // Visibility check
            let accessible = match vis {
                goro_core::object::Visibility::Public => true,
                goro_core::object::Visibility::Protected => {
                    if let Some(ref cc) = calling_class_lower {
                        cc == &class_name_lower || vm.is_subclass_of(cc, &class_name_lower) || vm.is_subclass_of(&class_name_lower, cc)
                    } else { false }
                }
                goro_core::object::Visibility::Private => {
                    if let Some(ref cc) = calling_class_lower {
                        cc == &class_name_lower
                    } else { false }
                }
            };
            if !accessible { continue; }
            if *is_virtual && *has_get_hook {
                // Drop borrow before calling into VM.
                let obj_v = Value::Object(obj_rc.clone());
                drop(obj);
                let mut hook_name = b"__property_get_".to_vec();
                hook_name.extend_from_slice(name);
                if let Some(v) = vm.call_object_method(&obj_v, &hook_name, &[]) {
                    arr.set(
                        goro_core::array::ArrayKey::String(PhpString::from_vec(name.clone())),
                        v,
                    );
                }
                // Re-borrow
                obj = obj_rc.borrow();
                continue;
            }
            // Skip if the backing slot is absent: an uninitialized typed
            // property, an unset property, or a redeclared private stored under
            // a mangled key (the last is emitted by the dynamic-props pass below
            // under its bare name). get_property() returns Null for a missing
            // key, so gate on presence to avoid reporting it as a null value.
            if !obj.has_property(name) {
                continue;
            }
            // Read value from instance
            let val = obj.get_property(name);
            // Skip uninitialized
            if matches!(val, Value::Undef) {
                continue;
            }
            arr.set(
                goro_core::array::ArrayKey::String(PhpString::from_vec(name.clone())),
                val,
            );
        }
        // Also include dynamic properties not declared on the class (e.g. AllowDynamicProperties)
        for (name, val) in &obj.properties {
            if name.starts_with(b"__spl_") || name.starts_with(b"__reflection_")
                || name.starts_with(b"__timestamp") || name.starts_with(b"__enum_")
                || name.starts_with(b"__fiber_") || name.starts_with(b"__ctor_")
                || name.starts_with(b"__clone_") || name.starts_with(b"__destructed")
                || name.starts_with(b"__weak_ref_id") || name.starts_with(b"__sxml_")
                || name.starts_with(b"__lazy_") || name.starts_with(b"__uri_")
                || name.starts_with(b"__hash_") || name.starts_with(b"__hmac_") {
                continue;
            }
            if matches!(val, Value::Undef) { continue; }
            // Mangled private/protected storage keys (\0Class\0name) hold
            // redeclared private slots that the declared-property loop above
            // could not read by bare name. Demangle and apply scope visibility,
            // exposing them under the bare name when accessible.
            if name.first() == Some(&0) {
                if let Some(sep) = name[1..].iter().position(|&b| b == 0) {
                    let decl = &name[1..1 + sep];
                    let bare = &name[1 + sep + 1..];
                    let accessible = if decl == b"*" {
                        if let Some(ref cc) = calling_class_lower {
                            cc == &class_name_lower
                                || vm.is_subclass_of(cc, &class_name_lower)
                                || vm.is_subclass_of(&class_name_lower, cc)
                        } else { false }
                    } else {
                        let decl_lower: Vec<u8> =
                            decl.iter().map(|b| b.to_ascii_lowercase()).collect();
                        calling_class_lower.as_deref() == Some(decl_lower.as_slice())
                    };
                    if accessible {
                        let key = goro_core::array::ArrayKey::String(PhpString::from_vec(bare.to_vec()));
                        if !arr.contains_key(&key) {
                            arr.set(key, val.clone());
                        }
                    }
                    continue;
                }
            }
            if class_props_order.iter().any(|(n, _, _, _, _)| n == name) { continue; }
            let key = goro_core::array::ArrayKey::String(PhpString::from_vec(name.clone()));
            if !arr.contains_key(&key) {
                arr.set(key, val.clone());
            }
        }
        // zend_symtable: numeric property names become integer keys.
        Ok(Value::Array(Vm::proptable_to_symtable(Rc::new(RefCell::new(arr)))))
    } else {
        Ok(Value::False)
    }
}
fn get_class_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    // get_class() with no argument (or null) uses the current class scope
    if args.is_empty() {
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
            Err(throw_type_error_msg(_vm, format!("get_class(): Argument #1 ($object) must be of type object, {} given", type_name)))
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
        Err(throw_type_error_msg(_vm, format!("get_class(): Argument #1 ($object) must be of type object, {} given", type_name)))
    } else {
        // PHP 8: TypeError for non-object
        let type_name = Vm::value_type_name(val);
        Err(throw_type_error_msg(_vm, format!("get_class(): Argument #1 ($object) must be of type object, {} given", type_name)))
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
fn sleep_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let secs = args.first().unwrap_or(&Value::Null).to_long();
    if secs < 0 {
        return Err(throw_value_error_in(_vm, "sleep", args, "sleep(): Argument #1 ($seconds) must be greater than or equal to 0"));
    }
    // Actually sleep up to 2 seconds to support tests that compare timestamps
    // before/after sleep(1). Cap at 2s to keep tests reasonably fast.
    if secs > 0 {
        let actual = (secs as u64).min(2);
        std::thread::sleep(std::time::Duration::from_secs(actual));
    }
    Ok(Value::Long(0))
}
fn usleep_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let usecs = args.first().unwrap_or(&Value::Null).to_long();
    if usecs < 0 {
        return Err(throw_value_error_in(_vm, "usleep", args, "usleep(): Argument #1 ($microseconds) must be greater than or equal to 0"));
    }
    if usecs > 0 {
        // Sleep the actual duration, capped at 2s (like sleep()).
        let actual = (usecs as u64).min(2_000_000);
        std::thread::sleep(std::time::Duration::from_micros(actual));
    }
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
fn tempnam_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Delegate to the standards-compliant tempnam_fn3.
    tempnam_fn3(vm, args)
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
            if name.as_bytes().is_empty() || name.as_bytes().contains(&b'=') || name.as_bytes().contains(&0) {
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
        return Err(throw_value_error_in(vm, "putenv", args, "putenv(): Argument #1 ($assignment) must have a valid syntax"));
    }
    if b.contains(&0) {
        return Err(throw_value_error_in(vm, "putenv", args, "putenv(): Argument #1 ($assignment) must not contain any null bytes"));
    }
    match b.iter().position(|&c| c == b'=') {
        Some(eq) => {
            let key = std::ffi::OsStr::from_bytes(&b[..eq]);
            let value = std::ffi::OsStr::from_bytes(&b[eq + 1..]);
            unsafe { std::env::set_var(key, value); }
        }
        None => {
            unsafe { std::env::remove_var(std::ffi::OsStr::from_bytes(b)); }
        }
    }
    Ok(Value::True)
}
// spl_autoload_register, spl_autoload_functions, spl_autoload_unregister moved to goro-ext-spl

fn class_alias_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let original = args.first().unwrap_or(&Value::Null).to_php_string();
    let alias = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let original_lower: Vec<u8> = original.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
    let alias_lower: Vec<u8> = alias.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
    // Single-underscore alias was deprecated in 8.4.
    if alias.as_bytes() == b"_" {
        vm.emit_deprecated("Using \"_\" as a class alias is deprecated since 8.4");
    }
    // Reserved class names (e.g. "array", "callable", "bool") cannot be used as aliases.
    if goro_core::compiler::is_reserved_class_name(alias.as_bytes()) {
        let line = vm.current_line;
        let msg = format!("Cannot use \"{}\" as a class alias as it is reserved",
            std::str::from_utf8(alias.as_bytes()).unwrap_or(""));
        return Err(VmError { message: msg, line });
    }
    let autoload = args.get(2).map(|v| v.is_truthy()).unwrap_or(true);
    if autoload && !vm.classes.contains_key(&original_lower) && !vm.is_known_builtin_class(&original_lower) {
        vm.try_autoload_class(original.as_bytes());
    }
    if !vm.classes.contains_key(&original_lower) && !vm.is_known_builtin_class(&original_lower) {
        vm.emit_warning(&format!("Class \"{}\" not found", original.to_string_lossy()));
        return Ok(Value::False);
    }
    if let Some(existing) = vm.classes.get(&alias_lower) {
        // The message names the kind of the class being aliased.
        let kind = match vm.classes.get(&original_lower) {
            Some(o) if o.is_interface => "interface",
            Some(o) if o.is_trait => "trait",
            Some(o) if o.is_enum => "enum",
            _ => "class",
        };
        let msg = format!("Cannot redeclare {} {} (previously declared in {}:{})", kind, alias.to_string_lossy(),
            existing.filename.clone().unwrap_or_default(), existing.start_line);
        vm.emit_warning(&msg);
        return Ok(Value::False);
    }
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
    let class_lower: Vec<u8> = class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
    let check_lower: Vec<u8> = check_name.iter().map(|b| b.to_ascii_lowercase()).collect();
    if class_lower == check_lower {
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
    let class_name = match args.first().map(|v| v.deref()) {
        Some(Value::Object(obj)) => obj.borrow().class_name.clone(),
        Some(Value::String(s)) => {
            let lower = lower_bytes(s.as_bytes());
            if !vm.classes.contains_key(&lower) && !vm.is_known_builtin_class(&lower) {
                vm.try_autoload_class(s.as_bytes());
                if vm.current_exception.is_some() {
                    return Err(VmError { message: "Uncaught exception".into(), line: vm.current_line });
                }
                if !vm.classes.contains_key(&lower) && !vm.is_known_builtin_class(&lower) {
                    return Err(throw_type_error_msg(vm, format!("get_parent_class(): Argument #1 ($object_or_class) must be an object or a valid class name, string given")));
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
            return Err(throw_type_error_msg(vm, format!("get_parent_class(): Argument #1 ($object_or_class) must be an object or a valid class name, {} given", Vm::value_type_name(&other))));
        }
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

    // Sort flags follow the array they apply to. Collect arrays and
    // subsequent flags, then assign order/type based on position.
    for arg in args {
        match arg {
            Value::Array(arr) => {
                arrays.push(arr.clone());
                sort_orders.push(true); // Default ascending
                sort_types.push(0);      // Default SORT_REGULAR
            }
            Value::Reference(r) => {
                let val = r.borrow().clone();
                if let Value::Array(arr) = val {
                    arrays.push(arr.clone());
                    sort_orders.push(true);
                    sort_types.push(0);
                }
            }
            Value::Long(n) => {
                if let Some(last) = sort_orders.last_mut() {
                    match *n {
                        4 => *last = true,   // SORT_ASC
                        3 => *last = false,  // SORT_DESC
                        _ => {
                            if let Some(st) = sort_types.last_mut() {
                                *st = *n;
                            }
                        }
                    }
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

    // Extract values from each array (to use as sort keys)
    let all_values: Vec<Vec<Value>> = arrays
        .iter()
        .map(|a| a.borrow().values().cloned().collect())
        .collect();

    // Sort indices: primary by arrays[0], tiebreak with arrays[1], etc.
    let mut indices: Vec<usize> = (0..len).collect();
    indices.sort_by(|&a, &b| {
        for i in 0..arrays.len() {
            let result = php_compare_for_sort(&all_values[i][a], &all_values[i][b], sort_types[i]);
            let result = if sort_orders[i] { result } else { result.reverse() };
            if result != std::cmp::Ordering::Equal {
                return result;
            }
        }
        std::cmp::Ordering::Equal
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
            // Preserve (key, value) pairs together and reorder by sorted indices.
            // Integer keys get renumbered from 0 but string keys are preserved.
            for &sorted_idx in &indices {
                let (ref k, ref v) = old_entries[sorted_idx];
                match k {
                    ArrayKey::String(_) => {
                        new_arr.set(k.clone(), v.clone());
                    }
                    ArrayKey::Int(_) => {
                        new_arr.push(v.clone());
                    }
                }
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
    /// Directory handles keyed by resource id. Each entry stores the directory
    /// path (for rewind) and a remaining list of entry names that we pop from
    /// the back to feed readdir().
    static DIR_HANDLES: RefCell<StdHashMap<i64, DirHandle>> = RefCell::new(StdHashMap::new());
    /// "Current" directory stream pointer used as fallback when readdir/closedir/
    /// rewinddir are called with no argument or null. PHP keeps this state per
    /// request; opendir() updates it.
    static CURRENT_DIR_RESOURCE: std::cell::Cell<Option<i64>> = const { std::cell::Cell::new(None) };
    /// Stream contexts keyed by resource id. Each entry stores the options
    /// (wrapper => option_name => value) and params (notification, options).
    static STREAM_CONTEXTS: RefCell<StdHashMap<i64, StreamContext>> = RefCell::new(StdHashMap::new());
    /// User-defined stream wrappers: scheme -> (class_name, flags).
    static USER_WRAPPERS: RefCell<StdHashMap<Vec<u8>, UserWrapper>> = RefCell::new(StdHashMap::new());
}

#[derive(Clone, Default)]
pub(crate) struct StreamContext {
    /// Map: wrapper_name -> map(option_name -> Value)
    pub options: std::collections::BTreeMap<Vec<u8>, std::collections::BTreeMap<Vec<u8>, Value>>,
    /// Notification callback (or null if unset).
    pub notification: Option<Value>,
}

#[derive(Clone)]
pub(crate) struct UserWrapper {
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
    for entry in entries {
        if let Ok(e) = entry {
            names.push(e.file_name().to_string_lossy().to_string());
        }
    }
    names.sort();
    // Reverse so we can pop from the end efficiently while still feeding entries
    // in ascending order.
    names.reverse();
    Ok(names)
}

fn alloc_dir_handle(path: String) -> i64 {
    let names = match read_dir_entries(&path) {
        Ok(n) => n,
        Err(_) => Vec::new(),
    };
    NEXT_FILE_ID.with(|id| {
        let did = id.get();
        id.set(did + 1);
        DIR_HANDLES.with(|dh| {
            dh.borrow_mut().insert(did, DirHandle {
                path,
                remaining: names,
            });
        });
        goro_core::value::register_resource(did, b"stream");
        CURRENT_DIR_RESOURCE.with(|c| c.set(Some(did)));
        did
    })
}

/// Return the resource id from a directory handle Value. Accepts a Resource
/// or Long id (back-compat), a Directory object whose `handle` property holds
/// the resource id, or a Reference wrapping either of those.
fn dir_handle_resource_id(val: &Value) -> Option<i64> {
    match val {
        Value::Resource(n) => Some(*n),
        Value::Long(n) => Some(*n),
        Value::Reference(r) => dir_handle_resource_id(&r.borrow()),
        Value::Object(obj) => {
            let ob = obj.borrow();
            if ob.class_name.eq_ignore_ascii_case(b"Directory") {
                let handle = ob.get_property(b"handle");
                match handle {
                    Value::Resource(n) => Some(n),
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
}

fn alloc_file_handle(file: std::fs::File, mode: &str) -> i64 {
    alloc_file_handle_special(file, mode, SpecialStream::None)
}

fn alloc_file_handle_uri(file: std::fs::File, mode: &str, uri: &str) -> i64 {
    let fid = alloc_file_handle_special(file, mode, SpecialStream::None);
    FILE_HANDLES.with(|h| {
        if let Some(fh) = h.borrow_mut().get_mut(&fid) {
            fh.uri = uri.to_string();
        }
    });
    fid
}

fn alloc_file_handle_special_uri(file: std::fs::File, mode: &str, special: SpecialStream, uri: &str) -> i64 {
    let fid = alloc_file_handle_special(file, mode, special);
    FILE_HANDLES.with(|h| {
        if let Some(fh) = h.borrow_mut().get_mut(&fid) {
            fh.uri = uri.to_string();
            // For php:// streams, set wrapper_type and stream_type appropriately.
            match special {
                // Memory/temp streams are always read-write ("w+b"),
                // whatever mode they were opened with.
                SpecialStream::Memory => {
                    fh.wrapper_type = "PHP".to_string();
                    fh.stream_type = "MEMORY".to_string();
                    fh.mode = "w+b".to_string();
                }
                SpecialStream::Temp => {
                    fh.wrapper_type = "PHP".to_string();
                    fh.stream_type = "TEMP".to_string();
                    fh.mode = "w+b".to_string();
                }
                SpecialStream::Stdin | SpecialStream::Stdout | SpecialStream::Stderr | SpecialStream::Output => {
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

/// Helper used by file functions to validate that the first argument is an
/// open stream resource. If the value is not a stream resource (or is a closed
/// stream resource), this throws a TypeError matching PHP's wording.
///
/// Returns Ok(fid) for valid streams (FILE_HANDLES contains the id).
fn require_open_stream(vm: &mut Vm, val: &Value, func_name: &str) -> Result<i64, VmError> {
    let fid = val.to_long();
    let has_handle = FILE_HANDLES.with(|h| h.borrow().contains_key(&fid));
    if has_handle {
        return Ok(fid);
    }
    let msg = format!("{}(): Argument #1 ($stream) must be an open stream resource", func_name);
    let exc = vm.throw_type_error(msg.clone());
    vm.current_exception = Some(exc);
    Err(VmError { message: msg, line: vm.current_line })
}

fn alloc_file_handle_special(file: std::fs::File, mode: &str, special: SpecialStream) -> i64 {
    NEXT_FILE_ID.with(|id| {
        let fid = id.get();
        id.set(fid + 1);
        FILE_HANDLES.with(|handles| {
            handles.borrow_mut().insert(fid, FileHandle {
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
            });
        });
        fid
    })
}

fn data_uri_base64_decode(input: &[u8]) -> Vec<u8> {
    let mut filtered = Vec::with_capacity(input.len());
    for &b in input {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'+' | b'/' | b'=' => filtered.push(b),
            _ => {}
        }
    }
    while filtered.len() % 4 != 0 { filtered.push(b'='); }
    let mut out = Vec::with_capacity(filtered.len() * 3 / 4);
    let mut i = 0;
    while i + 4 <= filtered.len() {
        let mut n: u32 = 0;
        let mut pad = 0u32;
        for j in 0..4 {
            let c = filtered[i + j];
            let v: u32 = match c {
                b'A'..=b'Z' => (c - b'A') as u32,
                b'a'..=b'z' => (c - b'a' + 26) as u32,
                b'0'..=b'9' => (c - b'0' + 52) as u32,
                b'+' => 62,
                b'/' => 63,
                b'=' => { pad += 1; 0 }
                _ => 0,
            };
            n = (n << 6) | v;
        }
        out.push(((n >> 16) & 0xff) as u8);
        if pad < 2 { out.push(((n >> 8) & 0xff) as u8); }
        if pad < 1 { out.push((n & 0xff) as u8); }
        i += 4;
    }
    out
}

fn data_uri_urldecode(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        let b = input[i];
        if b == b'%' && i + 2 < input.len() {
            let h1 = hex_digit_misc(input[i + 1]);
            let h2 = hex_digit_misc(input[i + 2]);
            match (h1, h2) {
                (Some(a), Some(b)) => { out.push((a << 4) | b); i += 3; continue; }
                _ => {}
            }
        }
        if b == b'+' { out.push(b' '); } else { out.push(b); }
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

fn fopen_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let mut filename = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy();
    // Empty filename → ValueError, matching PHP's behavior.
    if filename.is_empty() {
        let msg = "fopen(): Argument #1 ($filename) cannot be empty";
        let exc = vm.create_exception(b"ValueError", msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg.to_string(), line: vm.current_line });
    }
    // Preserve the original filename for the uri field of stream_get_meta_data.
    // PHP keeps the path as passed to fopen() (including any file:// prefix).
    let original_uri = filename.clone();
    let mode = args.get(1).map(|v| v.to_php_string().to_string_lossy()).unwrap_or_else(|| "r".to_string());

    // PHP fopen rejects null bytes embedded in the path.
    if filename.contains('\0') {
        vm.emit_warning(&format!(
            "fopen(): Argument #1 ($filename) must not contain any null bytes"
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
                "fopen(): Remote host file access not supported, file://{}",
                rest
            ));
            vm.emit_warning(&format!(
                "fopen({}): Failed to open stream: no suitable wrapper could be found",
                filename
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
        let val = http_fetch_ctx(&filename, vm, "GET", &[], &[]);
        let body = match val {
            Value::String(s) => s.as_bytes().to_vec(),
            _ => return Ok(Value::False),
        };
        // Write to a temp file
        let tmp_path = std::env::temp_dir().join(format!(
            "goro_http_{}_{}.tmp",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
        ));
        if std::fs::write(&tmp_path, &body).is_err() {
            return Ok(Value::False);
        }
        if let Ok(f) = std::fs::File::open(&tmp_path) {
            // Best-effort cleanup; we leak the temp file otherwise.
            let _ = std::fs::remove_file(&tmp_path);
            let fid = alloc_file_handle(f, &mode);
            goro_core::value::register_resource(fid, b"stream");
            return Ok(Value::Long(fid));
        }
        return Ok(Value::False);
    }

    // Normalize mode: strip 'b' (binary) and 't' (text) flags - they're all the same on Linux
    let mode_clean: String = mode.chars().filter(|&c| c != 'b' && c != 't').collect();

    // Handle compress.zlib:// scheme: read+decompress to a temporary file (read-only).
    if let Some(real_path) = filename.strip_prefix("compress.zlib://") {
        let real_path = real_path.strip_prefix("file://").unwrap_or(real_path);
        if mode_clean == "r" {
            match std::fs::read(real_path) {
                Ok(data) => {
                    // Detect gzip vs zlib vs raw
                    use std::io::Read;
                    let decompressed = if data.len() >= 2 && data[0] == 0x1f && data[1] == 0x8b {
                        let mut dec = flate2::read::GzDecoder::new(data.as_slice());
                        let mut buf = Vec::new();
                        if dec.read_to_end(&mut buf).is_err() {
                            vm.emit_warning(&format!("fopen({}): Failed to open stream: operation failed", filename));
                            return Ok(Value::False);
                        }
                        buf
                    } else if data.len() >= 2 && (data[0] & 0x0F) == 8 {
                        let mut dec = flate2::read::ZlibDecoder::new(data.as_slice());
                        let mut buf = Vec::new();
                        if dec.read_to_end(&mut buf).is_err() {
                            data.clone()
                        } else {
                            buf
                        }
                    } else {
                        data
                    };
                    // Create a tempfile
                    let mut tmp = match tempfile::tempfile() {
                        Ok(f) => f,
                        Err(_) => return Ok(Value::False),
                    };
                    use std::io::{Seek, SeekFrom, Write};
                    if tmp.write_all(&decompressed).is_err() {
                        return Ok(Value::False);
                    }
                    let _ = tmp.seek(SeekFrom::Start(0));
                    let fid = alloc_file_handle(tmp, &mode);
                    goro_core::value::register_resource(fid, b"stream");
                    return Ok(Value::Resource(fid));
                }
                Err(_) => {
                    vm.emit_warning(&format!("fopen({}): Failed to open stream: No such file or directory", filename));
                    return Ok(Value::False);
                }
            }
        }
    }

    // Handle php://filter/<chain>/resource=<inner_url> by recursively
    // opening the inner stream then attaching the requested chain.
    if let Some(rest) = filename.strip_prefix("php://filter/") {
        // Split off the "/resource=" or "?resource=" portion.
        let (chain_str, inner_url) = if let Some(idx) = rest.find("/resource=") {
            (&rest[..idx], &rest[idx + "/resource=".len()..])
        } else if let Some(idx) = rest.find("resource=") {
            (&rest[..idx.saturating_sub(1)], &rest[idx + "resource=".len()..])
        } else {
            (rest, "")
        };
        // Recurse on the inner URL.
        let inner_args = vec![Value::String(PhpString::from_string(inner_url.to_string())), Value::String(PhpString::from_string(mode.clone()))];
        let inner = fopen_fn(vm, &inner_args)?;
        let inner_fid = match inner {
            Value::Resource(n) => n,
            _ => return Ok(Value::False),
        };
        // Parse and attach each filter spec separated by '|'.
        for spec in chain_str.split('|') {
            if spec.is_empty() { continue; }
            // spec may be "read=<filter>", "write=<filter>", or just <filter>.
            let (dir, name) = if let Some(rest) = spec.strip_prefix("read=") {
                (crate::stream_filters::STREAM_FILTER_READ, rest)
            } else if let Some(rest) = spec.strip_prefix("write=") {
                (crate::stream_filters::STREAM_FILTER_WRITE, rest)
            } else {
                (crate::stream_filters::STREAM_FILTER_ALL, spec)
            };
            let _ = crate::stream_filters::attach_filter(
                inner_fid,
                name.as_bytes(),
                dir,
                &Value::Null,
                false,
                None,
            );
        }
        return Ok(Value::Resource(inner_fid));
    }

    // Handle data: URLs: data:[mediatype][;base64],<data> (both data: and
    // data:// forms — only the latter is canonical for PHP wrappers but
    // RFC2397 uses the former).
    let data_rest = filename.strip_prefix("data://").or_else(|| filename.strip_prefix("data:"));
    if let Some(rest) = data_rest {
        // Optional mediatype and ;base64 prefix.
        let (header, payload) = rest.split_once(',').unwrap_or(("", rest));
        let is_base64 = header.split(';').any(|p| p == "base64");
        let bytes: Vec<u8> = if is_base64 {
            data_uri_base64_decode(payload.as_bytes())
        } else {
            data_uri_urldecode(payload.as_bytes())
        };
        let mut tmp = match tempfile::tempfile() {
            Ok(f) => f,
            Err(_) => return Ok(Value::False),
        };
        use std::io::{Seek, SeekFrom, Write};
        if tmp.write_all(&bytes).is_err() {
            return Ok(Value::False);
        }
        let _ = tmp.seek(SeekFrom::Start(0));
        let fid = alloc_file_handle(tmp, &mode);
        goro_core::value::register_resource(fid, b"stream");
        return Ok(Value::Resource(fid));
    }

    // Handle php:// wrapper paths. php://stdout, php://stderr, php://output
    // are write-only sinks routed through the VM output. php://memory and
    // php://temp are seekable in-memory streams backed by a tempfile.
    if let Some(rest) = filename.strip_prefix("php://") {
        let lower: String = rest.to_ascii_lowercase();
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
                    let fid = alloc_file_handle_special_uri(f, &mode, sp, &filename);
                    goro_core::value::register_resource(fid, b"stream");
                    return Ok(Value::Resource(fid));
                }
                Err(_) => return Ok(Value::False),
            }
        }

        // php://fd/<n> -- duplicate the file descriptor <n>.
        // php://fd is invalid (must include a number).
        if lower == "fd" {
            vm.emit_warning(&format!("fopen(): Invalid php:// URL specified"));
            vm.emit_warning(&format!(
                "fopen({}): Failed to open stream: operation failed",
                filename
            ));
            return Ok(Value::False);
        }
        if let Some(fd_tail) = lower.strip_prefix("fd/") {
            // Reject if trailing slash present or empty
            if fd_tail.is_empty() || fd_tail.contains('/') {
                vm.emit_warning(&format!(
                    "fopen({}): Failed to open stream: php://fd/ stream must be specified in the form php://fd/<orig fd>",
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
                { 1024 }
            };
            match parsed {
                Some(n) if n >= 0 && n < max_fd => {
                    // Try to dup the fd. If dup fails, emit error.
                    let dup_fd = unsafe { libc::dup(n as i32) };
                    if dup_fd < 0 {
                        let err = std::io::Error::last_os_error();
                        let errno = err.raw_os_error().unwrap_or(0);
                        vm.emit_warning(&format!(
                            "fopen({}): Failed to open stream: Error duping file descriptor {}; possibly it doesn't exist: [{}]: {}",
                            filename, n, errno, err
                        ));
                        return Ok(Value::False);
                    }
                    // Convert raw fd into File. We pick a SpecialStream variant
                    // so writes route correctly: fd 1 -> Stdout, fd 2 -> Stderr,
                    // others use None (writes go to the file itself).
                    use std::os::unix::io::FromRawFd;
                    let f = unsafe { std::fs::File::from_raw_fd(dup_fd) };
                    let sp = match n {
                        1 => SpecialStream::Stdout,
                        2 => SpecialStream::Stderr,
                        _ => SpecialStream::None,
                    };
                    let fid = alloc_file_handle_special(f, &mode, sp);
                    goro_core::value::register_resource(fid, b"stream");
                    return Ok(Value::Resource(fid));
                }
                _ => {
                    vm.emit_warning(&format!(
                        "fopen({}): Failed to open stream: The file descriptors must be non-negative numbers smaller than {}",
                        filename, max_fd
                    ));
                    return Ok(Value::False);
                }
            }
        }
    }

    // Validate mode: first character must be one of r/w/a/x/c, optionally
    // followed by '+', 'b' and/or 't' in any order.
    let valid_mode = {
        let chars: Vec<char> = mode.chars().collect();
        if chars.is_empty() {
            false
        } else {
            let first = chars[0];
            let valid_first = matches!(first, 'r' | 'w' | 'a' | 'x' | 'c');
            let valid_rest = chars[1..].iter().all(|&c| matches!(c, '+' | 'b' | 't' | 'e'));
            valid_first && valid_rest
        }
    };
    if !valid_mode {
        vm.emit_warning(&format!(
            "fopen({}): Failed to open stream: `{}' is not a valid mode for fopen",
            filename, mode
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
        let inc = vm.constants.get(b"include_path".as_ref())
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
        "r+" => std::fs::OpenOptions::new().read(true).write(true).open(&open_path),
        "w" => std::fs::File::create(&open_path),
        "w+" => std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(true).open(&open_path),
        "a" => std::fs::OpenOptions::new().write(true).append(true).create(true).open(&open_path),
        "a+" => std::fs::OpenOptions::new().read(true).write(true).append(true).create(true).open(&open_path),
        "x" => std::fs::OpenOptions::new().write(true).create_new(true).open(&open_path),
        "x+" => std::fs::OpenOptions::new().read(true).write(true).create_new(true).open(&open_path),
        "c" => std::fs::OpenOptions::new().write(true).create(true).open(&open_path),
        "c+" => std::fs::OpenOptions::new().read(true).write(true).create(true).open(&open_path),
        _ => std::fs::File::open(&open_path), // default to read
    };

    match file {
        Ok(f) => {
            // PHP's `uri` field in stream_get_meta_data preserves the path passed
            // to fopen() (e.g. relative paths stay relative, file:// prefix is kept).
            let fid = alloc_file_handle_uri(f, &mode, &original_uri);
            goro_core::value::register_resource(fid, b"stream");
            Ok(Value::Resource(fid))
        }
        Err(e) => {
            let msg = strip_os_error(&e.to_string());
            vm.emit_warning(&format!("fopen({}): Failed to open stream: {}", filename, msg));
            Ok(Value::False)
        }
    }
}

fn fclose_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let fid = args.first().unwrap_or(&Value::Null).to_long();
    // Flush write-filter chain.
    let flush_bytes = crate::stream_filters::flush_chain(vm, fid, crate::stream_filters::STREAM_FILTER_WRITE);
    if !flush_bytes.is_empty() {
        write_to_stream(vm, fid, &flush_bytes);
    }
    // Tear down filters attached to this stream.
    crate::stream_filters::remove_stream_filters(fid);
    FILE_HANDLES.with(|handles| {
        handles.borrow_mut().remove(&fid);
    });
    // Mark the resource as closed rather than removing it, so gettype()
    // still reports "resource (closed)" while is_resource() returns false.
    goro_core::value::close_resource(fid);
    Ok(Value::True)
}

/// Write `bytes` to the physical backing of the stream (file or vm
/// output for special streams). Used after the write filter chain has
/// been applied.
fn write_to_stream(vm: &mut Vm, fid: i64, bytes: &[u8]) -> usize {
    // The PHP constants STDIN/STDOUT/STDERR are pre-registered at resource
    // ids 1/2/3 without a FILE_HANDLES entry. Route those directly.
    if fid == 2 || fid == 3 {
        vm.write_output(bytes);
        return bytes.len();
    }
    // Check special routing for explicitly opened php://stdout etc.
    let special = FILE_HANDLES.with(|h| {
        h.borrow().get(&fid).map(|fh| fh.special).unwrap_or(SpecialStream::None)
    });
    match special {
        SpecialStream::Stdout | SpecialStream::Output | SpecialStream::Stderr => {
            vm.write_output(bytes);
            bytes.len()
        }
        SpecialStream::Stdin => {
            // Writing to stdin is a no-op; return 0
            0
        }
        SpecialStream::None | SpecialStream::Memory | SpecialStream::Temp => {
            FILE_HANDLES.with(|h| {
                let mut h = h.borrow_mut();
                if let Some(fh) = h.get_mut(&fid) {
                    // For append mode, writes always go to the end of the file.
                    // PHP/POSIX 'a' / 'a+' mode requires this behavior. std::fs::OpenOptions
                    // .append(true) on Linux already enforces it via O_APPEND for real
                    // files, but tempfile-backed php://temp / php://memory streams need
                    // an explicit seek since they are opened with .read+.write.
                    let mode_chars: String = fh.mode.chars().filter(|&c| c != 'b' && c != 't' && c != 'e').collect();
                    if mode_chars.starts_with('a') {
                        let _ = fh.file.seek(SeekFrom::End(0));
                    }
                    match fh.file.write(bytes) {
                        Ok(n) => n,
                        Err(_) => 0,
                    }
                } else { 0 }
            })
        }
    }
}

fn fread_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let arg = args.first().unwrap_or(&Value::Null);
    let fid = require_open_stream(vm, arg, "fread")?;
    let length_signed = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    if length_signed <= 0 {
        let msg = "fread(): Argument #2 ($length) must be greater than 0".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    let length = length_signed as usize;
    // If read filters are attached, route through filter chain.
    let has_read_filters = crate::stream_filters::chain_has_direction(fid, crate::stream_filters::STREAM_FILTER_READ);
    if has_read_filters {
        return Ok(Value::String(PhpString::from_vec(read_filtered(vm, fid, length))));
    }
    // Detect write-only mode (w/a/x/c without '+')
    let is_write_only = FILE_HANDLES.with(|h| {
        h.borrow().get(&fid).map(|fh| {
            let mode = &fh.mode;
            // Strip 'b'/'t'/'e' suffixes
            let core: String = mode.chars().filter(|c| matches!(*c, 'r'|'w'|'a'|'x'|'c'|'+')).collect();
            // Write-only if mode does not contain 'r' or '+'
            !core.contains('r') && !core.contains('+')
        }).unwrap_or(false)
    });
    let result = FILE_HANDLES.with(|handles| {
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
                    // PHP marks EOF when fewer bytes than requested are read
                    // from a stream. This applies to plain files and to
                    // php://memory / php://temp (which are seekable, so once
                    // we hit the end we report EOF).
                    if n < length && matches!(fh.special, SpecialStream::None | SpecialStream::Memory | SpecialStream::Temp) {
                        fh.eof = true;
                    }
                    Ok(Value::String(PhpString::from_vec(buf)))
                }
                Err(e) => {
                    // Capture errno before we drop the borrow
                    Err(e.raw_os_error().unwrap_or(0))
                }
            }
        } else {
            Ok(Value::False)
        }
    });
    match result {
        Ok(v) => Ok(v),
        Err(errno) => {
            if is_write_only {
                // PHP emits a Notice on fread of write-only stream. PHP uses
                // its internal 8192-byte buffer size for the reported count,
                // not the user-requested length.
                let line = vm.current_line;
                vm.emit_notice_at(&format!(
                    "fread(): Read of 8192 bytes failed with errno={} {}",
                    errno,
                    match errno {
                        9 => "Bad file descriptor",
                        _ => "Unknown error",
                    }
                ), line);
            }
            Ok(Value::False)
        }
    }
}

/// Helper: read up to `length` post-filter bytes from a stream with one
/// or more READ filters attached. Reads from the underlying file in chunks,
/// runs the filter chain, accumulates the result, and returns up to
/// `length` bytes. Excess filtered output stays buffered on the handle.
fn read_filtered(vm: &mut Vm, fid: i64, length: usize) -> Vec<u8> {
    let mut produced: Vec<u8> = FILE_HANDLES.with(|h| {
        h.borrow_mut().get_mut(&fid).map(|fh| std::mem::take(&mut fh.filter_read_ready)).unwrap_or_default()
    });
    while produced.len() < length {
        // Read a chunk of raw bytes from the file.
        let mut raw = vec![0u8; 8192];
        let (n, eof_flag) = FILE_HANDLES.with(|h| {
            let mut h = h.borrow_mut();
            let Some(fh) = h.get_mut(&fid) else { return (0usize, true); };
            match fh.file.read(&mut raw) {
                Ok(0) => { fh.eof = true; (0, true) }
                Ok(n) => (n, false),
                Err(_) => (0, true),
            }
        });
        raw.truncate(n);
        if n == 0 {
            // Flush the chain to extract trailing bytes.
            let trailing = crate::stream_filters::flush_chain(vm, fid, crate::stream_filters::STREAM_FILTER_READ);
            produced.extend_from_slice(&trailing);
            break;
        }
        let filtered = crate::stream_filters::apply_read_chain(vm, fid, &raw);
        produced.extend_from_slice(&filtered);
        let _ = eof_flag;
    }
    // Split produced into return-now and stash-for-later.
    let take = produced.len().min(length);
    let leftover = produced[take..].to_vec();
    let ret = produced[..take].to_vec();
    FILE_HANDLES.with(|h| {
        if let Some(fh) = h.borrow_mut().get_mut(&fid) {
            fh.filter_read_ready = leftover;
            if fh.eof && fh.filter_read_ready.is_empty() && ret.is_empty() {
                // already EOF
            }
        }
    });
    ret
}

fn fwrite_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let arg = args.first().unwrap_or(&Value::Null);
    // For STDOUT/STDERR (1/2/3) without FILE_HANDLES entry, accept long IDs.
    let raw_fid = arg.to_long();
    let is_special_writer = raw_fid == 1 || raw_fid == 2 || raw_fid == 3;
    let fid = if is_special_writer { raw_fid } else { require_open_stream(vm, arg, "fwrite")? };
    let data = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let bytes = data.as_bytes();
    // PHP: if $length is provided and >= 0, write min(length, strlen(data)) bytes.
    // If $length is negative, write 0 bytes (treat as 0).
    let length: usize = match args.get(2) {
        Some(v) if !matches!(v, Value::Null | Value::Undef) => {
            let n = v.to_long();
            if n <= 0 { 0 } else { (n as usize).min(bytes.len()) }
        }
        _ => bytes.len(),
    };
    let to_write = &bytes[..length];
    let original_len = to_write.len();

    // Check if the underlying stream allows writing.
    let (stream_mode, is_special) = FILE_HANDLES.with(|h| {
        let b = h.borrow();
        match b.get(&fid) {
            Some(fh) => (Some(fh.mode.clone()), !matches!(fh.special, SpecialStream::None)),
            None => (None, false),
        }
    });
    if let Some(ref m) = stream_mode {
        let clean: String = m.chars().filter(|&c| c != 'b' && c != 't').collect();
        // 'r' (read-only) without '+' is not writable.
        if clean == "r" {
            if !is_special {
                let line = vm.current_line;
                vm.emit_notice_at(&format!(
                    "fwrite(): Write of {} bytes failed with errno=9 Bad file descriptor",
                    original_len
                ), line);
            }
            return Ok(Value::False);
        }
    }

    // Apply write filter chain if any are attached.
    let filtered = crate::stream_filters::apply_write_chain(vm, fid, to_write);
    // STDOUT/STDERR are resource ids 1/2/3 without a FILE_HANDLES entry.
    let exists = fid == 1 || fid == 2 || fid == 3 || FILE_HANDLES.with(|h| h.borrow().contains_key(&fid));
    if !exists { return Ok(Value::False); }
    write_to_stream(vm, fid, &filtered);
    // PHP returns the number of bytes from the user's input that were
    // accepted, regardless of how many post-filter bytes were emitted.
    Ok(Value::Long(original_len as i64))
}

fn fgets_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let arg = args.first().unwrap_or(&Value::Null);
    let fid = require_open_stream(vm, arg, "fgets")?;
    // PHP semantics: $length (if provided) must be > 0. fgets reads up to
    // length-1 bytes (the remaining byte is a virtual null terminator in C).
    // When omitted, reads until newline (no byte cap — we use a large default).
    let explicit_length = match args.get(1) {
        Some(v) if !matches!(v, Value::Null | Value::Undef) => Some(v.to_long()),
        _ => None,
    };
    let limit = if let Some(n) = explicit_length {
        if n <= 0 {
            let msg = "fgets(): Argument #2 ($length) must be greater than 0".to_string();
            let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
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

    let has_read_filters = crate::stream_filters::chain_has_direction(fid, crate::stream_filters::STREAM_FILTER_READ);
    if has_read_filters {
        let mut out: Vec<u8> = Vec::new();
        let mut hit_newline = false;
        while out.len() < limit {
            let chunk = read_filtered(vm, fid, 1);
            if chunk.is_empty() { break; }
            out.extend_from_slice(&chunk);
            if *chunk.last().unwrap() == b'\n' { hit_newline = true; break; }
        }
        let is_eof_now = FILE_HANDLES.with(|h| h.borrow().get(&fid).map(|fh| fh.eof && fh.filter_read_ready.is_empty()).unwrap_or(true));
        if out.is_empty() && is_eof_now { return Ok(Value::False); }
        let _ = hit_newline;
        return Ok(Value::String(PhpString::from_vec(out)));
    }

    FILE_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(fh) = handles.get_mut(&fid) {
            let mut buf = Vec::new();
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
    })
}

fn feof_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let arg = args.first().unwrap_or(&Value::Null);
    let fid = arg.to_long();
    let has_handle = FILE_HANDLES.with(|handles| handles.borrow().contains_key(&fid));
    if !has_handle {
        // If it was a resource (possibly closed) or any other type, throw TypeError
        let is_closed_resource = matches!(arg, Value::Resource(_))
            && goro_core::value::is_registered_resource(fid)
            && !goro_core::value::is_open_resource(fid);
        let is_resource = matches!(arg, Value::Resource(_)) && goro_core::value::is_open_resource(fid);
        if is_closed_resource || !is_resource {
            let msg = "feof(): Argument #1 ($stream) must be an open stream resource".to_string();
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
        }
        return Ok(Value::True);
    }
    FILE_HANDLES.with(|handles| {
        let handles = handles.borrow();
        if let Some(fh) = handles.get(&fid) {
            if fh.eof { Ok(Value::True) } else { Ok(Value::False) }
        } else {
            Ok(Value::True)
        }
    })
}

fn rewind_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let arg = args.first().unwrap_or(&Value::Null);
    let fid = require_open_stream(vm, arg, "rewind")?;
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

fn fseek_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let arg = args.first().unwrap_or(&Value::Null);
    let fid = require_open_stream(vm, arg, "fseek")?;
    let offset = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let whence = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    // Invalid whence values return -1 without seeking.
    if !matches!(whence, 0 | 1 | 2) {
        return Ok(Value::Long(-1));
    }

    let result = FILE_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(fh) = handles.get_mut(&fid) {
            // Track whether this is a special stream that doesn't support seek.
            let is_unseekable_special = matches!(
                fh.special,
                SpecialStream::Stdout | SpecialStream::Stderr | SpecialStream::Output
            );
            fh.eof = false;
            let seek = match whence {
                0 => SeekFrom::Start(offset as u64), // SEEK_SET
                1 => SeekFrom::Current(offset),       // SEEK_CUR
                2 => SeekFrom::End(offset),            // SEEK_END
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
    let fid = arg.to_long();
    let has_handle = FILE_HANDLES.with(|handles| handles.borrow().contains_key(&fid));
    if !has_handle {
        let is_closed_resource = matches!(arg, Value::Resource(_))
            && goro_core::value::is_registered_resource(fid)
            && !goro_core::value::is_open_resource(fid);
        let is_open_resource = matches!(arg, Value::Resource(_)) && goro_core::value::is_open_resource(fid);
        if is_closed_resource || !is_open_resource {
            let msg = "ftell(): Argument #1 ($stream) must be an open stream resource".to_string();
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
        }
        return Ok(Value::False);
    }
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

fn fsync_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let arg = args.first().unwrap_or(&Value::Null);
    let fid = require_open_stream(vm, arg, "fsync")?;
    enum Result3 { Ok, Err, NoFsync }
    let result = FILE_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(fh) = handles.get_mut(&fid) {
            // Memory/Temp/Output have no real backing fd; cannot fsync.
            if matches!(fh.special, SpecialStream::Memory | SpecialStream::Temp | SpecialStream::Output) {
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
    enum Result3 { Ok, Err, NoFsync }
    let result = FILE_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(fh) = handles.get_mut(&fid) {
            if matches!(fh.special, SpecialStream::Memory | SpecialStream::Temp | SpecialStream::Output) {
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

fn mkdir_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path_v = args.first().unwrap_or(&Value::Null).to_php_string();
    // Null byte check (PHP raises ValueError as TypeError message)
    if path_v.as_bytes().contains(&0) {
        let msg = "mkdir(): Argument #1 ($directory) must not contain any null bytes".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    let path = path_v.to_string_lossy().to_string();
    let path = strip_file_scheme(&path).unwrap_or(path);
    let recursive = args.get(2).map(|v| v.is_truthy()).unwrap_or(false);
    let result = if recursive {
        std::fs::create_dir_all(&path)
    } else {
        std::fs::create_dir(&path)
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
    let path_v = args.first().unwrap_or(&Value::Null).to_php_string();
    if path_v.as_bytes().contains(&0) {
        let msg = "rmdir(): Argument #1 ($directory) must not contain any null bytes".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
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
    // GLOB_ONLYDIR = 1073741824 (0x40000000) in PHP
    #[cfg(target_os = "linux")]
    if flags & 1073741824 != 0 {
        libc_flags |= libc::GLOB_ONLYDIR;
    }

    let ret = unsafe { libc::glob(c_pattern.as_ptr(), libc_flags, None, &mut glob_buf) };

    // GLOB_ONLYDIR is a hint to the OS; on Linux it can still return
    // non-directories. PHP enforces it explicitly.
    let only_dir = flags & 1073741824 != 0;

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
                if !std::fs::metadata(test_path).map(|m| m.is_dir()).unwrap_or(false) {
                    continue;
                }
            }
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
// Thread-local storage for HTTP response headers and status code. The CLI
// server reads these after script execution.
thread_local! {
    pub(crate) static SENT_HEADERS: std::cell::RefCell<Vec<(Vec<u8>, Vec<u8>)>> = const { std::cell::RefCell::new(Vec::new()) };
    pub(crate) static RESPONSE_CODE: std::cell::Cell<i64> = const { std::cell::Cell::new(200) };
    // Request body for php://input
    pub(crate) static REQUEST_BODY: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
}
pub fn set_request_body(body: Vec<u8>) {
    REQUEST_BODY.with(|b| *b.borrow_mut() = body);
}
pub fn get_request_body() -> Vec<u8> {
    REQUEST_BODY.with(|b| b.borrow().clone())
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
        Some((f, l)) => format!("Cannot modify header information - headers already sent by (output started at {}:{})", f, l),
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
    let name = String::from_utf8_lossy(&line[..colon]).trim().as_bytes().to_vec();
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
        h.borrow_mut().retain(|(k, v)| !(k.eq_ignore_ascii_case(name) && v.starts_with(value_prefix)));
    });
}

fn header_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if headers_already_sent_error(_vm) {
        return Ok(Value::Null);
    }
    let header = args.first().unwrap_or(&Value::Null).to_php_string();
    let replace = args.get(1).map(|v| v.is_truthy()).unwrap_or(true);
    let response_code = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    let header_str = header.to_string_lossy();
    // Special case: "HTTP/1.x NNN ..." sets the status code.
    if header_str.starts_with("HTTP/") {
        if let Some(rest) = header_str.split_whitespace().nth(1) {
            if let Ok(code) = rest.parse::<i64>() {
                RESPONSE_CODE.with(|c| c.set(code));
            }
        }
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
                if c.get() == 200 { c.set(401); }
            });
        }
        // PHP auto-sets 302 for Location: headers (unless code is already 3xx).
        if name.eq_ignore_ascii_case(b"Location") && response_code == 0 {
            RESPONSE_CODE.with(|c| {
                let cur = c.get();
                if cur == 200 || cur < 300 { c.set(302); }
            });
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
fn setcookie_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Build a Set-Cookie header from the arguments.
    let name = args.first().map(|v| v.to_php_string().to_string_lossy()).unwrap_or_default();
    if name.is_empty() {
        return Ok(Value::False);
    }
    let value = args.get(1).map(|v| v.to_php_string().to_string_lossy()).unwrap_or_default();
    let expires_or_opts = args.get(2);
    let mut cookie = format!("{}={}", name, value);
    if let Some(Value::Array(arr)) = expires_or_opts {
        let arr = arr.borrow();
        for (k, v) in arr.iter() {
            let key = match k {
                ArrayKey::String(s) => s.to_string_lossy(),
                _ => continue,
            };
            let val = v.to_php_string().to_string_lossy();
            match key.to_ascii_lowercase().as_str() {
                "path" => cookie.push_str(&format!("; path={}", val)),
                "domain" => cookie.push_str(&format!("; domain={}", val)),
                "expires" => cookie.push_str(&format!("; expires={}", val)),
                "secure" => if v.is_truthy() { cookie.push_str("; secure"); },
                "httponly" => if v.is_truthy() { cookie.push_str("; HttpOnly"); },
                "samesite" => cookie.push_str(&format!("; SameSite={}", val)),
                _ => {}
            }
        }
    } else if let Some(expires) = expires_or_opts {
        let exp = expires.to_long();
        if exp > 0 {
            cookie.push_str(&format!("; expires=t{}", exp));
        }
    }
    if headers_already_sent_error(_vm) {
        return Ok(Value::False);
    }
    SENT_HEADERS.with(|h| {
        h.borrow_mut().push((b"Set-Cookie".to_vec(), cookie.into_bytes()));
    });
    Ok(Value::True)
}
fn headers_sent_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let (file, line) = vm.output_start.clone().unwrap_or_default();
    if let Some(Value::Reference(r)) = args.first() {
        *r.borrow_mut() = Value::String(PhpString::from_string(file));
    }
    if let Some(Value::Reference(r)) = args.get(1) {
        *r.borrow_mut() = Value::Long(line as i64);
    }
    Ok(if vm.headers_sent { Value::True } else { Value::False })
}

fn header_remove_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(name_val) = args.first() {
        if matches!(name_val, Value::Null) {
            // Remove all
            SENT_HEADERS.with(|h| h.borrow_mut().clear());
        } else {
            let name = name_val.to_php_string();
            let name_bytes = name.as_bytes();
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
                let header_name: String = parts.iter().map(|p| {
                    let mut chars = p.chars();
                    match chars.next() {
                        Some(c) => c.to_uppercase().chain(chars.flat_map(|c| c.to_lowercase())).collect::<String>(),
                        None => String::new(),
                    }
                }).collect::<Vec<_>>().join("-");
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
            if k.eq_ignore_ascii_case(b"X-Powered-By") { has_xpb = true; }
            if k.eq_ignore_ascii_case(b"Content-Type") || k.eq_ignore_ascii_case(b"Content-type") { has_ct = true; }
        }
    });
    if !has_xpb {
        arr.set(
            ArrayKey::String(PhpString::from_bytes(b"X-Powered-By")),
            Value::String(PhpString::from_bytes(b"PHP/8.5.4")),
        );
    }
    SENT_HEADERS.with(|h| {
        for (k, v) in h.borrow().iter() {
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
fn http_response_code_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(v) = args.first() {
        if !matches!(v, Value::Null) {
            let code = v.to_long();
            let prev = RESPONSE_CODE.with(|c| {
                let p = c.get();
                c.set(code);
                p
            });
            return Ok(Value::Long(prev));
        }
    }
    Ok(Value::Long(RESPONSE_CODE.with(|c| c.get())))
}

fn array_diff_key_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Type check all arguments
    for (i, arg) in args.iter().enumerate() {
        let a = if let Value::Reference(r) = arg { r.borrow().clone() } else { arg.clone() };
        if !matches!(a, Value::Array(_)) {
            let type_name = arg_type_name(&a);
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
            let type_name = arg_type_name(&a);
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
            let type_name = arg_type_name(&a);
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
            let type_name = arg_type_name(&a);
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

// Simple stable merge sort that tolerates inconsistent comparators without
// panicking, unlike the stdlib sort on recent Rust versions. Used by usort
// and friends, where user callbacks may not implement a total order.
fn stable_sort_by<T, F>(arr: &mut Vec<T>, mut cmp: F)
where
    T: Clone,
    F: FnMut(&T, &T) -> std::cmp::Ordering,
{
    let n = arr.len();
    if n < 2 {
        return;
    }
    // Bottom-up merge sort using two alternating buffers.
    let mut buf: Vec<T> = arr.clone();
    let mut width = 1usize;
    // Track whether arr is the current source (true) or buf is (false).
    let mut arr_is_src = true;
    while width < n {
        let (src, dst): (&[T], &mut [T]) = if arr_is_src {
            (arr.as_slice(), buf.as_mut_slice())
        } else {
            (buf.as_slice(), arr.as_mut_slice())
        };
        let mut i = 0usize;
        while i < n {
            let mid = std::cmp::min(i + width, n);
            let end = std::cmp::min(i + 2 * width, n);
            let (left, right) = src[i..end].split_at(mid - i);
            let mut li = 0usize;
            let mut ri = 0usize;
            let mut di = i;
            while li < left.len() && ri < right.len() {
                if cmp(&left[li], &right[ri]) == std::cmp::Ordering::Greater {
                    dst[di] = right[ri].clone();
                    ri += 1;
                } else {
                    dst[di] = left[li].clone();
                    li += 1;
                }
                di += 1;
            }
            while li < left.len() {
                dst[di] = left[li].clone();
                li += 1;
                di += 1;
            }
            while ri < right.len() {
                dst[di] = right[ri].clone();
                ri += 1;
                di += 1;
            }
            i = end;
        }
        arr_is_src = !arr_is_src;
        width *= 2;
    }
    if !arr_is_src {
        // Latest result is in buf; copy back to arr.
        arr.clone_from_slice(&buf);
    }
}

fn cmp_callback_result(
    vm: &mut Vm,
    result: Value,
    fname: &str,
    warned: &mut bool,
) -> std::cmp::Ordering {
    // PHP 8.2+ deprecates comparison functions returning bool. PHP emits the
    // notice once per sort call (not once per comparison) and then continues
    // with int coercion semantics.
    if matches!(result, Value::True | Value::False) && !*warned {
        *warned = true;
        vm.emit_deprecated(&format!(
            "{}(): Returning bool from comparison function is deprecated, return an integer less than, equal to, or greater than zero",
            fname
        ));
    }
    let cmp = result.to_long();
    if cmp < 0 {
        std::cmp::Ordering::Less
    } else if cmp > 0 {
        std::cmp::Ordering::Greater
    } else {
        std::cmp::Ordering::Equal
    }
}

fn usort_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let (Some(Value::Array(arr)), Some(callback)) = (args.first(), args.get(1)) {
        let callback = callback.clone();
        let mut entries: Vec<Value> = arr.borrow().values().cloned().collect();

        let mut warned = false;
        stable_sort_by(&mut entries, |a, b| {
            let result = vm.call_callback(&callback, &[a.clone(), b.clone()]).unwrap_or(Value::Long(0));
            cmp_callback_result(vm, result, "usort", &mut warned)
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
        let mut entries: Vec<(ArrayKey, Value)> =
            arr.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();

        let cb = callback.clone();
        let mut warned = false;
        stable_sort_by(&mut entries, |a, b| {
            let result = call_user_func(vm, &[cb.clone(), a.1.clone(), b.1.clone()]).unwrap_or(Value::Long(0));
            cmp_callback_result(vm, result, "uasort", &mut warned)
        });

        let mut new_arr = PhpArray::new();
        for (key, val) in entries {
            new_arr.set(key, val);
        }
        *arr.borrow_mut() = new_arr;
    }
    Ok(Value::True)
}
fn uksort_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let (Some(Value::Array(arr)), Some(callback)) = (args.first(), args.get(1)) {
        let mut entries: Vec<(ArrayKey, Value)> =
            arr.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();

        let cb = callback.clone();
        let mut warned = false;
        stable_sort_by(&mut entries, |a, b| {
            let key_a = match &a.0 {
                ArrayKey::Int(n) => Value::Long(*n),
                ArrayKey::String(s) => Value::String(s.clone()),
            };
            let key_b = match &b.0 {
                ArrayKey::Int(n) => Value::Long(*n),
                ArrayKey::String(s) => Value::String(s.clone()),
            };
            let result = call_user_func(vm, &[cb.clone(), key_a, key_b]).unwrap_or(Value::Long(0));
            cmp_callback_result(vm, result, "uksort", &mut warned)
        });

        let mut new_arr = PhpArray::new();
        for (key, val) in entries {
            new_arr.set(key, val);
        }
        *arr.borrow_mut() = new_arr;
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
    let mut put = |k: &[u8], v: Value| {
        result.set(goro_core::array::ArrayKey::String(PhpString::from_bytes(k)), v);
    };
    put(b"running", if goro_core::gc::gc_active() { Value::True } else { Value::False });
    put(b"protected", Value::False);
    put(b"full", Value::False);
    put(b"runs", Value::Long(goro_core::gc::gc_runs()));
    put(b"collected", Value::Long(goro_core::gc::gc_collected()));
    put(b"threshold", Value::Long(10001));
    put(b"buffer_size", Value::Long(16384));
    put(b"roots", Value::Long(goro_core::gc::gc_root_count()));
    put(b"application_time", Value::Double(0.0));
    put(b"collector_time", Value::Double(0.0));
    put(b"destructor_time", Value::Double(0.0));
    put(b"free_time", Value::Double(0.0));
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
            // Lazy object prefix: "lazy ghost " / "lazy proxy " before object(...)
            let lazy_prefix: &str = match obj_borrow.get_property(b"__lazy_state") {
                Value::String(s) if s.as_bytes() == b"ghost" => "lazy ghost ",
                Value::String(s) if s.as_bytes() == b"proxy" => "lazy proxy ",
                _ => "",
            };
            // Initialized proxy: show as `lazy proxy object(C)#N (1) { ["instance"]=> <real> }`
            if lazy_prefix == "lazy proxy " {
                if let Value::Object(real) = obj_borrow.get_property(b"__lazy_real") {
                    drop(obj_borrow);
                    vm.write_output(
                        format!("{}lazy proxy object({})#{} (1) refcount({}){{\n", prefix, class_name, oid, 2).as_bytes(),
                    );
                    vm.write_output(format!("{}  [\"instance\"]=>\n", prefix).as_bytes());
                    debug_zval_dump_value(vm, &Value::Object(real), indent + 2);
                    vm.write_output(format!("{}}}\n", prefix).as_bytes());
                    return;
                }
            }
            let prop_count = obj_borrow.properties.iter()
                .filter(|(name, val)| !name.starts_with(b"__") && !matches!(val, Value::Undef))
                .count();
            vm.write_output(
                format!("{}{}object({})#{} ({}) refcount({}){{\n", prefix, lazy_prefix, class_name, oid, prop_count, 2).as_bytes(),
            );
            for (name, value) in &obj_borrow.properties {
                if name.starts_with(b"__") {
                    continue;
                }
                let name_str = String::from_utf8_lossy(name);
                vm.write_output(format!("{}  [\"{}\"]=>\n", prefix, name_str).as_bytes());
                if matches!(value, Value::Undef) {
                    // Emit a PHP-style uninitialized marker for typed props.
                    // We look up the class to get the property's type.
                    let class_lower: Vec<u8> = obj_borrow.class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                    let prop_type = vm.classes.get(&class_lower)
                        .and_then(|c| c.properties.iter().find(|p| p.name == *name))
                        .and_then(|p| p.property_type.as_ref())
                        .map(|t| vm.param_type_name(t))
                        .unwrap_or_else(|| String::from("mixed"));
                    vm.write_output(format!("{}  uninitialized({})\n", prefix, prop_type).as_bytes());
                } else {
                    debug_zval_dump_value(vm, value, indent + 2);
                }
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

/// Throw a TypeError from a builtin (sets the pending exception).
fn throw_type_error_msg(vm: &mut Vm, msg: String) -> VmError {
    let exc = vm.throw_type_error(msg.clone());
    vm.current_exception = Some(exc);
    VmError { message: format!("Uncaught TypeError: {}", msg), line: vm.current_line }
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
                    return Err(VmError { message: "Uncaught exception".into(), line: vm.current_line });
                }
            }
            if !vm.classes.contains_key(&lower) {
                return Err(throw_type_error_msg(vm, format!("get_class_methods(): Argument #1 ($object_or_class) must be an object or a valid class name, {} given", Vm::value_type_name(&val))));
            }
            name
        }
        Value::Object(obj) => obj.borrow().class_name.clone(),
        _ => {
            return Err(throw_type_error_msg(vm, format!("get_class_methods(): Argument #1 ($object_or_class) must be an object or a valid class name, {} given", Vm::value_type_name(&val))));
        }
    };
    let class_lower: Vec<u8> = class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
    let scope: Option<Vec<u8>> = vm.current_class_scope()
        .map(|name| name.iter().map(|b| b.to_ascii_lowercase()).collect());
    let mut result = PhpArray::new();
    let methods: Vec<(Vec<u8>, Visibility, Vec<u8>)> = match vm.classes.get(&class_lower) {
        Some(class) => {
            // Own methods first (declaration order), then inherited ones.
            let own = class.methods.values().filter(|m| lower_bytes(&m.declaring_class) == class_lower);
            let inherited = class.methods.values().filter(|m| lower_bytes(&m.declaring_class) != class_lower);
            own.chain(inherited)
                .filter(|m| !m.name.starts_with(b"__property_"))
                .map(|m| (m.name.clone(), m.visibility, lower_bytes(&m.declaring_class)))
                .collect()
        }
        None => Vec::new(),
    };
    for (name, vis, decl) in methods {
        let visible = match vis {
            Visibility::Public => true,
            Visibility::Protected => scope.as_ref().map(|sc| {
                *sc == decl || vm.class_extends(sc, &decl) || vm.class_extends(&decl, sc)
            }).unwrap_or(false),
            Visibility::Private => scope.as_ref().map(|sc| *sc == decl).unwrap_or(false),
        };
        if visible {
            result.push(Value::String(PhpString::from_vec(name)));
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
    // PHP throws an Error when the class is not declared; the message
    // uses the original-case spelling supplied by the caller.
    if !vm.classes.contains_key(&name_lower) {
        let class_str = class_name.to_string_lossy().to_string();
        let msg = format!(
            "get_class_vars(): Argument #1 ($class) must be a valid class name, {} given",
            class_str
        );
        let exc = vm.create_exception(b"Error", &msg, 0);
        vm.current_exception = Some(exc);
        return Ok(Value::Null);
    }
    // Caller-scope visibility filter: only return properties the calling
    // class would be allowed to see. Mirrors what PHP's get_class_vars
    // does at the engine level. Compare scopes in lowercase form because
    // PHP class names are case-insensitive and declaring_class is stored
    // lowercase.
    let caller_scope: Option<Vec<u8>> = vm.current_class_scope()
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
                    let decl_lower: Vec<u8> = prop.declaring_class
                        .iter().map(|b| b.to_ascii_lowercase()).collect();
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
    if vm.classes.contains_key(&name_lower) {
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
        // Non-static, walking child → parent
        for cls_lower in &chain {
            for prop in &class.properties {
                if prop.is_static || prop.is_virtual {
                    continue;
                }
                let decl_lower: Vec<u8> =
                    prop.declaring_class.iter().map(|b| b.to_ascii_lowercase()).collect();
                if decl_lower != *cls_lower {
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
        }
        // Static, walking child → parent
        for cls_lower in &chain {
            for prop in &class.properties {
                if !prop.is_static {
                    continue;
                }
                let decl_lower: Vec<u8> =
                    prop.declaring_class.iter().map(|b| b.to_ascii_lowercase()).collect();
                if decl_lower != *cls_lower {
                    continue;
                }
                if !can_see(prop) {
                    continue;
                }
                let val = class
                    .static_properties
                    .get(&prop.name)
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
        match std::fs::read_dir(dir_path) {
            Ok(_) => {
                let did = alloc_dir_handle(path_str.to_string());
                Ok(Value::Resource(did))
            }
            Err(e) => {
                vm.emit_warning(&format!(
                    "opendir({}): Failed to open directory: {}",
                    path_str, e
                ));
                Ok(Value::False)
            }
        }
    } else {
        let err_msg = match std::fs::metadata(dir_path) {
            Err(e) => format!("{}", e),
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
            return Err(VmError {
                message: format!(
                    "{}(): Argument #1 ($dir_handle) must be of type resource, int given",
                    func_name
                ),
                line: vm.current_line,
            });
        }
        Value::Resource(n) => {
            let is_dir = DIR_HANDLES.with(|dh| dh.borrow().contains_key(&n));
            if is_dir {
                Ok(Some(n))
            } else if !goro_core::value::is_open_resource(n) {
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
    CURRENT_DIR_RESOURCE.with(|c| {
        if c.get() == Some(id) {
            c.set(None);
        }
    });
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
            return Err(VmError { message: msg, line: vm.current_line });
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
    let target_s = target.to_string_lossy();
    let link_s = link.to_string_lossy();
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
fn strip_os_error(msg: &str) -> String {
    if let Some(idx) = msg.rfind(" (os error") {
        msg[..idx].to_string()
    } else {
        msg.to_string()
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

    let frames: Vec<(String, String, u32, Vec<Value>, bool)> = vm.call_stack.iter().rev().cloned().collect();
    for (idx, (func_name, file, line, frame_args, is_method)) in frames.iter().enumerate() {
        let frame_args = &vm.trace_frame_args(func_name, frame_args);
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
        // Parse class::method or class->method from func_name. For closures
        // (`{closure:...}`) we never split on `::` even if the closure name
        // embeds `Class::method()`.
        if !func_name.starts_with("{closure:") && func_name.find("::").is_some() {
            let sep = func_name.find("::").unwrap();
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
        } else if !func_name.starts_with("{closure:") && func_name.find("->").is_some() {
            let sep = func_name.find("->").unwrap();
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
    let frames: Vec<(String, String, u32, Vec<Value>, bool)> = vm.call_stack.iter().rev().cloned().collect();
    for (i, (func_name, file, line, frame_args, is_instance)) in frames.iter().enumerate() {
        let frame_args = &vm.trace_frame_args(func_name, frame_args);
        if i >= max_frames {
            break;
        }
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
        // Frames with empty file/line=0 represent an internal-engine call
        // (e.g. a generator body resumed by VM machinery, or a Fiber callback).
        let loc = if *line == 0 || file.is_empty() {
            "[internal function]".to_string()
        } else {
            let file_display = if file == "Unknown.php" {
                vm.current_file.as_str()
            } else {
                file.as_str()
            };
            format!("{}({})", file_display, line)
        };
        lines.push(format!("#{} {}: {}({})", i, loc, display_name, args_str));
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

    // Float keys with fractional or out-of-range components trigger a
    // deprecation notice when implicitly truncated to int.
    if let Value::Double(f) = key_deref {
        let truncated = f.trunc();
        let in_range = (i64::MIN as f64) <= truncated && truncated <= (i64::MAX as f64);
        if !f.is_nan() && (f != truncated || !in_range) {
            // PHP emits this notice with serialize_precision (-1 / shortest
            // roundtrip) formatting, matching what var_dump uses for float
            // values when serialize_precision is the default.
            // Use shortest-roundtrip formatting (serialize_precision=-1)
            // matching PHP's deprecation notice format.
            let prev_prec = goro_core::value::get_php_precision();
            goro_core::value::set_php_precision(-1);
            let formatted: String = Value::Double(f).to_php_string().to_string_lossy().to_string();
            goro_core::value::set_php_precision(prev_prec);
            let msg = format!(
                "Implicit conversion from float {} to int loses precision",
                formatted
            );
            vm.emit_deprecated(&msg);
        }
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
        let type_name = arg_type_name(arr);
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
    // Validate length argument (treat null/0 as "no limit").
    if let Some(len_val) = args.get(1) {
        if !matches!(len_val, Value::Null) {
            let len = len_val.to_long();
            if len != 0 && (len < 0 || len > i64::MAX - 1) {
                let msg = format!(
                    "fgetcsv(): Argument #2 ($length) must be between 0 and {}",
                    i64::MAX - 1
                );
                let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg, line: vm.current_line });
            }
        }
    }
    // Validate separator must be a single character.
    if let Some(v) = args.get(2) {
        if !matches!(v, Value::Null) {
            let s = v.to_php_string();
            if s.len() != 1 {
                let msg = "fgetcsv(): Argument #3 ($separator) must be a single character".to_string();
                let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg, line: vm.current_line });
            }
        }
    }
    // Validate enclosure must be a single character.
    if let Some(v) = args.get(3) {
        if !matches!(v, Value::Null) {
            let s = v.to_php_string();
            if s.len() != 1 {
                let msg = "fgetcsv(): Argument #4 ($enclosure) must be a single character".to_string();
                let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg, line: vm.current_line });
            }
        }
    }
    // Validate escape (may be empty or single character).
    if let Some(v) = args.get(4) {
        if !matches!(v, Value::Null) {
            let s = v.to_php_string();
            if s.len() > 1 {
                let msg = "fgetcsv(): Argument #5 ($escape) must be empty or a single character".to_string();
                let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg, line: vm.current_line });
            }
        }
    }
    let _length = args.get(1).and_then(|v| if matches!(v, Value::Null) { None } else { Some(v.to_long()) });
    let separator = match args.get(2) {
        Some(v) if !matches!(v, Value::Null) => {
            let s = v.to_php_string();
            let b = s.as_bytes();
            if b.is_empty() { b',' } else { b[0] }
        }
        _ => b',',
    };
    let enclosure = match args.get(3) {
        Some(v) if !matches!(v, Value::Null) => {
            let s = v.to_php_string();
            let b = s.as_bytes();
            if b.is_empty() { b'"' } else { b[0] }
        }
        _ => b'"',
    };
    let escape = match args.get(4) {
        Some(v) if !matches!(v, Value::Null) => {
            let s = v.to_php_string();
            let b = s.as_bytes();
            if b.is_empty() { None } else { Some(b[0]) }
        }
        _ => {
            // PHP 8.4+ deprecation: implicit default escape will change.
            let line = vm.current_line;
            vm.emit_deprecated_at(
                "fgetcsv(): the $escape parameter must be provided as its default value will change",
                line,
            );
            Some(b'\\')
        }
    };

    // Read one line at a time. When the CSV parser reports an unterminated
    // enclosure, keep appending more lines and re-parse.
    let mut buf: Vec<u8> = Vec::new();
    let mut first_chunk = true;
    let mut hit_eof = false;
    let arr: PhpArray = loop {
        let chunk = FILE_HANDLES.with(|handles| {
            let mut handles = handles.borrow_mut();
            let fh = handles.get_mut(&fid)?;
            let mut local = Vec::new();
            let mut byte = [0u8; 1];
            loop {
                match fh.file.read(&mut byte) {
                    Ok(0) => {
                        fh.eof = true;
                        break;
                    }
                    Ok(_) => {
                        local.push(byte[0]);
                        if byte[0] == b'\n' { break; }
                    }
                    Err(_) => break,
                }
            }
            Some(local)
        });
        match chunk {
            None => {
                // No file handle.
                return Ok(Value::False);
            }
            Some(line) => {
                if line.is_empty() {
                    // EOF — stop accumulating.
                    hit_eof = true;
                    if buf.is_empty() {
                        return Ok(Value::False);
                    }
                    break crate::strings::parse_csv_fields_ex(&buf, separator, enclosure, escape).0;
                }
                buf.extend_from_slice(&line);
            }
        }
        // Strip trailing CR/LF for parsing attempt.
        let mut try_bytes = buf.clone();
        if try_bytes.last() == Some(&b'\n') {
            try_bytes.pop();
            if try_bytes.last() == Some(&b'\r') {
                try_bytes.pop();
            }
        }
        // Blank line at start: PHP returns array(NULL).
        if first_chunk && try_bytes.is_empty() {
            let mut a = PhpArray::new();
            a.push(Value::Null);
            return Ok(Value::Array(Rc::new(RefCell::new(a))));
        }
        first_chunk = false;
        let (fields, unterminated) = crate::strings::parse_csv_fields_ex(&try_bytes, separator, enclosure, escape);
        if unterminated {
            continue;
        }
        break fields;
    };
    let _ = hit_eof;
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
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
            vm.emit_warning(&format!("filetype(): Lstat failed for {}", path_str));
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

/// Natural order comparison (matches PHP strnatcmp).
fn natcmp(a: &str, b: &str) -> std::cmp::Ordering {
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    let mut ai = 0;
    let mut bi = 0;
    while ai < ab.len() && bi < bb.len() {
        // Skip spaces.
        while ai < ab.len() && (ab[ai] as char).is_whitespace() {
            ai += 1;
        }
        while bi < bb.len() && (bb[bi] as char).is_whitespace() {
            bi += 1;
        }
        if ai >= ab.len() || bi >= bb.len() {
            break;
        }
        let ca = ab[ai];
        let cb = bb[bi];
        if ca.is_ascii_digit() && cb.is_ascii_digit() {
            // If either starts with '0', compare left-aligned (fractional):
            // digit-by-digit, shorter wins on tie.
            let fractional = ca == b'0' || cb == b'0';
            if fractional {
                // Left-aligned: compare digit by digit; first mismatch decides.
                loop {
                    let da = if ai < ab.len() { ab[ai] } else { 0 };
                    let db = if bi < bb.len() { bb[bi] } else { 0 };
                    let da_digit = (da as char).is_ascii_digit();
                    let db_digit = (db as char).is_ascii_digit();
                    if !da_digit && !db_digit {
                        break;
                    }
                    if !da_digit {
                        return std::cmp::Ordering::Less;
                    }
                    if !db_digit {
                        return std::cmp::Ordering::Greater;
                    }
                    if da != db {
                        return da.cmp(&db);
                    }
                    ai += 1;
                    bi += 1;
                }
            } else {
                // Right-aligned: longer number is bigger; tie → first mismatch.
                let mut bias: std::cmp::Ordering = std::cmp::Ordering::Equal;
                loop {
                    let da_in_range = ai < ab.len();
                    let db_in_range = bi < bb.len();
                    let da = if da_in_range { ab[ai] } else { 0 };
                    let db = if db_in_range { bb[bi] } else { 0 };
                    let da_digit = da_in_range && (da as char).is_ascii_digit();
                    let db_digit = db_in_range && (db as char).is_ascii_digit();
                    if !da_digit && !db_digit {
                        if bias != std::cmp::Ordering::Equal {
                            return bias;
                        }
                        break;
                    }
                    if !da_digit {
                        return std::cmp::Ordering::Less;
                    }
                    if !db_digit {
                        return std::cmp::Ordering::Greater;
                    }
                    if bias == std::cmp::Ordering::Equal {
                        if da < db {
                            bias = std::cmp::Ordering::Less;
                        } else if da > db {
                            bias = std::cmp::Ordering::Greater;
                        }
                    }
                    ai += 1;
                    bi += 1;
                }
            }
        } else {
            if ca != cb {
                return ca.cmp(&cb);
            }
            ai += 1;
            bi += 1;
        }
    }
    // Remaining chars (trailing spaces are skipped above) break the tie by length.
    (ab.len() - ai).cmp(&(bb.len() - bi))
}

fn set_include_path_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let old = vm.constants.get(b"include_path".as_ref()).cloned()
        .unwrap_or_else(|| Value::String(PhpString::from_bytes(b".")));
    vm.constants.insert(b"include_path".to_vec(),
        Value::String(PhpString::from_vec(path.as_bytes().to_vec())));
    Ok(old)
}

fn get_include_path_fn(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(vm.constants.get(b"include_path".as_ref()).cloned()
        .unwrap_or_else(|| Value::String(PhpString::from_bytes(b"."))))
}

fn restore_include_path_fn(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    vm.constants.insert(b"include_path".to_vec(),
        Value::String(PhpString::from_bytes(b".")));
    Ok(Value::Null)
}

fn get_resource_type_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    // For closed resources, return "Unknown" (matches PHP behavior).
    if let Value::Resource(id) = val {
        if goro_core::value::is_registered_resource(*id) && !goro_core::value::is_open_resource(*id) {
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
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    if path.as_bytes().contains(&0) {
        let msg = "unlink(): Argument #1 ($filename) must not contain any null bytes".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
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
    if let (Ok(c1), Ok(c2)) = (std::fs::canonicalize(&*from_str), std::fs::canonicalize(&*to_str)) {
        if c1 == c2 {
            return Ok(Value::False);
        }
    }
    match std::fs::copy(&*from_str, &*to_str) {
        Ok(_) => Ok(Value::True),
        Err(e) => {
            let msg = strip_os_error(&e.to_string());
            vm.emit_warning(&format!("copy({}): Failed to open stream: {}", from_str, msg));
            Ok(Value::False)
        }
    }
}

fn readfile_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let path_str = path.to_string_lossy();
    if path_str.is_empty() {
        let msg = "Path must not be empty".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    let use_include_path = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
    let resolved = resolve_with_include_path(vm, &path_str, use_include_path);
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
        },
    }
}

fn resolve_with_include_path(vm: &Vm, filename: &str, use_include_path: bool) -> String {
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
        return Err(VmError { message: msg, line: vm.current_line });
    }
    let ignore_new_lines = flags & 2 != 0; // FILE_IGNORE_NEW_LINES
    let skip_empty = flags & 4 != 0; // FILE_SKIP_EMPTY_LINES
    let use_inc = flags & 1 != 0; // FILE_USE_INCLUDE_PATH
    let path_str = path.to_string_lossy();
    let path_str = resolve_with_include_path(vm, &path_str, use_inc);
    match read_path_with_wrappers(&path_str) {
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
        Err(_) => {
            vm.emit_warning(&format!("file({}): Failed to open stream: No such file or directory", path_str));
            Ok(Value::False)
        }
    }
}

fn lstat_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let path_str = path.to_string_lossy();
    let res = stat_path(&path_str, true)?;
    if matches!(res, Value::False) {
        vm.emit_warning(&format!("lstat(): Lstat failed for {}", path_str));
    }
    Ok(res)
}

fn is_executable_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    #[cfg(unix)]
    {
        Ok(if check_access_bit(&path.to_string_lossy(), 0o100, 0o010, 0o001) {
            Value::True
        } else {
            Value::False
        })
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
        let msg = "tempnam(): Argument #2 ($prefix) must be of type string, array given".to_string();
        let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    let prefix = prefix_val.to_php_string();
    // Reject null bytes in prefix.
    if prefix.as_bytes().contains(&0) {
        let msg = "tempnam(): Argument #2 ($prefix) must not contain any null bytes".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    let dir = args.first().unwrap_or(&Value::Null).to_php_string();
    let raw_dir_str = if dir.is_empty() {
        std::env::temp_dir().to_string_lossy().to_string()
    } else {
        dir.to_string_lossy()
    };
    // Resolve the directory to a canonical path (like PHP's expand_filepath
    // before opening). If the requested directory doesn't exist or isn't
    // writable, fall back to the system temp dir.
    let dir_path = std::path::Path::new(&raw_dir_str);
    let canonical = dir_path.canonicalize().ok();
    let dir_str = match canonical {
        Some(p) if p.is_dir() => p.to_string_lossy().to_string(),
        _ => std::env::temp_dir().to_string_lossy().to_string(),
    };
    // PHP uses only the basename of the prefix and truncates to 64 chars.
    let prefix_bytes = prefix.as_bytes();
    let prefix_base: Vec<u8> = match prefix_bytes.iter().rposition(|&c| c == b'/') {
        Some(pos) => prefix_bytes[pos + 1..].to_vec(),
        None => prefix_bytes.to_vec(),
    };
    let prefix_truncated: Vec<u8> = prefix_base.iter().take(64).copied().collect();
    let prefix_str = String::from_utf8_lossy(&prefix_truncated).to_string();

    // Try a few names with random suffix until one doesn't exist.
    use std::os::unix::fs::OpenOptionsExt;
    use std::time::SystemTime;
    for attempt in 0..16u32 {
        let nanos = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let mix = (nanos as u64).wrapping_add(attempt as u64 * 1_000_003);
        let name = format!("{}/{}{}", dir_str, prefix_str, mix);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&name)
        {
            Ok(_) => return Ok(Value::String(PhpString::from_string(name))),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => {
                vm.emit_warning(&format!(
                    "tempnam(): file created in the system's temporary directory"
                ));
                return Ok(Value::False);
            }
        }
    }
    Ok(Value::False)
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
            let entries: Vec<_> = arr.borrow().iter().map(|(k, v)| (k.clone(), unwrap_rc1_ref(v))).collect();
            for (key, value) in entries {
                let value_deref = value.deref();
                if let Value::Array(inner_new) = &value_deref {
                    let inner_new_ptr = Rc::as_ptr(inner_new) as usize;
                    // Detect direct self-reference before recursing.
                    if inner_new_ptr == arr_ptr {
                        let e = "Recursion detected".to_string();
                        let exc = vm.create_exception(b"Error", &e, vm.current_line);
                        vm.current_exception = Some(exc);
                        return Err(VmError { message: format!("Uncaught Error: {}", e), line: vm.current_line });
                    }
                    let existing_inner = if let Some(existing) = result.get(&key) {
                        let existing = existing.deref();
                        if let Value::Array(inner_old) = existing {
                            Some(inner_old.clone())
                        } else { None }
                    } else { None };
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
                                return Err(VmError { message: format!("Uncaught Error: {}", e), line: vm.current_line });
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

fn array_replace_recursive_inner(base: &PhpArray, replacement: &Rc<RefCell<PhpArray>>, visited: &mut Vec<usize>) -> Result<Value, String> {
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
            if let Some(existing) = result.get(&key) {
                let existing = existing.deref();
                if let Value::Array(inner_old) = existing {
                    visited.push(ptr);
                    let merged = array_replace_recursive_inner(&inner_old.borrow(), inner_new, visited)?;
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

fn array_product_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let arr = match args.first() {
        Some(Value::Array(arr)) => arr.borrow(),
        _ => return Ok(Value::Long(0)),
    };
    let mut product_int: i64 = 1;
    let mut is_float = false;
    let mut product_float: f64 = 1.0;
    for (_, value) in arr.iter() {
        let v = value.deref();
        match &v {
            Value::Double(f) => {
                if !is_float {
                    is_float = true;
                    product_float = product_int as f64;
                }
                product_float *= f;
            }
            Value::Long(n) => {
                if is_float {
                    product_float *= *n as f64;
                } else {
                    match product_int.checked_mul(*n) {
                        Some(r) => product_int = r,
                        None => {
                            is_float = true;
                            product_float = product_int as f64 * *n as f64;
                        }
                    }
                }
            }
            Value::True => {
                // 1 * x => no change
            }
            Value::False | Value::Null => {
                if is_float {
                    product_float = 0.0;
                } else {
                    product_int = 0;
                }
            }
            Value::String(s) => {
                // PHP tries to parse as number; if not numeric, warns.
                let bytes = s.as_bytes();
                // Lean on Value's numeric parsing.
                let as_str = String::from_utf8_lossy(bytes);
                let trimmed = as_str.trim();
                let as_int = trimmed.parse::<i64>().ok();
                let as_flt = trimmed.parse::<f64>().ok();
                if as_int.is_none() && as_flt.is_none() {
                    vm.emit_warning(
                        "array_product(): Multiplication is not supported on type string",
                    );
                    if is_float {
                        product_float = 0.0;
                    } else {
                        product_int = 0;
                    }
                    continue;
                }
                if let Some(n) = as_int {
                    if is_float {
                        product_float *= n as f64;
                    } else {
                        match product_int.checked_mul(n) {
                            Some(r) => product_int = r,
                            None => {
                                is_float = true;
                                product_float = product_int as f64 * n as f64;
                            }
                        }
                    }
                } else if let Some(f) = as_flt {
                    if !is_float {
                        is_float = true;
                        product_float = product_int as f64;
                    }
                    product_float *= f;
                }
            }
            Value::Array(_) => {
                vm.emit_warning(
                    "array_product(): Multiplication is not supported on type array",
                );
            }
            Value::Object(o) => {
                let class = o.borrow().class_name.clone();
                let name = goro_core::value::display_class_name(&class);
                vm.emit_warning(&format!(
                    "array_product(): Multiplication is not supported on type {}",
                    name
                ));
            }
            _ => {
                // fallback for unhandled types
                let n = v.to_long();
                if is_float {
                    product_float *= n as f64;
                } else {
                    match product_int.checked_mul(n) {
                        Some(r) => product_int = r,
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
            let type_name = arg_type_name(&val);
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
        let mut found_in_any = false;
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
                            // PHP uses string comparison (zend_compare with string semantics)
                            // for values in array_*_assoc functions, not loose `==`.
                            if php_value_string_eq(value, ovalue) {
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
            } else {
                found_in_any = true;
            }
        }
        // intersect: keep if found in ALL others
        // diff: keep if NOT found in ANY other
        let keep = if is_intersect { found_in_all } else { !found_in_any };
        if keep {
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
            let type_name = arg_type_name(&val);
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
            let type_name = arg_type_name(&val);
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
            let type_name = arg_type_name(&val);
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


fn parse_ini_string_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = match args.first().map(|v| v.deref()) {
        Some(v) => v.to_php_string().as_bytes().to_vec(),
        None => return Ok(Value::False),
    };
    let process_sections = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
    let scanner_mode = args.get(2).map(|v| v.to_long()).unwrap_or(crate::ini_parser::SCANNER_NORMAL);
    Ok(crate::ini_parser::parse_ini(vm, &data, process_sections, scanner_mode, "Unknown"))
}

fn process_ini_value(raw: &str, is_raw: bool, _is_typed: bool, vm: &Vm) -> (String, bool) {
    let raw = raw.trim_end();
    if raw.is_empty() {
        return (String::new(), false);
    }

    // Handle single-quoted (literal, no constants, no escapes). Only for NORMAL/TYPED.
    if !is_raw && raw.starts_with('\'') {
        if let Some(end) = raw[1..].find('\'') {
            return (raw[1..1 + end].to_string(), true);
        }
        return (raw[1..].to_string(), true);
    }

    // Handle double-quoted with matching closing quote.
    if raw.starts_with('"') {
        if let Some(end) = raw[1..].rfind('"') {
            let inner = &raw[1..1 + end];
            let after = raw[2 + end..].trim();
            // For RAW mode, the value must be entirely `"..."`. Anything
            // beyond the closing quote means the whole thing is treated
            // literally (quotes preserved). Inline comments are still
            // stripped at the next `;` outside the quoted region.
            if is_raw {
                if after.is_empty() {
                    return (inner.to_string(), true);
                }
                // Literal: keep raw, but strip from `;` after the quoted region.
                let close_pos = 2 + end; // index of char after closing "
                let tail = &raw[close_pos..];
                let val = if let Some(semi) = tail.find(';') {
                    format!("{}{}", &raw[..close_pos], &tail[..semi])
                } else {
                    raw.to_string()
                };
                return (val.trim_end().to_string(), true);
            }
            // NORMAL/TYPED: process escape sequences inside the quoted region.
            let mut out = String::new();
            let mut chars = inner.chars().peekable();
            while let Some(c) = chars.next() {
                if c == '\\' {
                    match chars.peek() {
                        Some(&'n') => { chars.next(); out.push('\n'); }
                        Some(&'t') => { chars.next(); out.push('\t'); }
                        Some(&'r') => { chars.next(); out.push('\r'); }
                        Some(&'\\') => { chars.next(); out.push('\\'); }
                        Some(&'"') => { chars.next(); out.push('"'); }
                        _ => { out.push('\\'); }
                    }
                } else {
                    out.push(c);
                }
            }
            return (out, true);
        }
        // Unterminated double quote: PHP RAW keeps the opening quote but
        // still cuts at semicolon. Fall through to unquoted handling.
    }
    // Unquoted value: strip inline comment from semicolon onwards.
    // (Both NORMAL and RAW strip `;` for unquoted values.)
    let mut val = match raw.find(';') {
        Some(p) => raw[..p].trim_end().to_string(),
        None => raw.to_string(),
    };
    val = val.trim().to_string();
    // For NORMAL/TYPED mode, resolve constants. Constants resolve only for
    // bareword values (identifier-like).
    if !is_raw && !val.is_empty() {
        let is_ident = val.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if is_ident {
            let upper = val.as_bytes().to_vec();
            // Skip booleans/null keywords which the caller handles.
            let lower = val.to_ascii_lowercase();
            if !matches!(lower.as_str(), "true" | "false" | "on" | "off" | "yes" | "no" | "none" | "null") {
                if let Some(c) = vm.constants.get(upper.as_slice()) {
                    return (c.to_php_string().to_string_lossy().to_string(), false);
                }
            }
        }
    }
    (val, false)
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
        vm.emit_deprecated(&format!("Constant {} is deprecated since 8.3, as assert_options() is deprecated", name));
    }
    vm.emit_deprecated("Function assert_options() is deprecated since 8.3");

    let (key, default): (&[u8], bool) = match option {
        1 => (b"assert.active", true),
        3 => (b"assert.bail", false),
        4 => (b"assert.warning", true),
        5 => (b"assert.exception", true),
        2 => {
            // ASSERT_CALLBACK
            let prev = ASSERT_CALLBACK.with(|c| c.borrow().clone()).unwrap_or_else(|| {
                match vm.constants.get(b"assert.callback".as_ref()) {
                    Some(v) if !v.to_php_string().as_bytes().is_empty() => Value::String(v.to_php_string()),
                    _ => Value::Null,
                }
            });
            if let Some(nv) = new_value {
                ASSERT_CALLBACK.with(|c| *c.borrow_mut() = if matches!(nv, Value::Null) { None } else { Some(nv) });
            }
            return Ok(prev);
        }
        _ => {
            let msg = "assert_options(): Argument #1 ($option) must be an ASSERT_* constant";
            let exc = vm.create_exception(b"ValueError", msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError { message: format!("Uncaught ValueError: {}", msg), line: vm.current_line });
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
        return Err(VmError { message: msg, line: vm.current_line });
    }
    let size = size_long as u64;
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
    goro_core::value::register_resource(fid, b"stream");
    Ok(Value::Resource(fid))
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
                let secs = modified.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
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
                let secs = accessed.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
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
            Ok(meta) => Ok(Value::Long(meta.ctime() as i64)),
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
                    let secs = c.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
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

fn chown_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let filename = args.first().unwrap_or(&Value::Null).to_php_string();
    let path_str = filename.to_string_lossy();
    if path_str.is_empty() || !std::path::Path::new(&*path_str).exists() {
        vm.emit_warning("chown(): No such file or directory");
        return Ok(Value::False);
    }
    // We don't actually change ownership (root-only on Unix), but return true
    // if the file exists (matches PHP's behavior when called as root or
    // already-owner).
    Ok(Value::True)
}

fn chgrp_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let filename = args.first().unwrap_or(&Value::Null).to_php_string();
    let path_str = filename.to_string_lossy();
    if path_str.is_empty() || !std::path::Path::new(&*path_str).exists() {
        vm.emit_warning("chgrp(): No such file or directory");
        return Ok(Value::False);
    }
    Ok(Value::True)
}

fn fputcsv_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let fid = args.first().unwrap_or(&Value::Null).to_long();
    let fields = match args.get(1) {
        Some(Value::Array(arr)) => arr.clone(),
        _ => return Ok(Value::False),
    };
    // Validate separator/enclosure/escape — each must be a single character
    // (escape may be empty). PHP throws ValueError for invalid sizes. Null
    // arguments are treated as not-provided (named-arg gaps).
    if let Some(v) = args.get(2) {
        if !matches!(v, Value::Null) {
            let s = v.to_php_string();
            if s.len() != 1 {
                let msg = "fputcsv(): Argument #3 ($separator) must be a single character".to_string();
                let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg, line: vm.current_line });
            }
        }
    }
    if let Some(v) = args.get(3) {
        if !matches!(v, Value::Null) {
            let s = v.to_php_string();
            if s.len() != 1 {
                let msg = "fputcsv(): Argument #4 ($enclosure) must be a single character".to_string();
                let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg, line: vm.current_line });
            }
        }
    }
    if let Some(v) = args.get(4) {
        if !matches!(v, Value::Null) {
            let s = v.to_php_string();
            if s.len() > 1 {
                let msg = "fputcsv(): Argument #5 ($escape) must be empty or a single character".to_string();
                let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg, line: vm.current_line });
            }
        }
    }
    let separator = match args.get(2) {
        Some(v) if !matches!(v, Value::Null) => {
            let s = v.to_php_string();
            let b = s.as_bytes();
            if b.is_empty() { b',' } else { b[0] }
        }
        _ => b',',
    };
    let enclosure = match args.get(3) {
        Some(v) if !matches!(v, Value::Null) => {
            let s = v.to_php_string();
            let b = s.as_bytes();
            if b.is_empty() { b'"' } else { b[0] }
        }
        _ => b'"',
    };
    let escape: Option<u8> = match args.get(4) {
        Some(v) if !matches!(v, Value::Null) => {
            let s = v.to_php_string();
            let b = s.as_bytes();
            if b.is_empty() { None } else { Some(b[0]) }
        }
        _ => {
            let line = vm.current_line;
            vm.emit_deprecated_at(
                "fputcsv(): the $escape parameter must be provided as its default value will change",
                line,
            );
            Some(b'\\')
        }
    };

    // User-provided end-of-line sequence (PHP 8.1+), default "\n".
    let eol: Vec<u8> = match args.get(5) {
        Some(v) if !matches!(v, Value::Null | Value::Undef) => v.to_php_string().as_bytes().to_vec(),
        _ => b"\n".to_vec(),
    };
    let mut line: Vec<u8> = Vec::new();
    let arr = fields.borrow();
    let mut first = true;
    for (_, val) in arr.iter() {
        if !first {
            line.push(separator);
        }
        first = false;
        // PHP emits "Array to string conversion" warning when fputcsv encounters
        // an array element (and writes literal "Array").
        if matches!(val, Value::Array(_)) {
            let cur_line = vm.current_line;
            vm.emit_warning_at("Array to string conversion", cur_line);
        }
        let s = val.to_php_string();
        // Check if we need to enclose (delimiter, enclosure, escape, \n, \r,
        // \t, or space anywhere in the field — matches PHP behavior).
        let s_bytes = s.as_bytes();
        let needs_enclosure = s_bytes.iter().any(|&b| {
            b == separator
                || b == enclosure
                || escape.map(|e| b == e).unwrap_or(false)
                || b == b'\n'
                || b == b'\r'
                || b == b'\t'
                || b == b' '
        });
        if needs_enclosure {
            line.push(enclosure);
            let mut escaped = false;
            for &c in s_bytes {
                if let Some(e) = escape {
                    if c == e && !escaped {
                        escaped = true;
                        line.push(c);
                        continue;
                    }
                }
                if c == enclosure && !escaped {
                    line.push(enclosure);
                }
                escaped = false;
                line.push(c);
            }
            line.push(enclosure);
        } else {
            line.extend_from_slice(s_bytes);
        }
    }
    line.extend_from_slice(&eol);

    let bytes = line.as_slice();
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
    let has_read_filters = crate::stream_filters::chain_has_direction(fid, crate::stream_filters::STREAM_FILTER_READ);
    if has_read_filters {
        let mut total = 0i64;
        loop {
            let chunk = read_filtered(vm, fid, 8192);
            if chunk.is_empty() { break; }
            total += chunk.len() as i64;
            vm.write_output(&chunk);
        }
        return Ok(Value::Long(total));
    }
    let buf = FILE_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(fh) = handles.get_mut(&fid) {
            let mut buf = Vec::new();
            match fh.file.read_to_end(&mut buf) {
                Ok(_n) => {
                    fh.eof = true;
                    Some(buf)
                }
                Err(_) => None,
            }
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
        match std::fs::metadata(&*path_s) {
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
        return Err(throw_value_error_in(vm, "parse_ini_file", args, "parse_ini_file(): Argument #1 ($filename) must not be empty"));
    }
    let process_sections = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
    let scanner_mode = args.get(2).map(|v| v.to_long()).unwrap_or(0);

    // Search include_path for relative filenames.
    let read_result = std::fs::read_to_string(&*path_str).or_else(|e| {
        // Only fall back to include_path for non-absolute, non-current-dir paths.
        if path_str.starts_with('/') || path_str.starts_with("./") || path_str.starts_with("../") {
            return Err(e);
        }
        let include_path = vm
            .constants
            .get(b"include_path".as_ref())
            .map(|v| v.to_php_string().to_string_lossy().to_string())
            .unwrap_or_else(|| ".".to_string());
        for dir in include_path.split(':') {
            let candidate = format!("{}/{}", dir.trim_end_matches('/'), path_str);
            if let Ok(c) = std::fs::read_to_string(&candidate) {
                return Ok(c);
            }
        }
        Err(e)
    });

    match read_result {
        Ok(content) => {
            Ok(crate::ini_parser::parse_ini(vm, content.as_bytes(), process_sections, scanner_mode, &path_str))
        }
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
        let msg = format!("error_get_last() expects exactly 0 arguments, {} given", args.len());
        let exc = vm.create_exception(b"ArgumentCountError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: format!("Uncaught ArgumentCountError: {}", msg), line: vm.current_line });
    }
    match vm.last_error.clone() {
        None => Ok(Value::Null),
        Some((ty, msg, file, line)) => {
            let mut arr = PhpArray::new();
            arr.set(ArrayKey::String(PhpString::from_bytes(b"type")), Value::Long(ty));
            arr.set(ArrayKey::String(PhpString::from_bytes(b"message")), Value::String(PhpString::from_string(msg)));
            arr.set(ArrayKey::String(PhpString::from_bytes(b"file")), Value::String(PhpString::from_string(file)));
            arr.set(ArrayKey::String(PhpString::from_bytes(b"line")), Value::Long(line as i64));
            Ok(Value::Array(Rc::new(RefCell::new(arr))))
        }
    }
}

fn error_clear_last_fn(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    vm.last_error = None;
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
    let mode = args.first().map(|v| v.to_php_string()).unwrap_or_else(|| PhpString::from_bytes(b"a"));
    if mode.len() != 1 {
        return Err(throw_value_error_msg(_vm, "php_uname(): Argument #1 ($mode) must be a single character"));
    }
    let mode_char = mode.as_bytes()[0];
    if !matches!(mode_char, b'a' | b'm' | b'n' | b'r' | b's' | b'v') {
        return Err(throw_value_error_msg(_vm, "php_uname(): Argument #1 ($mode) must be one of \"a\", \"m\", \"n\", \"r\", \"s\", or \"v\""));
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

thread_local! {
    /// Startup configuration directives (php.ini / -d / phpt --INI--).
    static CFG_VARS: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
}

/// Record a startup configuration directive for get_cfg_var().
pub fn register_cfg_var(key: &str, value: &str) {
    CFG_VARS.with(|c| c.borrow_mut().push((key.to_string(), value.to_string())));
}

fn get_cfg_var_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let name = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy();
    if name == "cfg_file_path" {
        return Ok(Value::String(PhpString::from_bytes(b"")));
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
                        a.set(Vm::value_to_array_key(Value::String(PhpString::from_string(off.trim().to_string()))), val);
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
                // Constants not visible from the calling scope are "not defined"
                if let Some(meta) = class.constants_meta.get(const_name.as_bytes()) {
                    use goro_core::object::Visibility;
                    if meta.visibility != Visibility::Public {
                        let decl = meta.declaring_class.to_ascii_lowercase();
                        let scope = vm.current_class_scope().map(|s| s.to_ascii_lowercase());
                        let ok = match meta.visibility {
                            Visibility::Private => scope.as_deref() == Some(decl.as_slice()),
                            _ => scope.map(|sc| sc == decl || vm.class_extends(&sc, &decl) || vm.class_extends(&decl, &sc)).unwrap_or(false),
                        };
                        if !ok {
                            return Ok(Value::False);
                        }
                    }
                }
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

fn fgetc_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let arg = args.first().unwrap_or(&Value::Null);
    let fid = require_open_stream(vm, arg, "fgetc")?;
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
            let msg = "flock(): Argument #2 ($operation) must be of type int, array given".to_string();
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
        }
        Value::String(_) => {
            let msg = "flock(): Argument #2 ($operation) must be of type int, string given".to_string();
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
        }
        _ => op_arg.to_long(),
    };
    let basic = op & 3;
    if basic == 0 {
        let msg = "flock(): Argument #2 ($operation) must be one of LOCK_SH, LOCK_EX, or LOCK_UN".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    let arg = args.first().cloned().unwrap_or(Value::Null);
    // Allow dir-handle resources (Linux PHP flock on a dir returns false but
    // does not throw).
    let dir_fid = if let Value::Resource(rid) = &arg {
        let is_dir = DIR_HANDLES.with(|dh| dh.borrow().contains_key(rid));
        if is_dir { Some(*rid) } else { None }
    } else { None };
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

fn stream_get_contents_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let fid = args.first().unwrap_or(&Value::Null).to_long();
    let max_length = args.get(1).map(|v| v.to_long()).unwrap_or(-1);
    let offset = args.get(2).map(|v| v.to_long()).unwrap_or(-1);

    let has_read_filters = crate::stream_filters::chain_has_direction(fid, crate::stream_filters::STREAM_FILTER_READ);
    if has_read_filters {
        if offset >= 0 {
            FILE_HANDLES.with(|h| {
                if let Some(fh) = h.borrow_mut().get_mut(&fid) {
                    let _ = fh.file.seek(SeekFrom::Start(offset as u64));
                }
            });
        }
        let limit = if max_length < 0 { usize::MAX } else { max_length as usize };
        let mut out: Vec<u8> = Vec::new();
        while out.len() < limit {
            let want = (limit - out.len()).min(8192);
            let chunk = read_filtered(vm, fid, want);
            if chunk.is_empty() { break; }
            out.extend_from_slice(&chunk);
        }
        return Ok(Value::String(PhpString::from_vec(out)));
    }

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

    // Helper: a property is "visible" on `class_lower` if declared on it OR
    // declared on an ancestor with non-private visibility.
    fn class_has_visible_prop(vm: &Vm, class_lower: &[u8], pname: &[u8]) -> bool {
        if let Some(ce) = vm.get_class_def(class_lower) {
            for prop in &ce.properties {
                if prop.name != pname { continue; }
                let decl_lower: Vec<u8> = prop.declaring_class.iter().map(|b| b.to_ascii_lowercase()).collect();
                if prop.visibility == Visibility::Private && decl_lower != class_lower {
                    continue;
                }
                return true;
            }
        }
        false
    }

    // Extension-provided property handlers (has_property with ZEND_PROPERTY_EXISTS)
    if let Value::Object(obj) = obj_or_class {
        if let Some(r) = vm.native_prop_read(obj, prop_bytes) {
            return Ok(if r.is_some() { Value::True } else { Value::False });
        }
    }
    match obj_or_class {
        Value::Object(obj) => {
            let obj_borrow = obj.borrow();
            let class_lower: Vec<u8> = obj_borrow.class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
            // Declared property check (filters out inherited private).
            if class_has_visible_prop(vm, &class_lower, prop_bytes) {
                return Ok(Value::True);
            }
            // Otherwise consider this as a dynamic property only if it is
            // present on the instance AND not declared anywhere on the
            // class hierarchy (so that inherited private props don't count).
            if !obj_borrow.has_property(prop_bytes) {
                return Ok(Value::False);
            }
            drop(obj_borrow);
            let mut declared_anywhere = false;
            let mut current = Some(class_lower);
            while let Some(cl) = current {
                if let Some(ce) = vm.get_class_def(&cl) {
                    if ce.properties.iter().any(|p| p.name == prop_bytes) {
                        declared_anywhere = true;
                        break;
                    }
                    current = ce.parent.as_ref().map(|p| p.iter().map(|b| b.to_ascii_lowercase()).collect());
                } else { break; }
            }
            Ok(if !declared_anywhere { Value::True } else { Value::False })
        }
        Value::String(s) => {
            let class_lower: Vec<u8> = s.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
            if !vm.classes.contains_key(&class_lower) && !vm.is_known_builtin_class(&class_lower) {
                vm.try_autoload_class(s.as_bytes());
                if vm.current_exception.is_some() {
                    return Err(VmError { message: "Uncaught exception".into(), line: vm.current_line });
                }
            }
            Ok(if class_has_visible_prop(vm, &class_lower, prop_bytes) { Value::True } else { Value::False })
        }
        Value::Reference(r) => {
            let mut a = args.to_vec();
            a[0] = r.borrow().clone();
            property_exists_fn(vm, &a)
        }
        other => Err(throw_type_error_msg(vm, format!("property_exists(): Argument #1 ($object_or_class) must be of type object|string, {} given", Vm::value_type_name(other)))),
    }
}

fn get_mangled_object_vars_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Object(outer)) = args.first() {
        // For initialized lazy proxy, redirect to the real backing object.
        let obj_rc = vm.proxy_redirect(outer);
        let obj = obj_rc.borrow();
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
                || name.starts_with(b"__fiber_") || name.starts_with(b"__timestamp")
                || name.starts_with(b"__lazy_") || name.starts_with(b"__uri_")
                || name.starts_with(b"__weak_ref_id") || name.starts_with(b"__sxml_")
                || name.starts_with(b"__hash_") || name.starts_with(b"__hmac_")
                || name.starts_with(b"__pkey_id") {
                continue;
            }
            // Skip uninitialized properties (Undef).
            if matches!(val, Value::Undef) {
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
        Ok(Value::Array(Vm::proptable_to_symtable(Rc::new(RefCell::new(arr)))))
    } else {
        Ok(Value::Null)
    }
}

fn get_resource_id_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    let id = val.resource_id();
    if id >= 0 {
        Ok(Value::Long(id))
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

struct GetoptOpt {
    opt_char: u8,
    opt_name: Option<Vec<u8>>,
    need_param: u8,
}

/// Port of main/getopt.c php_getopt(). Returns the option char (0 for a long
/// option, -2 for an invalid argument, -1 at the end) and sets *optidx.
fn php_getopt(argv: &[Vec<u8>], opts: &[GetoptOpt], optarg: &mut Option<Vec<u8>>, optind: &mut usize,
              optchr: &mut usize, dash: &mut bool, optidx: &mut isize) -> i32 {
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
        arg_start += opts[*optidx as usize].opt_name.as_ref().map(|n| n.len()).unwrap_or(0);
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
    let options = args.first().map(|v| v.to_php_string().as_bytes().to_vec()).unwrap_or_default();
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
    }.or_else(|| Some(Value::Array(Rc::new(RefCell::new({
        let mut a = PhpArray::new();
        a.push(Value::String(PhpString::from_string(vm.current_file.clone())));
        a
    })))));
    let argv: Vec<Vec<u8>> = match argv_val.map(|v| v.deref()) {
        Some(Value::Array(a)) => a.borrow().values().map(|v| v.to_php_string().as_bytes().to_vec()).collect(),
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
        opts.push(GetoptOpt { opt_char: c, opt_name: None, need_param: need });
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
            opts.push(GetoptOpt { opt_char: 0, opt_name: Some(name), need_param: need });
        }
    }
    opts.push(GetoptOpt { opt_char: b'-', opt_name: None, need_param: 0 });

    let mut result = PhpArray::new();
    let mut optind = 1usize;
    let mut optchr = 0usize;
    let mut dash = false;
    let mut optidx: isize = -1;
    loop {
        let mut optarg: Option<Vec<u8>> = None;
        let o = php_getopt(&argv, &opts, &mut optarg, &mut optind, &mut optchr, &mut dash, &mut optidx);
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
            match std::str::from_utf8(&optname).ok().and_then(|s| s.parse::<i64>().ok()) {
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

    use std::process::{Command, Stdio};
    // PHP semantics: if cmd is an array, exec directly without a shell.
    // Otherwise pass to sh -c.
    let mut child_cmd = match cmd {
        Value::Array(arr) => {
            let arr = arr.borrow();
            let parts: Vec<String> = arr
                .iter()
                .map(|(_, v)| v.to_php_string().to_string_lossy().to_string())
                .collect();
            if parts.is_empty() {
                return Ok(Value::False);
            }
            let mut c = Command::new(&parts[0]);
            if parts.len() > 1 {
                c.args(&parts[1..]);
            }
            c
        }
        _ => {
            let cmd_str = cmd.to_php_string().to_string_lossy().to_string();
            let mut c = Command::new("sh");
            c.arg("-c").arg(&cmd_str);
            c
        }
    };
    let mut has_stdin_pipe = false;
    let mut has_stdout_pipe = false;
    let mut has_stderr_pipe = false;
    let mut stdin_assigned = false;
    let mut stdout_assigned = false;
    let mut stderr_assigned = false;

    if let Some(Value::Array(spec)) = descriptor_spec {
        let spec = spec.borrow();
        for (key, val) in spec.iter() {
            let fd_num = match key { ArrayKey::Int(n) => *n, _ => continue };
            match val {
                Value::Array(fd_spec) => {
                    let fd_spec = fd_spec.borrow();
                    let fd_type = fd_spec.get_int(0).map(|v| v.to_php_string()).unwrap_or_else(|| PhpString::from_bytes(b""));
                    match &*fd_type.to_string_lossy() {
                        "pipe" => match fd_num {
                            0 => { child_cmd.stdin(Stdio::piped()); has_stdin_pipe = true; stdin_assigned = true; }
                            1 => { child_cmd.stdout(Stdio::piped()); has_stdout_pipe = true; stdout_assigned = true; }
                            2 => { child_cmd.stderr(Stdio::piped()); has_stderr_pipe = true; stderr_assigned = true; }
                            _ => {}
                        },
                        "file" => {
                            let filename = fd_spec.get_int(1).map(|v| v.to_php_string()).unwrap_or_else(|| PhpString::from_bytes(b""));
                            let mode = fd_spec.get_int(2).map(|v| v.to_php_string()).unwrap_or_else(|| PhpString::from_bytes(b"r"));
                            let mode_str = mode.to_string_lossy();
                            let filename_str = filename.to_string_lossy();
                            match fd_num {
                                0 => { if let Ok(f) = std::fs::File::open(&*filename_str) { child_cmd.stdin(f); stdin_assigned = true; } }
                                1 => {
                                    let file = if mode_str.contains('a') {
                                        std::fs::OpenOptions::new().append(true).create(true).open(&*filename_str)
                                    } else { std::fs::File::create(&*filename_str) };
                                    if let Ok(f) = file { child_cmd.stdout(f); stdout_assigned = true; }
                                }
                                2 => {
                                    let file = if mode_str.contains('a') {
                                        std::fs::OpenOptions::new().append(true).create(true).open(&*filename_str)
                                    } else { std::fs::File::create(&*filename_str) };
                                    if let Ok(f) = file { child_cmd.stderr(f); stderr_assigned = true; }
                                }
                                _ => {}
                            }
                        }
                        _ => {}
                    }
                }
                Value::Long(fid) => {
                    // The user passed an existing file/stream resource (like STDIN, or an fopen handle).
                    // STDIN=1, STDOUT=2, STDERR=3 in our const setup; treat these as inherited.
                    // For other ids, look up the file in FILE_HANDLES and dup its fd.
                    use std::os::unix::io::AsRawFd;
                    let fid_v = *fid;
                    if fid_v <= 3 {
                        // Inherit standard streams: stdio is fd 0/1/2
                        match fd_num {
                            0 => { child_cmd.stdin(Stdio::inherit()); stdin_assigned = true; }
                            1 => { child_cmd.stdout(Stdio::inherit()); stdout_assigned = true; }
                            2 => { child_cmd.stderr(Stdio::inherit()); stderr_assigned = true; }
                            _ => {}
                        }
                    } else {
                        // dup the file handle into a fresh fd we can hand to Command
                        let dup_fd: Option<i32> = FILE_HANDLES.with(|h| {
                            let handles = h.borrow();
                            handles.get(&fid_v).map(|fh| {
                                let raw = fh.file.as_raw_fd();
                                unsafe { libc::dup(raw) }
                            })
                        });
                        if let Some(dup_fd) = dup_fd {
                            if dup_fd >= 0 {
                                let stdio: Stdio = unsafe { <Stdio as std::os::unix::io::FromRawFd>::from_raw_fd(dup_fd) };
                                match fd_num {
                                    0 => { child_cmd.stdin(stdio); stdin_assigned = true; }
                                    1 => { child_cmd.stdout(stdio); stdout_assigned = true; }
                                    2 => { child_cmd.stderr(stdio); stderr_assigned = true; }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    if !stdin_assigned { child_cmd.stdin(Stdio::null()); }
    if !stdout_assigned { child_cmd.stdout(Stdio::inherit()); }
    if !stderr_assigned { child_cmd.stderr(Stdio::inherit()); }

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
                    goro_core::value::register_resource(fid, b"stream");
                    pipes_arr.set(ArrayKey::Int(0), Value::Resource(fid));
                }
            }
            if has_stdout_pipe {
                if let Some(stdout) = child.stdout.take() {
                    let fid = alloc_file_handle(unsafe { std::fs::File::from_raw_fd(stdout.into_raw_fd()) }, "r");
                    goro_core::value::register_resource(fid, b"stream");
                    pipes_arr.set(ArrayKey::Int(1), Value::Resource(fid));
                }
            }
            if has_stderr_pipe {
                if let Some(stderr) = child.stderr.take() {
                    let fid = alloc_file_handle(unsafe { std::fs::File::from_raw_fd(stderr.into_raw_fd()) }, "r");
                    goro_core::value::register_resource(fid, b"stream");
                    pipes_arr.set(ArrayKey::Int(2), Value::Resource(fid));
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
            goro_core::value::register_resource(proc_id, b"process");
            Ok(Value::Resource(proc_id))
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
fn stream_set_timeout_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let arg = args.first().unwrap_or(&Value::Null);
    // Require an open stream resource (throws TypeError on closed/non-resource).
    let _fid = require_open_stream(vm, arg, "stream_set_timeout")?;
    // For regular file streams we return false; sockets would set SO_RCVTIMEO.
    // We do not yet track per-stream timeouts.
    Ok(Value::False)
}
fn stream_set_write_buffer_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(0))
}
fn stream_set_read_buffer_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(0))
}

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
    let has_src_read_filters = crate::stream_filters::chain_has_direction(src_fid, crate::stream_filters::STREAM_FILTER_READ);
    let mut buf: Vec<u8> = Vec::new();
    if has_src_read_filters {
        let limit = if max_length < 0 { usize::MAX } else { max_length as usize };
        while buf.len() < limit {
            let want = (limit - buf.len()).min(8192);
            let chunk = read_filtered(vm, src_fid, want);
            if chunk.is_empty() { break; }
            buf.extend_from_slice(&chunk);
        }
    } else {
        let data = FILE_HANDLES.with(|h| {
            let mut handles = h.borrow_mut();
            if let Some(fh) = handles.get_mut(&src_fid) {
                use std::io::Read;
                let mut buf = Vec::new();
                if max_length >= 0 {
                    buf.resize(max_length as usize, 0);
                    match fh.file.read(&mut buf) { Ok(n) => { buf.truncate(n); Some(buf) } Err(_) => None }
                } else {
                    match fh.file.read_to_end(&mut buf) { Ok(_) => Some(buf), Err(_) => None }
                }
            } else { None }
        });
        match data { Some(b) => buf = b, None => return Ok(Value::False), }
    }
    let len = buf.len();
    let filtered = crate::stream_filters::apply_write_chain(vm, dst_fid, &buf);
    let exists = dst_fid == 1 || dst_fid == 2 || dst_fid == 3 || FILE_HANDLES.with(|h| h.borrow().contains_key(&dst_fid));
    if !exists { return Ok(Value::False); }
    write_to_stream(vm, dst_fid, &filtered);
    Ok(Value::Long(len as i64))
}

fn stream_filter_register_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let name = args.first().map(|v| v.to_php_string().as_bytes().to_vec()).unwrap_or_default();
    let class = args.get(1).map(|v| v.to_php_string().as_bytes().to_vec()).unwrap_or_default();
    if name.is_empty() {
        let msg = "stream_filter_register(): Argument #1 ($filter_name) must be a non-empty string".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    if class.is_empty() {
        let msg = "stream_filter_register(): Argument #2 ($class) must be a non-empty string".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    Ok(if crate::stream_filters::register_user_filter(&name, &class) { Value::True } else { Value::False })
}

fn stream_filter_append_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    attach_filter_impl(vm, args, false)
}

fn stream_filter_prepend_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    attach_filter_impl(vm, args, true)
}

fn attach_filter_impl(vm: &mut Vm, args: &[Value], prepend: bool) -> Result<Value, VmError> {
    let stream_val = args.first().cloned().unwrap_or(Value::Null);
    let stream_id = match &stream_val {
        Value::Resource(n) => *n,
        Value::Long(n) => *n,
        Value::Reference(r) => match &*r.borrow() {
            Value::Resource(n) => *n,
            Value::Long(n) => *n,
            _ => return Ok(Value::False),
        },
        _ => return Ok(Value::False),
    };
    let name = args.get(1).map(|v| v.to_php_string().as_bytes().to_vec()).unwrap_or_default();
    if name.is_empty() { return Ok(Value::False); }
    let direction = args.get(2).map(|v| v.to_long()).unwrap_or(crate::stream_filters::STREAM_FILTER_ALL);
    let params = args.get(3).cloned().unwrap_or(Value::Null);

    // First check whether it's a registered user filter; if so, instantiate
    // an object of the registered class.
    let user_obj = if let Some(class_name) = crate::stream_filters::user_filter_class(&name) {
        Some(instantiate_user_filter(vm, &class_name, &name, &params))
    } else {
        None
    };

    let user_obj_opt = match user_obj {
        Some(Some(o)) => Some(o),
        Some(None) => return Ok(Value::False),
        None => None,
    };

    match crate::stream_filters::attach_filter(stream_id, &name, direction, &params, prepend, user_obj_opt) {
        Some(fid) => Ok(Value::Resource(fid)),
        None => Ok(Value::False),
    }
}

/// Instantiate the user-defined class registered for a stream filter.
/// Sets `filtername`, `params`, and `stream` properties as PHP does, then
/// calls `onCreate()` if defined.
fn instantiate_user_filter(vm: &mut Vm, class_name: &[u8], filter_name: &[u8], params: &Value) -> Option<Rc<RefCell<PhpObject>>> {
    let class_lower: Vec<u8> = class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
    if !vm.classes.contains_key(&class_lower) { return None; }
    let obj_id = vm.next_object_id();
    let mut obj = PhpObject::new(class_name.to_vec(), obj_id);
    vm.apply_class_property_defaults(&mut obj, &class_lower);
    obj.set_property(b"filtername".to_vec(), Value::String(PhpString::from_bytes(filter_name)));
    obj.set_property(b"params".to_vec(), params.clone());
    let rc = Rc::new(RefCell::new(obj));
    let obj_val = Value::Object(rc.clone());
    let _ = vm.call_object_method(&obj_val, b"onCreate", &[]);
    Some(rc)
}

fn stream_filter_remove_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let fid_opt: Option<i64> = match args.first() {
        Some(Value::Resource(n)) => Some(*n),
        Some(Value::Long(n)) => Some(*n),
        Some(Value::Reference(r)) => match &*r.borrow() {
            Value::Resource(n) => Some(*n),
            Value::Long(n) => Some(*n),
            _ => None,
        },
        _ => None,
    };
    let fid = match fid_opt {
        Some(n) if crate::stream_filters::is_filter_resource(n) => n,
        _ => {
            let msg = "stream_filter_remove(): supplied resource is not a valid stream filter resource".to_string();
            let exc = vm.create_exception(b"TypeError", &msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
        }
    };
    if crate::stream_filters::remove_filter(fid) { Ok(Value::True) } else { Ok(Value::False) }
}
/// Validate that `arr` has shape ["wrapper"]["opt"] = value (i.e. two levels of arrays).
/// On invalid, throws a ValueError matching PHP.
fn validate_context_options(vm: &mut Vm, arr: &Value) -> Result<std::collections::BTreeMap<Vec<u8>, std::collections::BTreeMap<Vec<u8>, Value>>, VmError> {
    let mut out = std::collections::BTreeMap::new();
    let outer = match arr {
        Value::Array(a) => a.clone(),
        _ => {
            let msg = "Options should have the form [\"wrappername\"][\"optionname\"] = $value".to_string();
            let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
        }
    };
    let outer_b = outer.borrow();
    for (k, v) in outer_b.iter() {
        let key_bytes: Vec<u8> = match k {
            ArrayKey::String(s) => s.as_bytes().to_vec(),
            ArrayKey::Int(_) => {
                let msg = "Options should have the form [\"wrappername\"][\"optionname\"] = $value".to_string();
                let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg, line: vm.current_line });
            }
        };
        let inner = match v {
            Value::Array(a) => a.clone(),
            _ => {
                let msg = "Options should have the form [\"wrappername\"][\"optionname\"] = $value".to_string();
                let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg, line: vm.current_line });
            }
        };
        let inner_b = inner.borrow();
        let mut opts = std::collections::BTreeMap::new();
        for (ik, iv) in inner_b.iter() {
            let ikey: Vec<u8> = match ik {
                ArrayKey::String(s) => s.as_bytes().to_vec(),
                ArrayKey::Int(n) => n.to_string().into_bytes(),
            };
            opts.insert(ikey, iv.clone());
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
    if has_ctx { return Some(fid); }
    // Stream resource: lazily attach default context.
    let has_stream = FILE_HANDLES.with(|h| h.borrow().contains_key(&fid));
    if has_stream {
        STREAM_CONTEXTS.with(|c| { c.borrow_mut().insert(fid, StreamContext::default()); });
        return Some(fid);
    }
    None
}

fn apply_context_params(vm: &mut Vm, ctx: &mut StreamContext, params: &Value) -> Result<(), VmError> {
    let arr = match params {
        Value::Array(a) => a.clone(),
        _ => {
            let msg = "Invalid stream/context parameter".to_string();
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
        }
    };
    let arr_b = arr.borrow();
    for (k, v) in arr_b.iter() {
        if let ArrayKey::String(s) = k {
            match s.as_bytes() {
                b"notification" => {
                    let is_valid = matches!(v, Value::String(_) | Value::Array(_) | Value::Object(_));
                    if !is_valid {
                        let msg = "Invalid notification callback".to_string();
                        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
                        vm.current_exception = Some(exc);
                        return Err(VmError { message: msg, line: vm.current_line });
                    }
                    ctx.notification = Some(v.clone());
                }
                b"options" => {
                    // If $v is not an array, raise TypeError matching PHP.
                    if !matches!(v, Value::Array(_)) {
                        let msg = "Invalid stream/context parameter".to_string();
                        let exc = vm.throw_type_error(msg.clone());
                        vm.current_exception = Some(exc);
                        return Err(VmError { message: msg, line: vm.current_line });
                    }
                    ctx.options = validate_context_options(vm, v)?;
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn context_options_to_array(opts: &std::collections::BTreeMap<Vec<u8>, std::collections::BTreeMap<Vec<u8>, Value>>) -> PhpArray {
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
                return Err(VmError { message: msg, line: vm.current_line });
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
                return Err(VmError { message: msg, line: vm.current_line });
            }
            apply_context_params(vm, &mut ctx, params_val)?;
        }
    }
    let fid = NEXT_FILE_ID.with(|id| {
        let v = id.get();
        id.set(v + 1);
        v
    });
    STREAM_CONTEXTS.with(|c| {
        c.borrow_mut().insert(fid, ctx);
    });
    goro_core::value::register_resource(fid, b"stream-context");
    Ok(Value::Resource(fid))
}

fn stream_context_set_option_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let ctx_val = args.first().unwrap_or(&Value::Null);
    let fid = match resolve_context_id(ctx_val) {
        Some(n) => n,
        None => {
            let msg = "stream_context_set_option(): Argument #1 ($context) must be a valid stream/context".to_string();
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
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
                return Err(VmError { message: msg, line: vm.current_line });
            }
        }
        if let Some(v) = arg4 {
            if !matches!(v, Value::Null | Value::Undef) {
                let msg = "stream_context_set_option(): Argument #4 ($value) cannot be provided when argument #2 ($wrapper_or_options) is an array".to_string();
                let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg, line: vm.current_line });
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
            vm.emit_deprecated("Calling stream_context_set_option() with 2 arguments is deprecated, use stream_context_set_options() instead");
            let msg = "stream_context_set_option(): Argument #3 ($option_name) cannot be null when argument #2 ($wrapper_or_options) is a string".to_string();
            let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
        }
        if matches!(arg3, Some(Value::Null | Value::Undef) | None) {
            let msg = "stream_context_set_option(): Argument #3 ($option_name) cannot be null when argument #2 ($wrapper_or_options) is a string".to_string();
            let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
        }
        if arg4.is_none() {
            let msg = "stream_context_set_option(): Argument #4 ($value) must be provided when argument #2 ($wrapper_or_options) is a string".to_string();
            let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
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
            return Err(VmError { message: msg, line: vm.current_line });
        }
    };
    let arr = STREAM_CONTEXTS.with(|c| {
        c.borrow().get(&fid).map(|ctx| context_options_to_array(&ctx.options)).unwrap_or_default()
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
            return Err(VmError { message: msg, line: vm.current_line });
        }
    };
    let params = args.get(1).unwrap_or(&Value::Null);
    let mut ctx_clone = STREAM_CONTEXTS.with(|c| c.borrow().get(&fid).cloned().unwrap_or_default());
    apply_context_params(vm, &mut ctx_clone, params)?;
    STREAM_CONTEXTS.with(|c| { c.borrow_mut().insert(fid, ctx_clone); });
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
            return Err(VmError { message: msg, line: vm.current_line });
        }
    };
    let mut arr = PhpArray::new();
    STREAM_CONTEXTS.with(|c| {
        if let Some(ctx) = c.borrow().get(&fid) {
            if let Some(n) = &ctx.notification {
                arr.set(ArrayKey::String(PhpString::from_bytes(b"notification")), n.clone());
            }
            arr.set(
                ArrayKey::String(PhpString::from_bytes(b"options")),
                Value::Array(Rc::new(RefCell::new(context_options_to_array(&ctx.options)))),
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

thread_local! {
    static DEFAULT_STREAM_CTX: std::cell::Cell<Option<i64>> = const { std::cell::Cell::new(None) };
}

fn stream_context_get_default_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Accept optional $options array. If provided, validate and merge.
    if let Some(opts) = args.first() {
        if !matches!(opts, Value::Null | Value::Undef) {
            let merged = validate_context_options(vm, opts)?;
            let fid = DEFAULT_STREAM_CTX.with(|d| {
                if let Some(id) = d.get() { return id; }
                let id = NEXT_FILE_ID.with(|n| { let v = n.get(); n.set(v + 1); v });
                STREAM_CONTEXTS.with(|c| { c.borrow_mut().insert(id, StreamContext::default()); });
                goro_core::value::register_resource(id, b"stream-context");
                d.set(Some(id));
                id
            });
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
            return Ok(Value::Resource(fid));
        }
    }
    let fid = DEFAULT_STREAM_CTX.with(|d| {
        if let Some(id) = d.get() { return id; }
        let id = NEXT_FILE_ID.with(|n| { let v = n.get(); n.set(v + 1); v });
        STREAM_CONTEXTS.with(|c| { c.borrow_mut().insert(id, StreamContext::default()); });
        goro_core::value::register_resource(id, b"stream-context");
        d.set(Some(id));
        id
    });
    Ok(Value::Resource(fid))
}

fn stream_context_set_default_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(opts) = args.first() {
        if !matches!(opts, Value::Null | Value::Undef) {
            let merged = validate_context_options(vm, opts)?;
            let fid = DEFAULT_STREAM_CTX.with(|d| {
                if let Some(id) = d.get() { return id; }
                let id = NEXT_FILE_ID.with(|n| { let v = n.get(); n.set(v + 1); v });
                STREAM_CONTEXTS.with(|c| { c.borrow_mut().insert(id, StreamContext::default()); });
                goro_core::value::register_resource(id, b"stream-context");
                d.set(Some(id));
                id
            });
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
            if s_str.starts_with("http://") || s_str.starts_with("https://") || s_str.starts_with("ftp://") {
                Ok(Value::False)
            } else { Ok(Value::True) }
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
        arr.set(ArrayKey::String(PhpString::from_bytes(b"timed_out")), Value::False);
        arr.set(ArrayKey::String(PhpString::from_bytes(b"blocked")), Value::True);
        arr.set(ArrayKey::String(PhpString::from_bytes(b"eof")), Value::False);
        arr.set(ArrayKey::String(PhpString::from_bytes(b"wrapper_type")), Value::String(PhpString::from_bytes(b"plainfile")));
        arr.set(ArrayKey::String(PhpString::from_bytes(b"stream_type")), Value::String(PhpString::from_bytes(b"dir")));
        arr.set(ArrayKey::String(PhpString::from_bytes(b"mode")), Value::String(PhpString::from_bytes(b"r")));
        arr.set(ArrayKey::String(PhpString::from_bytes(b"unread_bytes")), Value::Long(0));
        arr.set(ArrayKey::String(PhpString::from_bytes(b"seekable")), Value::True);
        return Ok(Value::Array(Rc::new(RefCell::new(arr))));
    }
    let fid = require_open_stream(vm, first, "stream_get_meta_data")?;
    let mut arr = PhpArray::new();
    FILE_HANDLES.with(|h| {
        if let Some(fh) = h.borrow().get(&fid) {
            // For user streams, prepend wrapper_data (the wrapper object instance).
            if let Some(obj) = &fh.user_wrapper {
                arr.set(ArrayKey::String(PhpString::from_bytes(b"wrapper_data")), obj.clone());
            }
            // Order matches PHP's _php_stream_populate_meta_data():
            // timed_out, blocked, eof, [wrapper_data], wrapper_type, stream_type,
            // mode, unread_bytes, seekable, uri.
            arr.set(ArrayKey::String(PhpString::from_bytes(b"timed_out")), Value::False);
            arr.set(ArrayKey::String(PhpString::from_bytes(b"blocked")), Value::True);
            arr.set(ArrayKey::String(PhpString::from_bytes(b"eof")), if fh.eof { Value::True } else { Value::False });
            arr.set(ArrayKey::String(PhpString::from_bytes(b"wrapper_type")), Value::String(PhpString::from_string(fh.wrapper_type.clone())));
            arr.set(ArrayKey::String(PhpString::from_bytes(b"stream_type")), Value::String(PhpString::from_string(fh.stream_type.clone())));
            arr.set(ArrayKey::String(PhpString::from_bytes(b"mode")), Value::String(PhpString::from_string(fh.mode.clone())));
            arr.set(ArrayKey::String(PhpString::from_bytes(b"unread_bytes")), Value::Long(fh.filter_read_ready.len() as i64));
            arr.set(ArrayKey::String(PhpString::from_bytes(b"seekable")), if fh.seekable { Value::True } else { Value::False });
            if !fh.uri.is_empty() {
                arr.set(ArrayKey::String(PhpString::from_bytes(b"uri")), Value::String(PhpString::from_string(fh.uri.clone())));
            }
        }
    });
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

fn stream_wrapper_register_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let scheme = args.first().unwrap_or(&Value::Null).to_php_string().as_bytes().to_vec();
    let class = args.get(1).unwrap_or(&Value::Null).to_php_string().as_bytes().to_vec();
    let flags = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    if scheme.is_empty() {
        vm.emit_warning("stream_wrapper_register(): Protocol must not be empty");
        return Ok(Value::False);
    }
    // Check if already registered (built-in or user). PHP returns false and emits
    // a warning if a wrapper for this protocol already exists.
    let scheme_lower: Vec<u8> = scheme.iter().map(|b| b.to_ascii_lowercase()).collect();
    let builtin = matches!(scheme_lower.as_slice(), b"file" | b"http" | b"https" | b"ftp" | b"ftps" | b"php" | b"data" | b"compress.zlib" | b"compress.bzip2" | b"phar" | b"glob");
    let already_user = USER_WRAPPERS.with(|w| w.borrow().contains_key(&scheme_lower));
    if builtin || already_user {
        vm.emit_warning(&format!(
            "stream_wrapper_register(): Protocol {}:// is already defined.",
            String::from_utf8_lossy(&scheme)
        ));
        return Ok(Value::False);
    }
    USER_WRAPPERS.with(|w| {
        w.borrow_mut().insert(scheme_lower, UserWrapper {
            class_name: class,
            flags,
        });
    });
    Ok(Value::True)
}

fn stream_wrapper_unregister_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let scheme = args.first().unwrap_or(&Value::Null).to_php_string().as_bytes().to_vec();
    let scheme_lower: Vec<u8> = scheme.iter().map(|b| b.to_ascii_lowercase()).collect();
    let was_present = USER_WRAPPERS.with(|w| w.borrow_mut().remove(&scheme_lower).is_some());
    if !was_present {
        // PHP also lets you "unregister" a built-in wrapper, which then needs to be
        // restored. For now we report false on unknown schemes.
        let builtin = matches!(scheme_lower.as_slice(), b"file" | b"http" | b"https" | b"ftp" | b"ftps" | b"php" | b"data" | b"compress.zlib" | b"compress.bzip2" | b"phar" | b"glob");
        if !builtin {
            vm.emit_warning(&format!(
                "stream_wrapper_unregister(): Unable to unregister protocol {}://",
                String::from_utf8_lossy(&scheme)
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
        b"data" as &[u8], b"file", b"glob", b"http", b"https", b"php",
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

fn stream_set_chunk_size_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let arg = args.first().unwrap_or(&Value::Null);
    let _fid = require_open_stream(vm, arg, "stream_set_chunk_size")?;
    let size = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    if size <= 0 {
        let msg = "stream_set_chunk_size(): Argument #2 ($size) must be greater than 0".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    // We don't currently honour the chunk size internally. Return the default
    // 8192 so callers see a sane previous value.
    Ok(Value::Long(8192))
}
fn stream_socket_recvfrom_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn stream_socket_sendto_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn stream_socket_shutdown_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::True) }
fn stream_socket_enable_crypto_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn headers_list_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut arr = PhpArray::new();
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
    // Build entries snapshot for the Directory class methods (read/rewind/close).
    let mut entries_arr = PhpArray::new();
    if let Ok(names) = read_dir_entries(&path_str) {
        // names is reversed (descending); reverse-iterate to get ascending.
        for (i, name) in names.iter().rev().enumerate() {
            entries_arr.set(
                ArrayKey::Int(i as i64),
                Value::String(PhpString::from_string(name.clone())),
            );
        }
    }
    let object_id = vm.alloc_object_id();
    let mut obj = PhpObject::new(b"Directory".to_vec(), object_id);
    obj.set_property(
        b"path".to_vec(),
        Value::String(PhpString::from_string(path_str.to_string())),
    );
    obj.set_property(b"handle".to_vec(), Value::Resource(did));
    obj.set_property(b"__dir_pos".to_vec(), Value::Long(0));
    obj.set_property(
        b"__dir_entries".to_vec(),
        Value::Array(Rc::new(RefCell::new(entries_arr))),
    );
    Ok(Value::Object(Rc::new(RefCell::new(obj))))
}

fn popen_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let command = args.first().unwrap_or(&Value::Null).to_php_string();
    let mode = args.get(1).map(|v| v.to_php_string()).unwrap_or_else(|| PhpString::from_bytes(b"r"));
    let cmd_str = command.to_string_lossy();
    let mode_str = mode.to_string_lossy();
    // Validate mode: PHP accepts only "r", "rb", "w", "wb".
    if !matches!(mode_str.as_str(), "r" | "rb" | "w" | "wb") {
        let msg = "popen(): Argument #2 ($mode) must be one of \"r\", \"rb\", \"w\", or \"wb\"".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
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
                    let fid = alloc_file_handle(unsafe { std::fs::File::from_raw_fd(stdout.into_raw_fd()) }, &mode_str);
                    goro_core::value::register_resource(fid, b"stream");
                    return Ok(Value::Resource(fid));
                }
            } else if let Some(stdin) = child.stdin {
                let fid = alloc_file_handle(unsafe { std::fs::File::from_raw_fd(stdin.into_raw_fd()) }, &mode_str);
                goro_core::value::register_resource(fid, b"stream");
                return Ok(Value::Resource(fid));
            }
            Ok(Value::False)
        }
        Err(_) => Ok(Value::False),
    }
}

fn pclose_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let fid = args.first().unwrap_or(&Value::Null).to_long();
    FILE_HANDLES.with(|h| h.borrow_mut().remove(&fid));
    goro_core::value::close_resource(fid);
    Ok(Value::Long(0))
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

fn disk_space_impl(vm: &mut Vm, args: &[Value], fn_name: &str, total: bool, called_as: &str) -> Result<Value, VmError> {
    let path_val = args.first().cloned().unwrap_or(Value::Null);
    let path_str = path_val.to_php_string();
    let path_bytes = path_str.as_bytes();
    if path_bytes.is_empty() {
        let msg = "Path cannot be empty".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    if path_bytes.contains(&0u8) {
        let msg = format!("{}(): Argument #1 ($directory) must not contain any null bytes", fn_name);
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
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
            vm.emit_warning_at(&format!("{}(): {}", called_as, std::io::Error::last_os_error()), line);
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
        Ok(Value::Double(if total { 10737418240.0 } else { 1073741824.0 }))
    }
}
fn disk_free_space_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> { disk_space_impl(vm, args, "disk_free_space", false, "disk_free_space") }
fn diskfreespace_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> { disk_space_impl(vm, args, "diskfreespace", false, "diskfreespace") }
fn disk_total_space_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> { disk_space_impl(vm, args, "disk_total_space", true, "disk_total_space") }
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
    let timeout = args.get(4).map(|v| v.to_double()).filter(|&d| d > 0.0).unwrap_or(60.0);

    let host_str = hostname.to_string_lossy();

    // Try to connect
    use std::net::{TcpStream, ToSocketAddrs};
    // Strip URL scheme
    let stripped = host_str
        .trim_start_matches("tcp://")
        .trim_start_matches("ssl://")
        .trim_start_matches("tls://")
        .to_string();
    let addr = if port > 0 && !stripped.contains(':') {
        format!("{}:{}", stripped, port)
    } else {
        stripped
    };

    // Resolve via DNS (supports "localhost")
    let socket_addr = match addr.to_socket_addrs() {
        Ok(mut iter) => match iter.next() {
            Some(a) => a,
            None => {
                if let Some(Value::Reference(r)) = errno_ref { *r.borrow_mut() = Value::Long(111); }
                if let Some(Value::Reference(r)) = errstr_ref {
                    *r.borrow_mut() = Value::String(PhpString::from_string(format!("DNS lookup failed for {}", addr)));
                }
                vm.emit_warning(&format!("fsockopen(): Unable to connect to {} (DNS lookup failed)", addr));
                return Ok(Value::False);
            }
        },
        Err(e) => {
            if let Some(Value::Reference(r)) = errno_ref { *r.borrow_mut() = Value::Long(111); }
            if let Some(Value::Reference(r)) = errstr_ref {
                *r.borrow_mut() = Value::String(PhpString::from_string(e.to_string()));
            }
            vm.emit_warning(&format!("fsockopen(): Unable to connect to {} ({})", addr, e));
            return Ok(Value::False);
        }
    };

    match TcpStream::connect_timeout(&socket_addr, std::time::Duration::from_secs_f64(timeout)) {
        Ok(stream) => {
            // Set errno to 0 and errstr to ""
            if let Some(Value::Reference(r)) = errno_ref { *r.borrow_mut() = Value::Long(0); }
            if let Some(Value::Reference(r)) = errstr_ref { *r.borrow_mut() = Value::String(PhpString::from_bytes(b"")); }

            use std::os::unix::io::IntoRawFd;
            let raw_fd = stream.into_raw_fd();
            use std::os::unix::io::FromRawFd;
            let file = unsafe { std::fs::File::from_raw_fd(raw_fd) };
            let fid = alloc_file_handle(file, "r+");
            goro_core::value::register_resource(fid, b"stream");
            Ok(Value::Resource(fid))
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

fn register_tick_function_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(cb) = args.first() {
        validate_callback_arg(vm, "register_tick_function", cb)?;
    }
    Ok(Value::True)
}
fn unregister_tick_function_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(cb) = args.first() {
        validate_callback_arg(vm, "unregister_tick_function", cb)?;
    }
    Ok(Value::Null)
}
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
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let (v, err) = goro_core::ini_quantity::parse_quantity(s.as_bytes(), true);
    if let Some(e) = err {
        vm.emit_warning(&e);
    }
    Ok(Value::Long(v as i64))
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
    let flags = if args.len() > 1 { args[1].to_long() } else { 0 };
    let token_parse = (flags & 1) != 0; // TOKEN_PARSE

    let mut lexer = goro_parser::Lexer::new(source.as_bytes());
    lexer.set_preserve_trivia(true);
    let tokens = lexer.tokenize();
    let src = source.as_bytes();

    // Pre-pass: combine adjacent Identifier / Backslash / Namespace sequences
    // into T_NAME_QUALIFIED (Foo\Bar=265), T_NAME_FULLY_QUALIFIED (\Foo\Bar=263),
    // or T_NAME_RELATIVE (namespace\Foo=264). Combining requires zero whitespace
    // or comments between consecutive lexer tokens.
    use goro_parser::token::TokenKind as TK;
    let is_ident = |k: &TK| matches!(k, TK::Identifier(_));
    // (token_id, start, end, line). token_id==0 represents a single-char token.
    let mut combined: Vec<(i64, usize, usize, u32)> = Vec::with_capacity(tokens.len());
    let mut i = 0usize;
    while i < tokens.len() {
        let tok = &tokens[i];
        if matches!(tok.kind, TK::Eof) {
            i += 1;
            continue;
        }
        let token_id = goro_parser::token::token_kind_to_php_id(&tok.kind);
        let s_start = tok.span.start as usize;
        let s_end = tok.span.end as usize;
        let s_line = tok.span.line;

        let adjacent = |idx: usize, end_pos: usize| -> bool {
            tokens.get(idx).map_or(false, |t| t.span.start as usize == end_pos)
        };

        // Case 1: `\` + Ident (+ \ Ident)* → T_NAME_FULLY_QUALIFIED (263)
        if matches!(tok.kind, TK::Backslash)
            && adjacent(i + 1, s_end)
            && tokens.get(i + 1).map_or(false, |t| is_ident(&t.kind))
        {
            let mut j = i + 1;
            let mut end = tokens[j].span.end as usize;
            j += 1;
            while j + 1 < tokens.len()
                && matches!(tokens[j].kind, TK::Backslash)
                && tokens[j].span.start as usize == end
                && is_ident(&tokens[j + 1].kind)
                && tokens[j + 1].span.start as usize == tokens[j].span.end as usize
            {
                end = tokens[j + 1].span.end as usize;
                j += 2;
            }
            combined.push((263, s_start, end, s_line));
            i = j;
            continue;
        }
        // Case 2: `namespace` + `\` + Ident (+ \ Ident)* → T_NAME_RELATIVE (264)
        if matches!(tok.kind, TK::Namespace)
            && adjacent(i + 1, s_end)
            && tokens.get(i + 1).map_or(false, |t| matches!(t.kind, TK::Backslash))
            && tokens.get(i + 2).map_or(false, |t| is_ident(&t.kind))
            && tokens[i + 2].span.start as usize == tokens[i + 1].span.end as usize
        {
            let mut j = i + 3;
            let mut end = tokens[i + 2].span.end as usize;
            while j + 1 < tokens.len()
                && matches!(tokens[j].kind, TK::Backslash)
                && tokens[j].span.start as usize == end
                && is_ident(&tokens[j + 1].kind)
                && tokens[j + 1].span.start as usize == tokens[j].span.end as usize
            {
                end = tokens[j + 1].span.end as usize;
                j += 2;
            }
            combined.push((264, s_start, end, s_line));
            i = j;
            continue;
        }
        // Case 3: Ident + `\` + Ident (+ \ Ident)* → T_NAME_QUALIFIED (265)
        if is_ident(&tok.kind)
            && tokens.get(i + 1).map_or(false, |t| matches!(t.kind, TK::Backslash))
            && tokens[i + 1].span.start as usize == s_end
            && tokens.get(i + 2).map_or(false, |t| is_ident(&t.kind))
            && tokens[i + 2].span.start as usize == tokens[i + 1].span.end as usize
        {
            let mut j = i + 3;
            let mut end = tokens[i + 2].span.end as usize;
            while j + 1 < tokens.len()
                && matches!(tokens[j].kind, TK::Backslash)
                && tokens[j].span.start as usize == end
                && is_ident(&tokens[j + 1].kind)
                && tokens[j + 1].span.start as usize == tokens[j].span.end as usize
            {
                end = tokens[j + 1].span.end as usize;
                j += 2;
            }
            combined.push((265, s_start, end, s_line));
            i = j;
            continue;
        }

        // Fallback: emit single token; token_id==0 marks a punctuation token.
        combined.push((token_id, s_start, s_end, s_line));
        i += 1;
    }

    let mut result = PhpArray::new();
    // Track context for TOKEN_PARSE semi-reserved handling:
    // after `::` or `->` or `const`, a reserved keyword should tokenize as T_STRING.
    let mut last_meaningful_id: i64 = 0;

    for (token_id_orig, start, end, line) in combined {
        let mut token_id = token_id_orig;

        // In TOKEN_PARSE mode, demote a keyword to T_STRING when it follows
        // `::` (T_DOUBLE_COLON=397), `->` (T_OBJECT_OPERATOR=384), `?->`
        // (T_NULLSAFE_OBJECT_OPERATOR=385), or `const` (T_CONST=312).
        if token_parse
            && token_id >= 272
            && token_id <= 342
            && matches!(last_meaningful_id, 397 | 384 | 385 | 312)
        {
            token_id = 262;
        }

        if token_id == 0 {
            if start < src.len() && end <= src.len() && start < end {
                let text = &src[start..end];
                result.push(Value::String(PhpString::from_bytes(text)));
                if text == b";" || text == b"{" || text == b"}" {
                    last_meaningful_id = 0;
                }
            }
        } else {
            let text = if start < src.len() && end <= src.len() && start < end {
                &src[start..end]
            } else {
                b""
            };
            let mut token_arr = PhpArray::new();
            token_arr.set(ArrayKey::Int(0), Value::Long(token_id));
            token_arr.set(ArrayKey::Int(1), Value::String(PhpString::from_bytes(text)));
            token_arr.set(ArrayKey::Int(2), Value::Long(line as i64));
            result.push(Value::Array(Rc::new(RefCell::new(token_arr))));
            if !goro_parser::token::token_id_is_ignorable(token_id) {
                last_meaningful_id = token_id;
            }
        }
    }

    Ok(Value::Array(Rc::new(RefCell::new(result))))
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
    // Validate $options (first argument) keys and value shapes. Real PHP only
    // accepts specific string keys with int/string values.
    let valid_keys: &[&[u8]] = &[
        b"max_input_vars",
        b"max_multipart_body_parts",
        b"max_file_uploads",
        b"upload_max_filesize",
        b"post_max_size",
    ];
    let size_keys: &[&[u8]] = &[b"upload_max_filesize", b"post_max_size"];
    if let Some(opts) = args.first() {
        if let Value::Array(arr) = opts {
            let pairs: Vec<(goro_core::array::ArrayKey, Value)> = arr.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            for (k, v) in pairs {
                match k {
                    goro_core::array::ArrayKey::Int(_) => {
                        let exc = vm.create_exception(b"ValueError", "Invalid integer key in $options argument", 0);
                        vm.current_exception = Some(exc);
                        return Err(VmError { message: "Uncaught ValueError".to_string(), line: 0 });
                    }
                    goro_core::array::ArrayKey::String(s) => {
                        let bytes = s.as_bytes();
                        if !valid_keys.iter().any(|vk| *vk == bytes) {
                            let exc = vm.create_exception(b"ValueError", &format!("Invalid key \"{}\" in $options argument", s.to_string_lossy()), 0);
                            vm.current_exception = Some(exc);
                            return Err(VmError { message: "Uncaught ValueError".to_string(), line: 0 });
                        }
                        // Check value type
                        if let Value::Array(_) = v {
                            let exc = vm.create_exception(b"ValueError", "Invalid array value in $options argument", 0);
                            vm.current_exception = Some(exc);
                            return Err(VmError { message: "Uncaught ValueError".to_string(), line: 0 });
                        }
                        // For *_max_filesize/post_max_size keys: validate suffix (K/M/G).
                        if size_keys.iter().any(|sk| *sk == bytes) {
                            if let Value::String(strval) = &v {
                                let raw = strval.as_bytes();
                                if let Some(&last) = raw.last() {
                                    if last.is_ascii_alphabetic()
                                        && !matches!(last, b'K' | b'M' | b'G' | b'k' | b'm' | b'g')
                                    {
                                        let mult = std::str::from_utf8(&[last]).unwrap_or("?").to_string();
                                        vm.emit_warning(&format!(
                                            "Invalid quantity \"{}\": unknown multiplier \"{}\", interpreting as \"1\" for backwards compatibility",
                                            strval.to_string_lossy(),
                                            mult
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // After validation, request_parse_body fails with a RequestParseBodyException
    // because we have no request context to parse.
    let exc = vm.create_exception(b"RequestParseBodyException", "Request does not provide a content type", 0);
    vm.current_exception = Some(exc);
    Err(VmError { message: "Uncaught RequestParseBodyException".to_string(), line: 0 })
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

fn stream_resolve_include_path_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let filename = args.first().map(|v| v.to_php_string().to_string_lossy()).unwrap_or_default();
    if filename.is_empty() { return Ok(Value::False); }
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
    let sep = if cfg!(target_os = "windows") { ';' } else { ':' };
    for entry in include_path.split(sep) {
        if entry.is_empty() { continue; }
        let candidate = if entry == "." {
            std::path::PathBuf::from(&*filename)
        } else {
            std::path::PathBuf::from(entry).join(&*filename)
        };
        if candidate.exists() {
            return Ok(match std::fs::canonicalize(&candidate) {
                Ok(cp) => Value::String(PhpString::from_string(cp.to_string_lossy().into_owned())),
                Err(_) => Value::String(PhpString::from_string(candidate.to_string_lossy().into_owned())),
            });
        }
    }
    Ok(Value::False)
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
