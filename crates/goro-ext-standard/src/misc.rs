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
    vm.register_function(b"array_key_exists", array_key_exists);
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
    vm.register_function(b"date_default_timezone_set", date_default_timezone_set);
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
    vm.register_function(b"array_diff_uassoc", array_diff_uassoc_fn);
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
    vm.register_function(b"mkdir", mkdir_fn);
    vm.register_function(b"rmdir", rmdir_fn);
    vm.register_function(b"glob", glob_fn);
    vm.register_function(b"scandir", scandir_fn);
    vm.register_function(b"header", header_fn);
    vm.register_function(b"headers_sent", headers_sent_fn);
    vm.register_function(b"http_response_code", http_response_code_fn);
    vm.register_function(b"spl_object_hash", spl_object_hash_fn);
    vm.register_function(b"spl_object_id", spl_object_id_fn);
    vm.register_function(b"iterator_to_array", iterator_to_array_fn);
    vm.register_function(b"iterator_count", iterator_count_fn);
    vm.register_function(b"array_map", array_map);

    // Date
    vm.register_function(b"time", time_fn);
    vm.register_function(b"microtime", microtime);
    vm.register_function(b"date", date_fn);
    vm.register_function(b"gmdate", gmdate_fn);
    vm.register_function(b"mktime", mktime);
    vm.register_function(b"gmmktime", gmmktime_fn);
    vm.register_function(b"strftime", strftime_fn);
    vm.register_function(b"strtotime", strtotime);
    vm.register_function(b"date_create", date_create_fn);
    vm.register_function(b"getdate", getdate_fn);
    vm.register_function(b"localtime", localtime_fn);
    vm.register_function(b"checkdate", checkdate_fn);
    vm.register_function(b"idate", idate_fn);

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
    vm.register_function(b"html_entity_decode", html_entity_decode);
    vm.register_function(b"htmlspecialchars_decode", htmlspecialchars_decode);
    vm.register_function(b"crc32", crc32_fn);
    vm.register_function(b"md5", md5_fn);
    vm.register_function(b"sha1", sha1_fn);
    vm.register_function(b"hash", hash_fn);
    vm.register_function(b"hash_algos", hash_algos_fn);
    vm.register_function(b"hash_equals", hash_equals_fn);
    vm.register_function(b"str_word_count", str_word_count);
    vm.register_function(b"substr_count", substr_count);
    vm.register_function(b"substr_replace", substr_replace);
    vm.register_function(b"str_ireplace", str_ireplace);
    vm.register_function(b"stripos", stripos);
    vm.register_function(b"strrpos", strrpos);
    vm.register_function(b"strripos", strripos);
    vm.register_function(b"strcmp", strcmp);
    vm.register_function(b"strncmp", strncmp);
    vm.register_function(b"strcasecmp", strcasecmp);
    vm.register_function(b"strncasecmp", strncasecmp);
    vm.register_function(b"str_contains", str_contains_fn);
    vm.register_function(b"wordwrap", wordwrap);
    vm.register_function(b"printf", printf);
    vm.register_function(b"fprintf", fprintf_fn);
    vm.register_function(b"vfprintf", vfprintf_fn);
    vm.register_function(b"sscanf", sscanf_fn);
    vm.register_function(b"ctype_alpha", ctype_alpha);
    vm.register_function(b"ctype_digit", ctype_digit);
    vm.register_function(b"ctype_alnum", ctype_alnum);
    vm.register_function(b"ctype_upper", ctype_upper);
    vm.register_function(b"ctype_lower", ctype_lower);
    vm.register_function(b"ctype_space", ctype_space);
    vm.register_function(b"ctype_cntrl", ctype_cntrl);
    vm.register_function(b"ctype_graph", ctype_graph);
    vm.register_function(b"ctype_print", ctype_print);
    vm.register_function(b"ctype_punct", ctype_punct);
    vm.register_function(b"ctype_xdigit", ctype_xdigit);

    // JSON
    vm.register_function(b"json_encode", json_encode);
    vm.register_function(b"json_decode", json_decode);
    vm.register_function(b"json_last_error", json_last_error);
    vm.register_function(b"json_last_error_msg", json_last_error_msg);
    vm.register_function(b"json_validate", json_validate);

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

    // Regex stubs
    vm.register_function(b"preg_match", preg_match);
    vm.register_function(b"preg_match_all", preg_match_all);
    vm.register_function(b"preg_replace", preg_replace);
    vm.register_function(b"preg_split", preg_split);
    vm.register_function(b"preg_quote", preg_quote);
}

// === Error handling ===

fn error_reporting(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let old = vm.error_reporting;
    if let Some(level) = args.first() {
        vm.error_reporting = level.to_long();
    }
    Ok(Value::Long(old))
}

fn set_error_handler(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Null)
}

fn restore_error_handler(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
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
            return Err(VmError {
                message: message.to_string(),
                line: 0,
            });
        }
        512 => {
            // E_USER_WARNING
            vm.emit_warning(&message);
        }
        1024 => {
            // E_USER_NOTICE
            vm.emit_notice_at(&message, 0);
        }
        16384 => {
            // E_USER_DEPRECATED
            vm.emit_deprecated_at(&message, 0);
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

fn ob_start(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True)
}
fn ob_end_clean(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True)
}
fn ob_end_flush(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True)
}
fn ob_get_contents(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(PhpString::empty()))
}
fn ob_get_clean(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(PhpString::empty()))
}
fn ob_get_level(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(0))
}
fn ob_flush(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True)
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
        Value::Object(_) => Ok(Value::True), // Objects with __invoke
        Value::Array(arr) => {
            let arr = arr.borrow();
            Ok(if arr.len() == 2 {
                Value::True
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

    // Get function name and captured vars
    let (func_name, captured) = match callback {
        Value::String(s) => (s.as_bytes().to_vec(), vec![]),
        Value::Array(arr) => {
            let arr = arr.borrow();
            let vals: Vec<Value> = arr.values().cloned().collect();
            if vals.is_empty() {
                return Ok(Value::Null);
            }
            let name = vals[0].to_php_string().as_bytes().to_vec();
            (name, vals[1..].to_vec())
        }
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
    if let Some(Value::Array(arr)) = args.first() {
        let mut arr = arr.borrow_mut();
        // Remove last element
        let len = arr.len();
        if len == 0 {
            return Ok(Value::Null);
        }
        let entries: Vec<_> = arr.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let (last_key, last_val) = entries.last().unwrap();
        arr.remove(last_key);
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
        let existing: Vec<Value> = arr_mut.values().cloned().collect();
        let new_values: Vec<Value> = args[1..].to_vec();

        let mut new_arr = PhpArray::new();
        for val in &new_values {
            new_arr.push(val.clone());
        }
        for val in &existing {
            new_arr.push(val.clone());
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

fn array_flip(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let arr = arr.borrow();
        let mut result = PhpArray::new();
        for (key, val) in arr.iter() {
            let new_key = match val {
                Value::Long(n) => goro_core::array::ArrayKey::Int(*n),
                Value::String(s) => goro_core::array::ArrayKey::String(s.clone()),
                _ => continue,
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
        for val in &entries[start..end.min(entries.len())] {
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
        let length = args.get(2).map(|v| Some(v.to_long())).unwrap_or(None);
        let replacement = args.get(3);

        let mut arr_mut = arr.borrow_mut();
        let entries: Vec<Value> = arr_mut.values().cloned().collect();
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

        // Extract removed elements
        let removed: Vec<Value> = entries[start..end].to_vec();

        // Build new array
        let mut new_entries: Vec<Value> = entries[..start].to_vec();
        if let Some(repl) = replacement {
            if let Value::Array(repl_arr) = repl {
                let repl_arr = repl_arr.borrow();
                for (_, v) in repl_arr.iter() {
                    new_entries.push(v.clone());
                }
            } else {
                new_entries.push(repl.clone());
            }
        }
        new_entries.extend_from_slice(&entries[end..]);

        // Replace the array contents
        let mut new_arr = PhpArray::new();
        for val in new_entries {
            new_arr.push(val);
        }
        *arr_mut = new_arr;

        // Return removed elements
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

    if let Some(Value::Array(arr)) = array {
        let arr = arr.borrow();
        let mut result = PhpArray::new();

        // Get callback function name
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
            Value::Null => {
                // null callback = identity
                for (key, val) in arr.iter() {
                    result.set(key.clone(), val.clone());
                }
                return Ok(Value::Array(Rc::new(RefCell::new(result))));
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
        let arr = arr.borrow();
        let callback = args.get(1);
        let mut result = PhpArray::new();

        if let Some(cb) = callback {
            // Get callback function
            let (func_name, captured) = match cb {
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
                _ => {
                    return Ok(Value::Array(Rc::new(RefCell::new(result))));
                }
            };
            let func_lower: Vec<u8> = func_name.iter().map(|b| b.to_ascii_lowercase()).collect();

            if let Some(builtin) = vm.functions.get(&func_lower).copied() {
                for (key, val) in arr.iter() {
                    let keep = builtin(vm, &[val.clone()])?.is_truthy();
                    if keep {
                        result.set(key.clone(), val.clone());
                    }
                }
            } else if let Some(user_fn) = vm.user_functions.get(&func_lower).cloned() {
                for (key, val) in arr.iter() {
                    let mut fn_cvs = vec![Value::Undef; user_fn.cv_names.len()];
                    let mut idx = 0;
                    for cv in &captured {
                        if idx < fn_cvs.len() {
                            fn_cvs[idx] = cv.clone();
                            idx += 1;
                        }
                    }
                    if idx < fn_cvs.len() {
                        fn_cvs[idx] = val.clone();
                    }
                    let keep = vm.execute_fn(&user_fn, fn_cvs)?.is_truthy();
                    if keep {
                        result.set(key.clone(), val.clone());
                    }
                }
            }
        } else {
            // No callback - filter falsy values
            for (key, val) in arr.iter() {
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
            for (_key, val) in &entries {
                let mut fn_cvs = vec![Value::Undef; user_fn.cv_names.len()];
                let mut idx = 0;
                for cv in &captured {
                    if idx < fn_cvs.len() {
                        fn_cvs[idx] = cv.clone();
                        idx += 1;
                    }
                }
                if idx < fn_cvs.len() {
                    fn_cvs[idx] = val.clone();
                }
                let _ = vm.execute_fn(&user_fn, fn_cvs);
            }
        } else if let Some(builtin) = vm.functions.get(&func_lower).copied() {
            for (_key, val) in &entries {
                let _ = builtin(vm, &[val.clone()]);
            }
        }
    }
    Ok(Value::True)
}

fn array_combine(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let keys = match args.first() {
        Some(Value::Array(a)) => a.borrow(),
        _ => return Ok(Value::False),
    };
    let vals = match args.get(1) {
        Some(Value::Array(a)) => a.borrow(),
        _ => return Ok(Value::False),
    };
    let mut result = PhpArray::new();
    let keys_vec: Vec<_> = keys.values().cloned().collect();
    let vals_vec: Vec<_> = vals.values().cloned().collect();
    for (k, v) in keys_vec.iter().zip(vals_vec.iter()) {
        let key = match k {
            Value::Long(n) => goro_core::array::ArrayKey::Int(*n),
            Value::String(s) => goro_core::array::ArrayKey::String(s.clone()),
            _ => goro_core::array::ArrayKey::String(k.to_php_string()),
        };
        result.set(key, v.clone());
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn array_chunk(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let size = args.get(1).map(|v| v.to_long()).unwrap_or(1).max(1) as usize;
        let arr = arr.borrow();
        let entries: Vec<_> = arr.values().cloned().collect();
        let mut result = PhpArray::new();
        for chunk in entries.chunks(size) {
            let mut sub = PhpArray::new();
            for val in chunk {
                sub.push(val.clone());
            }
            result.push(Value::Array(Rc::new(RefCell::new(sub))));
        }
        Ok(Value::Array(Rc::new(RefCell::new(result))))
    } else {
        Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
    }
}

fn array_pad(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
}

fn array_fill(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let start = args.first().map(|v| v.to_long()).unwrap_or(0);
    let num = args.get(1).map(|v| v.to_long()).unwrap_or(0);
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
            let k = val.to_php_string();
            result.set(goro_core::array::ArrayKey::String(k), fill_val.clone());
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

fn array_diff(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
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
            if !b_vals.contains(&s) {
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

fn sort_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let mut arr = arr.borrow_mut();
        let mut entries: Vec<Value> = arr.values().cloned().collect();
        entries.sort_by(|a, b| {
            let cmp = a.compare(b);
            if cmp < 0 {
                std::cmp::Ordering::Less
            } else if cmp > 0 {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });
        // Rebuild array with sequential integer keys
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
        let mut arr = arr.borrow_mut();
        let mut entries: Vec<Value> = arr.values().cloned().collect();
        entries.sort_by(|a, b| {
            let cmp = b.compare(a);
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
        *arr = new_arr;
    }
    Ok(Value::True)
}
fn asort_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let mut arr = arr.borrow_mut();
        let mut entries: Vec<_> = arr.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        entries.sort_by(|a, b| {
            let cmp = a.1.compare(&b.1);
            if cmp < 0 {
                std::cmp::Ordering::Less
            } else if cmp > 0 {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });
        *arr = goro_core::array::PhpArray::new();
        for (k, v) in entries {
            arr.set(k, v);
        }
    }
    Ok(Value::True)
}
fn arsort_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let mut arr = arr.borrow_mut();
        let mut entries: Vec<_> = arr.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        entries.sort_by(|a, b| {
            let cmp = b.1.compare(&a.1);
            if cmp < 0 {
                std::cmp::Ordering::Less
            } else if cmp > 0 {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });
        *arr = goro_core::array::PhpArray::new();
        for (k, v) in entries {
            arr.set(k, v);
        }
    }
    Ok(Value::True)
}
fn ksort_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let mut arr = arr.borrow_mut();
        let mut entries: Vec<_> = arr.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        *arr = goro_core::array::PhpArray::new();
        for (k, v) in entries {
            arr.set(k, v);
        }
    }
    Ok(Value::True)
}
fn krsort_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let mut arr = arr.borrow_mut();
        let mut entries: Vec<_> = arr.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        entries.sort_by(|a, b| b.0.cmp(&a.0));
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
        Ok(arr
            .iter()
            .next()
            .map(|(_, v)| v.clone())
            .unwrap_or(Value::False))
    } else {
        Ok(Value::False)
    }
}

fn next_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}
fn prev_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}
fn reset_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let arr = arr.borrow();
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
        let arr = arr.borrow();
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

// === Misc ===

fn ini_set(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let name = args.first().unwrap_or(&Value::Null).to_php_string();
    let value = args.get(1).cloned().unwrap_or(Value::Null);
    let key = name.as_bytes().to_vec();
    let old = vm.constants.get(&key).cloned();
    // Actually update the value
    vm.constants.insert(key, value);
    // Return old value or false if not previously set
    Ok(old.unwrap_or(Value::False))
}
fn ini_get(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let name = args.first().unwrap_or(&Value::Null).to_php_string();
    Ok(vm
        .constants
        .get(name.as_bytes())
        .cloned()
        .unwrap_or(Value::False))
}
fn ini_restore(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Null)
}
fn set_time_limit(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True)
}
fn php_assert(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    Ok(if val.is_truthy() {
        Value::True
    } else {
        Value::False
    })
}
fn class_exists(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let name = args.first().unwrap_or(&Value::Null).to_php_string();
    let name_lower: Vec<u8> = name
        .as_bytes()
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
fn property_exists(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let class_or_obj = args.first().unwrap_or(&Value::Null);
    let prop_name = args.get(1).unwrap_or(&Value::Null).to_php_string();
    match class_or_obj {
        Value::Object(obj) => Ok(if obj.borrow().has_property(prop_name.as_bytes()) {
            Value::True
        } else {
            Value::False
        }),
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

    Ok(if let Some(class) = vm.classes.get(&class_lower) {
        if class.methods.contains_key(&method_lower) {
            Value::True
        } else {
            Value::False
        }
    } else {
        Value::False
    })
}
fn is_object(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(match args.first() {
        Some(Value::Object(_)) | Some(Value::Generator(_)) => Value::True,
        _ => Value::False,
    })
}
fn date_default_timezone_set(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True)
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
fn array_column(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
}
fn array_count_values(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
}
fn array_rand(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(0))
}

// === Date/Time ===

fn time_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    use std::time::SystemTime;
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    Ok(Value::Long(secs as i64))
}

fn microtime(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    use std::time::SystemTime;
    let dur = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let as_float = args.first().map(|v| v.is_truthy()).unwrap_or(false);
    if as_float {
        Ok(Value::Double(dur.as_secs_f64()))
    } else {
        Ok(Value::String(PhpString::from_string(format!(
            "{:.8} {}",
            dur.subsec_nanos() as f64 / 1e9,
            dur.as_secs()
        ))))
    }
}

fn date_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let format = args
        .first()
        .unwrap_or(&Value::Null)
        .to_php_string()
        .to_string_lossy();
    let timestamp = args.get(1).map(|v| v.to_long());

    // Get current time or use provided timestamp
    let secs = timestamp.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    });

    // Simple date formatting - compute components from unix timestamp
    // This is a simplified version, not handling timezones properly
    let days_since_epoch = secs / 86400;
    let time_of_day = ((secs % 86400) + 86400) % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Compute year/month/day from days since epoch (1970-01-01)
    let (year, month, day) = days_to_ymd(days_since_epoch);

    let mut result = String::new();
    let fmt_bytes = format.as_bytes();
    let mut i = 0;
    while i < fmt_bytes.len() {
        let c = fmt_bytes[i];
        if c == b'\\' && i + 1 < fmt_bytes.len() {
            result.push(fmt_bytes[i + 1] as char);
            i += 2;
            continue;
        }
        match c {
            b'Y' => result.push_str(&format!("{:04}", year)),
            b'y' => result.push_str(&format!("{:02}", year % 100)),
            b'm' => result.push_str(&format!("{:02}", month)),
            b'n' => result.push_str(&format!("{}", month)),
            b'd' => result.push_str(&format!("{:02}", day)),
            b'j' => result.push_str(&format!("{}", day)),
            b'H' => result.push_str(&format!("{:02}", hours)),
            b'G' => result.push_str(&format!("{}", hours)),
            b'i' => result.push_str(&format!("{:02}", minutes)),
            b's' => result.push_str(&format!("{:02}", seconds)),
            b'U' => result.push_str(&format!("{}", secs)),
            b'N' => {
                let dow = ((days_since_epoch % 7) + 4) % 7; // Monday=1
                result.push_str(&format!("{}", if dow == 0 { 7 } else { dow }));
            }
            b'w' => {
                let dow = ((days_since_epoch % 7) + 4) % 7;
                result.push_str(&format!("{}", dow));
            }
            b'g' => {
                let h12 = if hours == 0 {
                    12
                } else if hours > 12 {
                    hours - 12
                } else {
                    hours
                };
                result.push_str(&format!("{}", h12));
            }
            b'A' => result.push_str(if hours < 12 { "AM" } else { "PM" }),
            b'a' => result.push_str(if hours < 12 { "am" } else { "pm" }),
            b't' => {
                let days_in_month = match month {
                    2 => {
                        if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
                            29
                        } else {
                            28
                        }
                    }
                    4 | 6 | 9 | 11 => 30,
                    _ => 31,
                };
                result.push_str(&format!("{}", days_in_month));
            }
            b'L' => {
                let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
                result.push(if leap { '1' } else { '0' });
            }
            _ => result.push(c as char),
        }
        i += 1;
    }

    Ok(Value::String(PhpString::from_string(result)))
}

/// Convert days since epoch (1970-01-01) to (year, month, day)
fn days_to_ymd(days: i64) -> (i64, u32, u32) {
    // Civil date from day count algorithm
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year, m as u32, d as u32)
}
/// Convert (year, month, day) to days since epoch (1970-01-01)
/// Inverse of days_to_ymd
fn ymd_to_days(year: i64, month: u32, day: u32) -> i64 {
    // Civil date to day count algorithm (inverse of days_to_ymd)
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let m = month;
    let doy = if m > 2 {
        (153 * (m as u64 - 3) + 2) / 5 + day as u64 - 1
    } else {
        (153 * (m as u64 + 9) + 2) / 5 + day as u64 - 1
    };
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe as i64 - 719468
}

fn mktime(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // mktime(hour, minute, second, month, day, year)
    // Get current time as defaults
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let now_days = now_secs / 86400;
    let now_time = ((now_secs % 86400) + 86400) % 86400;
    let (now_year, now_month, now_day) = days_to_ymd(now_days);
    let now_hours = now_time / 3600;
    let now_minutes = (now_time % 3600) / 60;
    let now_seconds = now_time % 60;

    let hour = args.first().map(|v| v.to_long()).unwrap_or(now_hours);
    let minute = args.get(1).map(|v| v.to_long()).unwrap_or(now_minutes);
    let second = args.get(2).map(|v| v.to_long()).unwrap_or(now_seconds);
    let month = args.get(3).map(|v| v.to_long()).unwrap_or(now_month as i64);
    let day = args.get(4).map(|v| v.to_long()).unwrap_or(now_day as i64);
    let year = args.get(5).map(|v| v.to_long()).unwrap_or(now_year);

    // Handle year values 0-69 => 2000-2069, 70-100 => 1970-2000
    let year = if (0..70).contains(&year) {
        year + 2000
    } else if (70..=100).contains(&year) {
        year + 1900
    } else {
        year
    };

    let days = ymd_to_days(year, month as u32, day as u32);
    let timestamp = days * 86400 + hour * 3600 + minute * 60 + second;
    Ok(Value::Long(timestamp))
}
fn strtotime(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
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
    let num = args.first().map(|v| v.to_double()).unwrap_or(0.0);
    let decimals = args.get(1).map(|v| v.to_long()).unwrap_or(0) as usize;
    let dec_point = args
        .get(2)
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_else(|| ".".to_string());
    let thousands_sep = args
        .get(3)
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_else(|| ",".to_string());

    // Round first (PHP rounds half up)
    let factor = 10f64.powi(decimals as i32);
    let rounded = (num.abs() * factor).round() / factor;
    let formatted = format!("{:.prec$}", rounded, prec = decimals);
    let parts: Vec<&str> = formatted.split('.').collect();
    let int_part = parts[0];
    let dec_part = parts.get(1).unwrap_or(&"");

    // Add thousands separator
    let int_bytes = int_part.as_bytes();
    let mut with_sep = String::new();
    let len = int_bytes.len();
    for (i, &b) in int_bytes.iter().enumerate() {
        if i > 0 && (len - i) % 3 == 0 {
            with_sep.push_str(&thousands_sep);
        }
        with_sep.push(b as char);
    }

    let mut result = String::new();
    if num < 0.0 {
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

fn base64_encode(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(PhpString::empty())) // stub
}
fn base64_decode(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(PhpString::empty())) // stub
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
    let mut result = Vec::new();
    for &b in s.as_bytes() {
        match b {
            b'&' => result.extend_from_slice(b"&amp;"),
            b'"' => result.extend_from_slice(b"&quot;"),
            b'\'' => result.extend_from_slice(b"&#039;"),
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
fn htmlspecialchars_decode(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(PhpString::empty()))
}
fn crc32_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(0))
}
fn md5_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = args.first().unwrap_or(&Value::Null).to_php_string();
    let raw = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
    let digest = md5_hash(data.as_bytes());
    if raw {
        Ok(Value::String(PhpString::from_vec(digest.to_vec())))
    } else {
        let hex: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
        Ok(Value::String(PhpString::from_string(hex)))
    }
}

fn sha1_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = args.first().unwrap_or(&Value::Null).to_php_string();
    let raw = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
    let digest = sha1_hash(data.as_bytes());
    if raw {
        Ok(Value::String(PhpString::from_vec(digest.to_vec())))
    } else {
        let hex: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
        Ok(Value::String(PhpString::from_string(hex)))
    }
}

fn hash_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let algo = args
        .first()
        .unwrap_or(&Value::Null)
        .to_php_string()
        .to_string_lossy();
    let data = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let raw = args.get(2).map(|v| v.is_truthy()).unwrap_or(false);
    let algo_lower = algo.to_ascii_lowercase();

    let digest: Vec<u8> = match algo_lower.as_str() {
        "md5" => md5_hash(data.as_bytes()).to_vec(),
        "sha1" => sha1_hash(data.as_bytes()).to_vec(),
        "crc32" | "crc32b" => {
            let mut crc: u32 = 0xFFFFFFFF;
            for &byte in data.as_bytes() {
                crc ^= byte as u32;
                for _ in 0..8 {
                    if crc & 1 != 0 {
                        crc = (crc >> 1) ^ 0xEDB88320;
                    } else {
                        crc >>= 1;
                    }
                }
            }
            let r = crc ^ 0xFFFFFFFF;
            r.to_be_bytes().to_vec()
        }
        _ => return Ok(Value::False),
    };

    if raw {
        Ok(Value::String(PhpString::from_vec(digest)))
    } else {
        let hex: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
        Ok(Value::String(PhpString::from_string(hex)))
    }
}

fn hash_algos_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut result = PhpArray::new();
    for algo in &["md5", "sha1", "crc32", "crc32b"] {
        result.push(Value::String(PhpString::from_bytes(algo.as_bytes())));
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn hash_equals_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let known = args.first().unwrap_or(&Value::Null).to_php_string();
    let user = args.get(1).unwrap_or(&Value::Null).to_php_string();
    Ok(if known.as_bytes() == user.as_bytes() {
        Value::True
    } else {
        Value::False
    })
}

/// MD5 hash implementation (RFC 1321)
fn md5_hash(data: &[u8]) -> [u8; 16] {
    let mut a0: u32 = 0x67452301;
    let mut b0: u32 = 0xefcdab89;
    let mut c0: u32 = 0x98badcfe;
    let mut d0: u32 = 0x10325476;

    // Pre-processing: adding padding bits
    let bit_len = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_le_bytes());

    // Per-round shift amounts
    let s: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    let k: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
        0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
        0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
        0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
        0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
        0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
        0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
        0xeb86d391,
    ];

    for chunk in msg.chunks(64) {
        let mut m = [0u32; 16];
        for i in 0..16 {
            m[i] = u32::from_le_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((b & c) | ((!b) & d), i),
                16..=31 => ((d & b) | ((!d) & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | (!d)), (7 * i) % 16),
            };
            let temp = d;
            d = c;
            c = b;
            b = b.wrapping_add(
                (a.wrapping_add(f).wrapping_add(k[i]).wrapping_add(m[g])).rotate_left(s[i]),
            );
            a = temp;
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }

    let mut result = [0u8; 16];
    result[0..4].copy_from_slice(&a0.to_le_bytes());
    result[4..8].copy_from_slice(&b0.to_le_bytes());
    result[8..12].copy_from_slice(&c0.to_le_bytes());
    result[12..16].copy_from_slice(&d0.to_le_bytes());
    result
}

/// SHA1 hash implementation (FIPS 180-1)
fn sha1_hash(data: &[u8]) -> [u8; 20] {
    let mut h0: u32 = 0x67452301;
    let mut h1: u32 = 0xEFCDAB89;
    let mut h2: u32 = 0x98BADCFE;
    let mut h3: u32 = 0x10325476;
    let mut h4: u32 = 0xC3D2E1F0;

    let bit_len = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h0, h1, h2, h3, h4);
        for i in 0..80 {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1u32),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDCu32),
                _ => (b ^ c ^ d, 0xCA62C1D6u32),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(w[i]);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    let mut result = [0u8; 20];
    result[0..4].copy_from_slice(&h0.to_be_bytes());
    result[4..8].copy_from_slice(&h1.to_be_bytes());
    result[8..12].copy_from_slice(&h2.to_be_bytes());
    result[12..16].copy_from_slice(&h3.to_be_bytes());
    result[16..20].copy_from_slice(&h4.to_be_bytes());
    result
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

fn sscanf_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Null)
}
fn ctype_alpha(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bytes = ctype_get_bytes(args);
    Ok(
        if !bytes.is_empty() && bytes.iter().all(|b| b.is_ascii_alphabetic()) {
            Value::True
        } else {
            Value::False
        },
    )
}
fn ctype_digit(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bytes = ctype_get_bytes(args);
    Ok(
        if !bytes.is_empty() && bytes.iter().all(|b| b.is_ascii_digit()) {
            Value::True
        } else {
            Value::False
        },
    )
}
fn ctype_alnum(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bytes = ctype_get_bytes(args);
    Ok(
        if !bytes.is_empty() && bytes.iter().all(|b| b.is_ascii_alphanumeric()) {
            Value::True
        } else {
            Value::False
        },
    )
}
fn ctype_upper(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bytes = ctype_get_bytes(args);
    Ok(
        if !bytes.is_empty() && bytes.iter().all(|b| b.is_ascii_uppercase()) {
            Value::True
        } else {
            Value::False
        },
    )
}
fn ctype_lower(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bytes = ctype_get_bytes(args);
    Ok(
        if !bytes.is_empty() && bytes.iter().all(|b| b.is_ascii_lowercase()) {
            Value::True
        } else {
            Value::False
        },
    )
}
fn ctype_space(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bytes = ctype_get_bytes(args);
    Ok(
        if !bytes.is_empty()
            && bytes
                .iter()
                .all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C))
        {
            Value::True
        } else {
            Value::False
        },
    )
}

/// Helper for ctype functions: get bytes to check (handles int as ASCII code)
fn ctype_get_bytes(args: &[Value]) -> Vec<u8> {
    let val = args.first().unwrap_or(&Value::Null);
    match val {
        Value::Long(n) => {
            if *n >= -128 && *n <= 255 {
                let c = if *n < 0 { (*n + 256) as u8 } else { *n as u8 };
                vec![c]
            } else {
                val.to_php_string().as_bytes().to_vec()
            }
        }
        _ => val.to_php_string().as_bytes().to_vec(),
    }
}

fn ctype_cntrl(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bytes = ctype_get_bytes(args);
    Ok(
        if !bytes.is_empty() && bytes.iter().all(|b| b.is_ascii_control()) {
            Value::True
        } else {
            Value::False
        },
    )
}

fn ctype_graph(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bytes = ctype_get_bytes(args);
    Ok(
        if !bytes.is_empty() && bytes.iter().all(|b| b.is_ascii_graphic()) {
            Value::True
        } else {
            Value::False
        },
    )
}

fn ctype_print(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bytes = ctype_get_bytes(args);
    Ok(
        if !bytes.is_empty() && bytes.iter().all(|b| *b >= 0x20 && *b <= 0x7e) {
            Value::True
        } else {
            Value::False
        },
    )
}

fn ctype_punct(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bytes = ctype_get_bytes(args);
    Ok(
        if !bytes.is_empty() && bytes.iter().all(|b| b.is_ascii_punctuation()) {
            Value::True
        } else {
            Value::False
        },
    )
}

fn ctype_xdigit(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bytes = ctype_get_bytes(args);
    Ok(
        if !bytes.is_empty() && bytes.iter().all(|b| b.is_ascii_hexdigit()) {
            Value::True
        } else {
            Value::False
        },
    )
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

// === JSON ===

fn json_encode(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    let s = json_encode_value(val);
    Ok(Value::String(PhpString::from_string(s)))
}

fn json_encode_value(val: &Value) -> String {
    match val {
        Value::Null | Value::Undef => "null".to_string(),
        Value::True => "true".to_string(),
        Value::False => "false".to_string(),
        Value::Long(n) => n.to_string(),
        Value::Double(f) => {
            if f.is_infinite() || f.is_nan() {
                "null".to_string()
            } else {
                format!("{}", f)
            }
        }
        Value::String(s) => {
            let mut result = String::from("\"");
            for &b in s.as_bytes() {
                match b {
                    b'"' => result.push_str("\\\""),
                    b'\\' => result.push_str("\\\\"),
                    b'\n' => result.push_str("\\n"),
                    b'\r' => result.push_str("\\r"),
                    b'\t' => result.push_str("\\t"),
                    b if b < 0x20 => result.push_str(&format!("\\u{:04x}", b)),
                    _ => result.push(b as char),
                }
            }
            result.push('"');
            result
        }
        Value::Array(arr) => {
            let arr = arr.borrow();
            // Check if it's a sequential array
            let is_list = arr.iter().enumerate().all(
                |(i, (k, _))| matches!(k, goro_core::array::ArrayKey::Int(n) if *n == i as i64),
            );
            if is_list {
                let parts: Vec<String> = arr.values().map(json_encode_value).collect();
                format!("[{}]", parts.join(","))
            } else {
                let parts: Vec<String> = arr
                    .iter()
                    .map(|(k, v)| {
                        let key_str = match k {
                            goro_core::array::ArrayKey::Int(n) => format!("\"{}\"", n),
                            goro_core::array::ArrayKey::String(s) => {
                                format!("\"{}\"", s.to_string_lossy())
                            }
                        };
                        format!("{}:{}", key_str, json_encode_value(v))
                    })
                    .collect();
                format!("{{{}}}", parts.join(","))
            }
        }
        Value::Object(_) | Value::Generator(_) => "null".to_string(), // TODO: implement object JSON encoding
        Value::Reference(r) => json_encode_value(&r.borrow()),
    }
}

fn json_decode(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let json_str = match args.first() {
        Some(v) => v.to_php_string(),
        None => return Ok(Value::Null),
    };
    let json_bytes = json_str.as_bytes();

    // $associative parameter: null (default) means objects become stdClass, true means arrays
    let associative = match args.get(1) {
        Some(Value::True) => true,
        Some(Value::Null) | Some(Value::Undef) | None => false,
        Some(Value::False) => false,
        Some(v) => v.is_truthy(),
    };

    // $depth parameter (arg 2) - we don't enforce it strictly but parse it
    let max_depth: usize = match args.get(2) {
        Some(Value::Long(n)) if *n > 0 => *n as usize,
        Some(Value::Long(_)) => return Ok(Value::Null),
        None => 512,
        _ => 512,
    };

    // $flags parameter (arg 3) - parse relevant flags
    let flags: i64 = match args.get(3) {
        Some(v) => v.to_long(),
        None => 0,
    };
    let bigint_as_string = (flags & 2) != 0; // JSON_BIGINT_AS_STRING = 2

    let mut parser = JsonParser {
        input: json_bytes,
        pos: 0,
        depth: 0,
        max_depth,
        associative,
        bigint_as_string,
        vm,
    };

    match parser.parse_value() {
        Some(val) => {
            parser.skip_whitespace();
            if parser.pos < parser.input.len() {
                // Trailing data after valid JSON
                Ok(Value::Null)
            } else {
                Ok(val)
            }
        }
        None => Ok(Value::Null),
    }
}

/// Hand-rolled JSON parser that converts JSON to PHP values.
struct JsonParser<'a, 'b> {
    input: &'a [u8],
    pos: usize,
    depth: usize,
    max_depth: usize,
    associative: bool,
    bigint_as_string: bool,
    vm: &'b mut Vm,
}

impl<'a, 'b> JsonParser<'a, 'b> {
    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<u8> {
        let ch = self.input.get(self.pos).copied();
        if ch.is_some() {
            self.pos += 1;
        }
        ch
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.input.len() {
            match self.input[self.pos] {
                b' ' | b'\t' | b'\n' | b'\r' => self.pos += 1,
                _ => break,
            }
        }
    }

    fn parse_value(&mut self) -> Option<Value> {
        self.skip_whitespace();
        let ch = self.peek()?;
        match ch {
            b'"' => self
                .parse_string()
                .map(|s| Value::String(PhpString::from_vec(s))),
            b'{' => self.parse_object(),
            b'[' => self.parse_array(),
            b't' => self.parse_literal(b"true", Value::True),
            b'f' => self.parse_literal(b"false", Value::False),
            b'n' => self.parse_literal(b"null", Value::Null),
            b'-' | b'0'..=b'9' => self.parse_number(),
            _ => None,
        }
    }

    fn parse_literal(&mut self, expected: &[u8], value: Value) -> Option<Value> {
        if self.input[self.pos..].starts_with(expected) {
            self.pos += expected.len();
            Some(value)
        } else {
            None
        }
    }

    fn parse_string(&mut self) -> Option<Vec<u8>> {
        // Skip opening quote
        if self.advance()? != b'"' {
            return None;
        }

        let mut result = Vec::new();
        loop {
            let ch = self.advance()?;
            match ch {
                b'"' => return Some(result),
                b'\\' => {
                    let esc = self.advance()?;
                    match esc {
                        b'"' => result.push(b'"'),
                        b'\\' => result.push(b'\\'),
                        b'/' => result.push(b'/'),
                        b'n' => result.push(b'\n'),
                        b'r' => result.push(b'\r'),
                        b't' => result.push(b'\t'),
                        b'b' => result.push(0x08),
                        b'f' => result.push(0x0C),
                        b'u' => {
                            let cp = self.parse_hex4()?;
                            // Handle UTF-16 surrogate pairs
                            if (0xD800..=0xDBFF).contains(&cp) {
                                // High surrogate, expect \uXXXX low surrogate
                                if self.advance()? != b'\\' {
                                    return None;
                                }
                                if self.advance()? != b'u' {
                                    return None;
                                }
                                let low = self.parse_hex4()?;
                                if !(0xDC00..=0xDFFF).contains(&low) {
                                    return None;
                                }
                                let codepoint =
                                    0x10000 + ((cp as u32 - 0xD800) << 10) + (low as u32 - 0xDC00);
                                if let Some(c) = char::from_u32(codepoint) {
                                    let mut buf = [0u8; 4];
                                    let s = c.encode_utf8(&mut buf);
                                    result.extend_from_slice(s.as_bytes());
                                } else {
                                    return None;
                                }
                            } else if (0xDC00..=0xDFFF).contains(&cp) {
                                // Lone low surrogate is invalid
                                return None;
                            } else {
                                if let Some(c) = char::from_u32(cp as u32) {
                                    let mut buf = [0u8; 4];
                                    let s = c.encode_utf8(&mut buf);
                                    result.extend_from_slice(s.as_bytes());
                                } else {
                                    return None;
                                }
                            }
                        }
                        _ => return None, // Invalid escape sequence
                    }
                }
                // Control characters (0x00-0x1F) are invalid unescaped in JSON
                b if b < 0x20 => return None,
                _ => result.push(ch),
            }
        }
    }

    fn parse_hex4(&mut self) -> Option<u16> {
        let mut val: u16 = 0;
        for _ in 0..4 {
            let ch = self.advance()?;
            let digit = match ch {
                b'0'..=b'9' => ch - b'0',
                b'a'..=b'f' => ch - b'a' + 10,
                b'A'..=b'F' => ch - b'A' + 10,
                _ => return None,
            };
            val = val * 16 + digit as u16;
        }
        Some(val)
    }

    fn parse_number(&mut self) -> Option<Value> {
        let start = self.pos;

        // Optional minus
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }

        // Integer part
        match self.peek() {
            Some(b'0') => {
                self.pos += 1;
                // After leading 0, next char must not be a digit (no leading zeros)
            }
            Some(b'1'..=b'9') => {
                self.pos += 1;
                while let Some(b'0'..=b'9') = self.peek() {
                    self.pos += 1;
                }
            }
            _ => return None,
        }

        let mut is_float = false;

        // Fractional part
        if self.peek() == Some(b'.') {
            is_float = true;
            self.pos += 1;
            // Must have at least one digit after decimal point
            match self.peek() {
                Some(b'0'..=b'9') => {}
                _ => return None,
            }
            while let Some(b'0'..=b'9') = self.peek() {
                self.pos += 1;
            }
        }

        // Exponent part
        if let Some(b'e') | Some(b'E') = self.peek() {
            is_float = true;
            self.pos += 1;
            // Optional sign
            if let Some(b'+') | Some(b'-') = self.peek() {
                self.pos += 1;
            }
            // Must have at least one digit
            match self.peek() {
                Some(b'0'..=b'9') => {}
                _ => return None,
            }
            while let Some(b'0'..=b'9') = self.peek() {
                self.pos += 1;
            }
        }

        let num_str = std::str::from_utf8(&self.input[start..self.pos]).ok()?;

        if is_float {
            let f: f64 = num_str.parse().ok()?;
            Some(Value::Double(f))
        } else {
            // Try to parse as i64 first, fall back to f64 for very large numbers
            if let Ok(n) = num_str.parse::<i64>() {
                Some(Value::Long(n))
            } else if self.bigint_as_string {
                Some(Value::String(PhpString::from_string(num_str.to_string())))
            } else {
                // Number too large for i64, parse as float
                let f: f64 = num_str.parse().ok()?;
                Some(Value::Double(f))
            }
        }
    }

    fn parse_array(&mut self) -> Option<Value> {
        // Skip opening bracket
        if self.advance()? != b'[' {
            return None;
        }

        self.depth += 1;
        if self.depth > self.max_depth {
            return None;
        }

        let mut arr = PhpArray::new();

        self.skip_whitespace();
        if self.peek() == Some(b']') {
            self.pos += 1;
            self.depth -= 1;
            return Some(Value::Array(Rc::new(RefCell::new(arr))));
        }

        loop {
            let val = self.parse_value()?;
            arr.push(val);

            self.skip_whitespace();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                }
                Some(b']') => {
                    self.pos += 1;
                    self.depth -= 1;
                    return Some(Value::Array(Rc::new(RefCell::new(arr))));
                }
                _ => return None,
            }
        }
    }

    fn parse_object(&mut self) -> Option<Value> {
        // Skip opening brace
        if self.advance()? != b'{' {
            return None;
        }

        self.depth += 1;
        if self.depth > self.max_depth {
            return None;
        }

        self.skip_whitespace();

        if self.associative {
            // Decode as associative array
            let mut arr = PhpArray::new();

            if self.peek() == Some(b'}') {
                self.pos += 1;
                self.depth -= 1;
                return Some(Value::Array(Rc::new(RefCell::new(arr))));
            }

            loop {
                self.skip_whitespace();
                let key = self.parse_string()?;

                self.skip_whitespace();
                if self.advance()? != b':' {
                    return None;
                }

                let val = self.parse_value()?;
                arr.set(
                    goro_core::array::ArrayKey::String(PhpString::from_vec(key)),
                    val,
                );

                self.skip_whitespace();
                match self.peek() {
                    Some(b',') => {
                        self.pos += 1;
                    }
                    Some(b'}') => {
                        self.pos += 1;
                        self.depth -= 1;
                        return Some(Value::Array(Rc::new(RefCell::new(arr))));
                    }
                    _ => return None,
                }
            }
        } else {
            // Decode as stdClass object
            let obj_id = self.vm.next_object_id();
            let mut obj = PhpObject::new(b"stdClass".to_vec(), obj_id);

            if self.peek() == Some(b'}') {
                self.pos += 1;
                self.depth -= 1;
                return Some(Value::Object(Rc::new(RefCell::new(obj))));
            }

            loop {
                self.skip_whitespace();
                let key = self.parse_string()?;

                self.skip_whitespace();
                if self.advance()? != b':' {
                    return None;
                }

                let val = self.parse_value()?;
                obj.set_property(key, val);

                self.skip_whitespace();
                match self.peek() {
                    Some(b',') => {
                        self.pos += 1;
                    }
                    Some(b'}') => {
                        self.pos += 1;
                        self.depth -= 1;
                        return Some(Value::Object(Rc::new(RefCell::new(obj))));
                    }
                    _ => return None,
                }
            }
        }
    }
}
fn json_last_error(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(0))
}
fn json_last_error_msg(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(PhpString::from_bytes(b"No error")))
}

fn json_validate(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let json_str = match args.first() {
        Some(v) => v.to_php_string(),
        None => return Ok(Value::False),
    };
    let json_bytes = json_str.as_bytes();

    let max_depth: usize = match args.get(1) {
        Some(Value::Long(n)) if *n > 0 => *n as usize,
        Some(Value::Long(_)) => return Ok(Value::False),
        None => 512,
        _ => 512,
    };

    let mut parser = JsonParser {
        input: json_bytes,
        pos: 0,
        depth: 0,
        max_depth,
        associative: true,
        bigint_as_string: false,
        vm,
    };

    match parser.parse_value() {
        Some(_) => {
            parser.skip_whitespace();
            if parser.pos < parser.input.len() {
                Ok(Value::False)
            } else {
                Ok(Value::True)
            }
        }
        None => Ok(Value::False),
    }
}

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
    match std::fs::read(path.to_string_lossy().as_ref() as &str) {
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
            .open(path.to_string_lossy().as_ref() as &str)
            .and_then(|mut f| f.write_all(data.as_bytes()).map(|_| data.len()))
    } else {
        std::fs::write(path.to_string_lossy().as_ref() as &str, data.as_bytes()).map(|_| data.len())
    };
    match result {
        Ok(len) => Ok(Value::Long(len as i64)),
        Err(_) => Ok(Value::False),
    }
}
fn realpath_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    match std::fs::canonicalize(path.to_string_lossy().as_ref() as &str) {
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
    match std::env::set_current_dir(path.to_string_lossy().as_ref() as &str) {
        Ok(_) => Ok(Value::True),
        Err(_) => Ok(Value::False),
    }
}
fn filesize_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    match std::fs::metadata(path.to_string_lossy().as_ref() as &str) {
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
        if std::path::Path::new(path.to_string_lossy().as_ref() as &str)
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
    match std::fs::metadata(path.to_string_lossy().as_ref() as &str) {
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
            if goro_core::value::parse_numeric_string(s.as_bytes()).is_some() {
                Value::True
            } else {
                Value::False
            }
        }
        _ => Value::False,
    })
}
fn dirname_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let s = path.to_string_lossy();
    let dir = std::path::Path::new(&s)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| ".".to_string());
    Ok(Value::String(PhpString::from_string(dir)))
}
fn basename_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let suffix = args.get(1).map(|v| v.to_php_string());
    let s = path.to_string_lossy();
    let mut base = std::path::Path::new(&s)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();
    if let Some(suf) = suffix {
        let suf_str = suf.to_string_lossy();
        if base.ends_with(&suf_str) && base.len() > suf_str.len() {
            base.truncate(base.len() - suf_str.len());
        }
    }
    Ok(Value::String(PhpString::from_string(base)))
}
fn pathinfo_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
}

// === Regex stubs ===
fn preg_match(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(0))
}
fn preg_match_all(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(0))
}
fn preg_replace(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Return subject unchanged
    Ok(args.get(2).cloned().unwrap_or(Value::Null))
}
fn preg_split(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
}
fn preg_quote(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(args.first().cloned().unwrap_or(Value::Null))
}

// Additional commonly needed stubs
fn register_shutdown_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Null)
}
fn interface_exists_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let name = args.first().unwrap_or(&Value::Null).to_php_string();
    let name_lower: Vec<u8> = name
        .as_bytes()
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
fn gc_enabled_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True)
}
fn gc_disable_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Null)
}
fn gc_enable_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
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
    let s = serialize_value(val);
    Ok(Value::String(PhpString::from_string(s)))
}
fn serialize_value(val: &Value) -> String {
    match val {
        Value::Null | Value::Undef => "N;".to_string(),
        Value::True => "b:1;".to_string(),
        Value::False => "b:0;".to_string(),
        Value::Long(n) => format!("i:{};", n),
        Value::Double(f) => format!("d:{};", f),
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
                result.push_str(&serialize_value(val));
            }
            result.push('}');
            result
        }
        Value::Object(_) | Value::Generator(_) => "N;".to_string(), // TODO: proper object serialization
        Value::Reference(r) => serialize_value(&r.borrow()),
    }
}
fn unserialize_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False) // stub
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
fn putenv_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True)
}
fn spl_autoload_register_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True)
}
fn class_alias_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True)
}
fn is_a_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}
fn is_subclass_of_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
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

fn array_change_key_case_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
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

fn array_diff_key_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.len() < 2 {
        return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
    }
    if let (Some(Value::Array(a)), Some(Value::Array(b))) = (args.first(), args.get(1)) {
        let a = a.borrow();
        let b = b.borrow();
        let b_keys: Vec<_> = b.keys().cloned().collect();
        let mut result = PhpArray::new();
        for (key, val) in a.iter() {
            if !b_keys.contains(key) {
                result.set(key.clone(), val.clone());
            }
        }
        Ok(Value::Array(Rc::new(RefCell::new(result))))
    } else {
        Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
    }
}

fn array_diff_assoc_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.len() < 2 {
        return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
    }
    if let (Some(Value::Array(a)), Some(Value::Array(b))) = (args.first(), args.get(1)) {
        let a = a.borrow();
        let b = b.borrow();
        let mut result = PhpArray::new();
        for (key, val) in a.iter() {
            let b_val = b.get(key);
            if b_val.is_none() || !b_val.unwrap().equals(val) {
                result.set(key.clone(), val.clone());
            }
        }
        Ok(Value::Array(Rc::new(RefCell::new(result))))
    } else {
        Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
    }
}

fn array_intersect_key_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
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

fn array_intersect_assoc_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
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

fn array_diff_uassoc_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Simplified: same as array_diff_assoc (ignores the callback for now)
    array_diff_assoc_fn(_vm, args)
}

fn array_all_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // PHP 8.5: array_all($array, $callback) - returns true if all elements pass callback
    // Without callable support, just check truthiness
    if let Some(Value::Array(arr)) = args.first() {
        let arr = arr.borrow();
        Ok(if arr.values().all(|v| v.is_truthy()) {
            Value::True
        } else {
            Value::False
        })
    } else {
        Ok(Value::False)
    }
}

fn array_any_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let arr = arr.borrow();
        Ok(if arr.values().any(|v| v.is_truthy()) {
            Value::True
        } else {
            Value::False
        })
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
fn array_is_list_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let arr = arr.borrow();
        let is_list = arr
            .iter()
            .enumerate()
            .all(|(i, (k, _))| matches!(k, goro_core::array::ArrayKey::Int(n) if *n == i as i64));
        Ok(if is_list { Value::True } else { Value::False })
    } else {
        Ok(Value::False)
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
        for name in class.methods.keys() {
            result.push(Value::String(PhpString::from_vec(name.clone())));
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
    let arr = args.first().unwrap_or(&Value::Null);
    let callback = args.get(1).unwrap_or(&Value::Null).to_php_string();
    if let Value::Array(a) = arr {
        let entries: Vec<_> = a
            .borrow()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for (key, val) in entries {
            if let Value::Array(_) = &val {
                // Recurse
                let sub_args = vec![val, Value::String(callback.clone())];
                array_walk_recursive_fn(vm, &sub_args)?;
            } else {
                // Call callback(value, key)
                let key_val = match &key {
                    goro_core::array::ArrayKey::Int(n) => Value::Long(*n),
                    goro_core::array::ArrayKey::String(s) => Value::String(s.clone()),
                };
                let cb_args = vec![val, key_val];
                let cb_lower: Vec<u8> = callback
                    .as_bytes()
                    .iter()
                    .map(|b| b.to_ascii_lowercase())
                    .collect();
                if let Some(func) = vm.functions.get(&cb_lower).copied() {
                    func(vm, &cb_args)?;
                } else if let Some(user_fn) = vm.user_functions.get(&cb_lower).cloned() {
                    let mut fn_cvs = vec![Value::Undef; user_fn.cv_names.len()];
                    for (i, arg) in cb_args.iter().enumerate() {
                        if i < fn_cvs.len() {
                            fn_cvs[i] = arg.clone();
                        }
                    }
                    vm.execute_fn(&user_fn, fn_cvs)?;
                }
            }
        }
    }
    Ok(Value::True)
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

// === Additional date/time functions ===

/// gmdate - identical to date() but uses UTC. Since we don't handle timezones yet, this is an alias.
fn gmdate_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // For now, same as date_fn since we don't handle timezones
    date_fn(_vm, args)
}

/// gmmktime - UTC version of mktime
fn gmmktime_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // For now, same as mktime since we don't handle timezones
    mktime(_vm, args)
}

/// strftime - format a timestamp using strftime-style format codes
fn strftime_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let format = args
        .first()
        .unwrap_or(&Value::Null)
        .to_php_string()
        .to_string_lossy();
    let timestamp = args.get(1).map(|v| v.to_long());

    let secs = timestamp.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    });

    let days_since_epoch = secs / 86400;
    let time_of_day = ((secs % 86400) + 86400) % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;
    let (year, month, day) = days_to_ymd(days_since_epoch);

    let dow = (((days_since_epoch % 7) + 4) % 7 + 7) % 7; // 0=Sunday

    let day_names_full = [
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
    ];
    let day_names_short = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    let month_names_full = [
        "",
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let month_names_short = [
        "", "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];

    let mut result = String::new();
    let fmt_bytes = format.as_bytes();
    let mut i = 0;
    while i < fmt_bytes.len() {
        if fmt_bytes[i] == b'%' && i + 1 < fmt_bytes.len() {
            i += 1;
            match fmt_bytes[i] {
                b'Y' => result.push_str(&format!("{:04}", year)),
                b'm' => result.push_str(&format!("{:02}", month)),
                b'd' => result.push_str(&format!("{:02}", day)),
                b'H' => result.push_str(&format!("{:02}", hours)),
                b'M' => result.push_str(&format!("{:02}", minutes)),
                b'S' => result.push_str(&format!("{:02}", seconds)),
                b'A' => {
                    result.push_str(day_names_full[dow as usize % 7]);
                }
                b'a' => {
                    result.push_str(day_names_short[dow as usize % 7]);
                }
                b'B' => {
                    result.push_str(month_names_full[month as usize]);
                }
                b'b' => {
                    result.push_str(month_names_short[month as usize]);
                }
                b'Z' => {
                    result.push_str("UTC");
                }
                b'%' => {
                    result.push('%');
                }
                other => {
                    result.push('%');
                    result.push(other as char);
                }
            }
        } else {
            result.push(fmt_bytes[i] as char);
        }
        i += 1;
    }

    Ok(Value::String(PhpString::from_string(result)))
}

/// date_create - create a DateTime-like value (returns stdClass with timestamp property)
fn date_create_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let datetime_str = args
        .first()
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_default();

    // Get current timestamp as default
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let timestamp = if datetime_str.is_empty() || datetime_str == "now" {
        now_secs
    } else {
        // Very basic parsing: try "Y-m-d H:i:s" or "Y-m-d"
        let parts: Vec<&str> = datetime_str.split(|c: char| c == ' ' || c == 'T').collect();
        let date_parts: Vec<&str> = parts.first().unwrap_or(&"").split('-').collect();
        if date_parts.len() == 3 {
            let year = date_parts[0].parse::<i64>().unwrap_or(1970);
            let month = date_parts[1].parse::<u32>().unwrap_or(1);
            let day = date_parts[2].parse::<u32>().unwrap_or(1);
            let mut h = 0i64;
            let mut m = 0i64;
            let mut s = 0i64;
            if let Some(time_str) = parts.get(1) {
                let time_parts: Vec<&str> = time_str.split(':').collect();
                h = time_parts.first().and_then(|v| v.parse().ok()).unwrap_or(0);
                m = time_parts.get(1).and_then(|v| v.parse().ok()).unwrap_or(0);
                s = time_parts.get(2).and_then(|v| v.parse().ok()).unwrap_or(0);
            }
            let days = ymd_to_days(year, month, day);
            days * 86400 + h * 3600 + m * 60 + s
        } else {
            now_secs
        }
    };

    // Return a stdClass-like object with a timestamp property
    let obj_id = _vm.next_object_id();
    let mut obj = PhpObject::new(b"stdClass".to_vec(), obj_id);
    obj.set_property(b"timestamp".to_vec(), Value::Long(timestamp));
    Ok(Value::Object(Rc::new(RefCell::new(obj))))
}

/// getdate - return associative array with date components
fn getdate_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let timestamp = args.get(0).map(|v| v.to_long());

    let secs = timestamp.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    });

    let days_since_epoch = secs / 86400;
    let time_of_day = ((secs % 86400) + 86400) % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds_val = time_of_day % 60;
    let (year, month, day) = days_to_ymd(days_since_epoch);

    let dow = (((days_since_epoch % 7) + 4) % 7 + 7) % 7; // 0=Sunday

    let day_names = [
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
    ];
    let month_names = [
        "",
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];

    // Compute day of year (0-based)
    let days_in_months = [0, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let is_leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let mut yday = 0i64;
    for m in 1..month {
        yday += days_in_months[m as usize] as i64;
        if m == 2 && is_leap {
            yday += 1;
        }
    }
    yday += (day as i64) - 1;

    let mut result = PhpArray::new();
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"seconds")),
        Value::Long(seconds_val),
    );
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"minutes")),
        Value::Long(minutes),
    );
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"hours")),
        Value::Long(hours),
    );
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"mday")),
        Value::Long(day as i64),
    );
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"wday")),
        Value::Long(dow),
    );
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"mon")),
        Value::Long(month as i64),
    );
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"year")),
        Value::Long(year),
    );
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"yday")),
        Value::Long(yday),
    );
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"weekday")),
        Value::String(PhpString::from_string(
            day_names[dow as usize % 7].to_string(),
        )),
    );
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"month")),
        Value::String(PhpString::from_string(
            month_names[month as usize].to_string(),
        )),
    );
    result.set(ArrayKey::Int(0), Value::Long(secs));

    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

/// localtime - return array of date components
fn localtime_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let timestamp = args.get(0).map(|v| v.to_long());
    let is_assoc = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);

    let secs = timestamp.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    });

    let days_since_epoch = secs / 86400;
    let time_of_day = ((secs % 86400) + 86400) % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds_val = time_of_day % 60;
    let (year, month, day) = days_to_ymd(days_since_epoch);

    let dow = (((days_since_epoch % 7) + 4) % 7 + 7) % 7; // 0=Sunday

    // Compute day of year (0-based)
    let days_in_months = [0, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let is_leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let mut yday = 0i64;
    for m in 1..month {
        yday += days_in_months[m as usize] as i64;
        if m == 2 && is_leap {
            yday += 1;
        }
    }
    yday += (day as i64) - 1;

    let mut result = PhpArray::new();

    if is_assoc {
        result.set(
            ArrayKey::String(PhpString::from_bytes(b"tm_sec")),
            Value::Long(seconds_val),
        );
        result.set(
            ArrayKey::String(PhpString::from_bytes(b"tm_min")),
            Value::Long(minutes),
        );
        result.set(
            ArrayKey::String(PhpString::from_bytes(b"tm_hour")),
            Value::Long(hours),
        );
        result.set(
            ArrayKey::String(PhpString::from_bytes(b"tm_mday")),
            Value::Long(day as i64),
        );
        result.set(
            ArrayKey::String(PhpString::from_bytes(b"tm_mon")),
            Value::Long((month as i64) - 1),
        ); // 0-based month
        result.set(
            ArrayKey::String(PhpString::from_bytes(b"tm_year")),
            Value::Long(year - 1900),
        ); // years since 1900
        result.set(
            ArrayKey::String(PhpString::from_bytes(b"tm_wday")),
            Value::Long(dow),
        );
        result.set(
            ArrayKey::String(PhpString::from_bytes(b"tm_yday")),
            Value::Long(yday),
        );
        result.set(
            ArrayKey::String(PhpString::from_bytes(b"tm_isdst")),
            Value::Long(0),
        ); // no DST support
    } else {
        result.push(Value::Long(seconds_val)); // 0: tm_sec
        result.push(Value::Long(minutes)); // 1: tm_min
        result.push(Value::Long(hours)); // 2: tm_hour
        result.push(Value::Long(day as i64)); // 3: tm_mday
        result.push(Value::Long((month as i64) - 1)); // 4: tm_mon (0-based)
        result.push(Value::Long(year - 1900)); // 5: tm_year (years since 1900)
        result.push(Value::Long(dow)); // 6: tm_wday
        result.push(Value::Long(yday)); // 7: tm_yday
        result.push(Value::Long(0)); // 8: tm_isdst
    }

    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

/// checkdate - validate a date
fn checkdate_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let month = args.first().map(|v| v.to_long()).unwrap_or(0);
    let day = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let year = args.get(2).map(|v| v.to_long()).unwrap_or(0);

    if year < 1 || year > 32767 || month < 1 || month > 12 || day < 1 {
        return Ok(Value::False);
    }

    let is_leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let max_day = match month {
        2 => {
            if is_leap {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };

    if day > max_day {
        Ok(Value::False)
    } else {
        Ok(Value::True)
    }
}

/// idate - return a single date component as integer
fn idate_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let format = args
        .first()
        .unwrap_or(&Value::Null)
        .to_php_string()
        .to_string_lossy();
    let timestamp = args.get(1).map(|v| v.to_long());

    let secs = timestamp.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    });

    let days_since_epoch = secs / 86400;
    let time_of_day = ((secs % 86400) + 86400) % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds_val = time_of_day % 60;
    let (year, month, day) = days_to_ymd(days_since_epoch);
    let dow = (((days_since_epoch % 7) + 4) % 7 + 7) % 7;

    // Compute day of year (0-based)
    let days_in_months_arr = [0, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let is_leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let mut yday = 0i64;
    for m in 1..month {
        yday += days_in_months_arr[m as usize] as i64;
        if m == 2 && is_leap {
            yday += 1;
        }
    }
    yday += (day as i64) - 1;

    let days_in_month = match month {
        2 => {
            if is_leap {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };

    let c = format.as_bytes().first().copied().unwrap_or(b'U');
    let val = match c {
        b'B' => {
            // Swatch internet time
            let beat = ((secs + 3600) % 86400) as f64 / 86.4;
            beat as i64
        }
        b'd' => day as i64,
        b'h' => {
            let h12 = hours % 12;
            if h12 == 0 { 12 } else { h12 }
        }
        b'H' => hours,
        b'i' => minutes,
        b'I' => 0, // no DST support
        b'L' => {
            if is_leap {
                1
            } else {
                0
            }
        }
        b'm' => month as i64,
        b's' => seconds_val,
        b't' => days_in_month,
        b'U' => secs,
        b'w' => dow,
        b'W' => {
            // ISO week number (simplified)
            (yday / 7 + 1) as i64
        }
        b'y' => year % 100,
        b'Y' => year,
        b'z' => yday,
        b'Z' => 0, // timezone offset, 0 for UTC
        _ => 0,
    };

    Ok(Value::Long(val))
}
