use goro_core::array::PhpArray;
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
    vm.register_function(b"array_combine", array_combine);
    vm.register_function(b"array_chunk", array_chunk);
    vm.register_function(b"array_pad", array_pad);
    vm.register_function(b"array_fill", array_fill);
    vm.register_function(b"array_diff", array_diff);
    vm.register_function(b"array_intersect", array_intersect);
    vm.register_function(b"sort", sort_fn);
    vm.register_function(b"rsort", rsort_fn);
    vm.register_function(b"asort", asort_fn);
    vm.register_function(b"arsort", arsort_fn);
    vm.register_function(b"ksort", ksort_fn);
    vm.register_function(b"krsort", krsort_fn);
    vm.register_function(b"shuffle", shuffle_fn);
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
    vm.register_function(b"array_first", array_first_fn);
    vm.register_function(b"array_last", array_last_fn);
    vm.register_function(b"array_change_key_case", array_change_key_case_fn);
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

    // Date
    vm.register_function(b"time", time_fn);
    vm.register_function(b"microtime", microtime);
    vm.register_function(b"date", date_fn);
    vm.register_function(b"mktime", mktime);
    vm.register_function(b"strtotime", strtotime);

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
    vm.register_function(b"htmlspecialchars", htmlspecialchars);
    vm.register_function(b"htmlentities", htmlentities);
    vm.register_function(b"html_entity_decode", html_entity_decode);
    vm.register_function(b"htmlspecialchars_decode", htmlspecialchars_decode);
    vm.register_function(b"crc32", crc32_fn);
    vm.register_function(b"md5", md5_fn);
    vm.register_function(b"sha1", sha1_fn);
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
    vm.register_function(b"sscanf", sscanf_fn);
    vm.register_function(b"ctype_alpha", ctype_alpha);
    vm.register_function(b"ctype_digit", ctype_digit);
    vm.register_function(b"ctype_alnum", ctype_alnum);
    vm.register_function(b"ctype_upper", ctype_upper);
    vm.register_function(b"ctype_lower", ctype_lower);
    vm.register_function(b"ctype_space", ctype_space);

    // JSON
    vm.register_function(b"json_encode", json_encode);
    vm.register_function(b"json_decode", json_decode);
    vm.register_function(b"json_last_error", json_last_error);
    vm.register_function(b"json_last_error_msg", json_last_error_msg);

    // File stubs (return false/null for now)
    vm.register_function(b"file_exists", file_exists_fn);
    vm.register_function(b"is_file", is_file_fn);
    vm.register_function(b"is_dir", is_dir_fn);
    vm.register_function(b"is_readable", is_readable_fn);
    vm.register_function(b"is_writable", is_writable_fn);
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

fn error_reporting(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Stub: just return the current level (E_ALL)
    Ok(Value::Long(32767))
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

fn trigger_error(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
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
    Ok(vm.constants.get(name.as_bytes()).cloned().unwrap_or(Value::Null))
}

// === Output buffering ===

fn ob_start(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True)
}
fn ob_end_clean(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::True) }
fn ob_end_flush(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::True) }
fn ob_get_contents(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(PhpString::empty()))
}
fn ob_get_clean(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(PhpString::empty()))
}
fn ob_get_level(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::Long(0)) }
fn ob_flush(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::True) }
fn ob_implicit_flush(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::Null) }

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
fn function_exists(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}
fn is_callable(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}
fn call_user_func(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Null)
}
fn call_user_func_array(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Null)
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
        if len == 0 { return Ok(Value::Null); }
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
        let mut arr = arr.borrow_mut();
        if arr.is_empty() { return Ok(Value::Null); }
        let entries: Vec<_> = arr.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let (first_key, first_val) = entries.first().unwrap();
        arr.remove(first_key);
        Ok(first_val.clone())
    } else {
        Ok(Value::Null)
    }
}

fn array_unshift(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(0)) // stub
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

        let start = if offset < 0 { (entries.len() as i64 + offset).max(0) as usize } else { offset as usize };
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

fn array_splice(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new())))) // stub
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
        Ok(if arr.contains_key(&k) { Value::True } else { Value::False })
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
            if strict { if val.identical(needle) { return Ok(Value::True); } }
            else { if val.equals(needle) { return Ok(Value::True); } }
        }
    }
    Ok(Value::False)
}

fn array_map(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // TODO: needs closure/callable support
    if let Some(Value::Array(arr)) = _args.get(1) {
        Ok(Value::Array(arr.clone()))
    } else {
        Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
    }
}

fn array_filter(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let arr = arr.borrow();
        let mut result = PhpArray::new();
        for (key, val) in arr.iter() {
            if val.is_truthy() {
                result.set(key.clone(), val.clone());
            }
        }
        Ok(Value::Array(Rc::new(RefCell::new(result))))
    } else {
        Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
    }
}

fn array_walk(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True)
}

fn array_combine(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let keys = match args.first() { Some(Value::Array(a)) => a.borrow(), _ => return Ok(Value::False) };
    let vals = match args.get(1) { Some(Value::Array(a)) => a.borrow(), _ => return Ok(Value::False) };
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
            for val in chunk { sub.push(val.clone()); }
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

fn array_diff(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.len() < 2 { return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new())))); }
    if let (Some(Value::Array(a)), Some(Value::Array(b))) = (args.first(), args.get(1)) {
        let a = a.borrow();
        let b = b.borrow();
        let b_vals: Vec<_> = b.values().map(|v| v.to_php_string().as_bytes().to_vec()).collect();
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
    if args.len() < 2 { return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new())))); }
    if let (Some(Value::Array(a)), Some(Value::Array(b))) = (args.first(), args.get(1)) {
        let a = a.borrow();
        let b = b.borrow();
        let b_vals: Vec<_> = b.values().map(|v| v.to_php_string().as_bytes().to_vec()).collect();
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

fn sort_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::True) }
fn rsort_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::True) }
fn asort_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::True) }
fn arsort_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::True) }
fn ksort_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::True) }
fn krsort_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::True) }
fn shuffle_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::True) }

fn range_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let start = args.first().map(|v| v.to_long()).unwrap_or(0);
    let end = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let step = args.get(2).map(|v| v.to_long()).unwrap_or(1).max(1);
    let mut result = PhpArray::new();
    if start <= end {
        let mut i = start;
        while i <= end { result.push(Value::Long(i)); i += step; }
    } else {
        let mut i = start;
        while i >= end { result.push(Value::Long(i)); i -= step; }
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn compact(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
}

fn current(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let arr = arr.borrow();
        Ok(arr.iter().next().map(|(_, v)| v.clone()).unwrap_or(Value::False))
    } else { Ok(Value::False) }
}

fn next_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn prev_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn reset_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let arr = arr.borrow();
        Ok(arr.iter().next().map(|(_, v)| v.clone()).unwrap_or(Value::False))
    } else { Ok(Value::False) }
}
fn end_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let arr = arr.borrow();
        Ok(arr.iter().last().map(|(_, v)| v.clone()).unwrap_or(Value::False))
    } else { Ok(Value::False) }
}
fn key_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let arr = arr.borrow();
        Ok(arr.iter().next().map(|(k, _)| match k {
            goro_core::array::ArrayKey::Int(n) => Value::Long(*n),
            goro_core::array::ArrayKey::String(s) => Value::String(s.clone()),
        }).unwrap_or(Value::Null))
    } else { Ok(Value::Null) }
}

// === Misc ===

fn ini_set(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn ini_get(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn ini_restore(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::Null) }
fn set_time_limit(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::True) }
fn php_assert(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    Ok(if val.is_truthy() { Value::True } else { Value::False })
}
fn class_exists(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let name = args.first().unwrap_or(&Value::Null).to_php_string();
    let name_lower: Vec<u8> = name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
    // Check built-in classes
    let is_builtin = matches!(name_lower.as_slice(),
        b"stdclass" | b"exception" | b"error" | b"typeerror" | b"valueerror"
        | b"runtimeexception" | b"logicexception" | b"invalidargumentexception"
        | b"badmethodcallexception" | b"closure" | b"generator"
    );
    Ok(if is_builtin || vm.classes.contains_key(&name_lower) { Value::True } else { Value::False })
}
fn get_class(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn get_declared_classes(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
}
fn property_exists(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn method_exists(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn is_object(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(match args.first() {
        Some(Value::Object(_)) => Value::True,
        _ => Value::False,
    })
}
fn date_default_timezone_set(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::True) }
fn setlocale(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn debug_zval_refcount(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::Null) }
fn extract_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::Long(0)) }
fn array_column(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
}
fn array_count_values(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
}
fn array_rand(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::Long(0)) }

// === Date/Time ===

fn time_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    use std::time::SystemTime;
    let secs = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs();
    Ok(Value::Long(secs as i64))
}

fn microtime(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    use std::time::SystemTime;
    let dur = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
    let as_float = args.first().map(|v| v.is_truthy()).unwrap_or(false);
    if as_float {
        Ok(Value::Double(dur.as_secs_f64()))
    } else {
        Ok(Value::String(PhpString::from_string(format!("{:.8} {}", dur.subsec_nanos() as f64 / 1e9, dur.as_secs()))))
    }
}

fn date_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(PhpString::from_bytes(b"2026-03-15")))
}
fn mktime(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::Long(0)) }
fn strtotime(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }

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
    Ok(Value::String(PhpString::from_string(format!("{:.prec$}", num, prec = decimals))))
}

fn money_format(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(PhpString::empty()))
}

fn hex2bin(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let hex = s.as_bytes();
    if hex.len() % 2 != 0 { return Ok(Value::False); }
    let mut result = Vec::with_capacity(hex.len() / 2);
    for i in (0..hex.len()).step_by(2) {
        let byte = u8::from_str_radix(&String::from_utf8_lossy(&hex[i..i+2]), 16);
        match byte { Ok(b) => result.push(b), Err(_) => return Ok(Value::False) }
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
            _ => { result.extend_from_slice(format!("%{:02X}", b).as_bytes()); }
        }
    }
    Ok(Value::String(PhpString::from_vec(result)))
}
fn urldecode(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::String(PhpString::empty())) }
fn rawurlencode(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let mut result = Vec::new();
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => result.push(b),
            _ => { result.extend_from_slice(format!("%{:02X}", b).as_bytes()); }
        }
    }
    Ok(Value::String(PhpString::from_vec(result)))
}
fn rawurldecode(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::String(PhpString::empty())) }
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
fn htmlentities(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> { htmlspecialchars(_vm, args) }
fn html_entity_decode(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::String(PhpString::empty())) }
fn htmlspecialchars_decode(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::String(PhpString::empty())) }
fn crc32_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::Long(0)) }
fn md5_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::String(PhpString::empty())) }
fn sha1_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::String(PhpString::empty())) }
fn str_word_count(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    Ok(Value::Long(s.to_string_lossy().split_whitespace().count() as i64))
}
fn substr_count(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let haystack = args.first().unwrap_or(&Value::Null).to_php_string();
    let needle = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() { return Ok(Value::Long(0)); }
    let mut count = 0i64;
    let mut i = 0;
    while i + n.len() <= h.len() {
        if &h[i..i+n.len()] == n { count += 1; i += n.len(); } else { i += 1; }
    }
    Ok(Value::Long(count))
}
fn substr_replace(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::String(PhpString::empty())) }
fn str_ireplace(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::String(PhpString::empty())) }
fn stripos(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let h = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy().to_lowercase();
    let n = args.get(1).unwrap_or(&Value::Null).to_php_string().to_string_lossy().to_lowercase();
    match h.find(&n) { Some(pos) => Ok(Value::Long(pos as i64)), None => Ok(Value::False) }
}
fn strrpos(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let h = args.first().unwrap_or(&Value::Null).to_php_string();
    let n = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let hb = h.as_bytes(); let nb = n.as_bytes();
    if nb.is_empty() { return Ok(Value::False); }
    for i in (0..=(hb.len().saturating_sub(nb.len()))).rev() {
        if &hb[i..i+nb.len()] == nb { return Ok(Value::Long(i as i64)); }
    }
    Ok(Value::False)
}
fn strripos(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let h = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy().to_lowercase();
    let n = args.get(1).unwrap_or(&Value::Null).to_php_string().to_string_lossy().to_lowercase();
    match h.rfind(&n) { Some(pos) => Ok(Value::Long(pos as i64)), None => Ok(Value::False) }
}
fn strcmp(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = args.first().unwrap_or(&Value::Null).to_php_string();
    let b = args.get(1).unwrap_or(&Value::Null).to_php_string();
    Ok(Value::Long(a.as_bytes().cmp(b.as_bytes()) as i64))
}
fn strncmp(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = args.first().unwrap_or(&Value::Null).to_php_string();
    let b = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let n = args.get(2).map(|v| v.to_long()).unwrap_or(0) as usize;
    let sa = &a.as_bytes()[..n.min(a.len())];
    let sb = &b.as_bytes()[..n.min(b.len())];
    Ok(Value::Long(sa.cmp(sb) as i64))
}
fn strcasecmp(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy().to_lowercase();
    let b = args.get(1).unwrap_or(&Value::Null).to_php_string().to_string_lossy().to_lowercase();
    Ok(Value::Long(a.cmp(&b) as i64))
}
fn strncasecmp(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy().to_lowercase();
    let b = args.get(1).unwrap_or(&Value::Null).to_php_string().to_string_lossy().to_lowercase();
    let n = args.get(2).map(|v| v.to_long()).unwrap_or(0) as usize;
    let sa = &a[..n.min(a.len())];
    let sb = &b[..n.min(b.len())];
    Ok(Value::Long(sa.cmp(sb) as i64))
}
fn str_contains_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let h = args.first().unwrap_or(&Value::Null).to_php_string();
    let n = args.get(1).unwrap_or(&Value::Null).to_php_string();
    if n.is_empty() { return Ok(Value::True); }
    let hb = h.as_bytes(); let nb = n.as_bytes();
    for i in 0..=hb.len().saturating_sub(nb.len()) {
        if &hb[i..i+nb.len()] == nb { return Ok(Value::True); }
    }
    Ok(Value::False)
}
fn wordwrap(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::String(PhpString::empty())) }
fn printf(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Use the sprintf implementation from strings module
    let formatted = crate::strings::do_sprintf(args);
    let len = formatted.len();
    vm.write_output(formatted.as_bytes());
    Ok(Value::Long(len as i64))
}
fn fprintf_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::Long(0)) }
fn sscanf_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::Null) }
fn ctype_alpha(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    Ok(if !s.is_empty() && s.as_bytes().iter().all(|b| b.is_ascii_alphabetic()) { Value::True } else { Value::False })
}
fn ctype_digit(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    Ok(if !s.is_empty() && s.as_bytes().iter().all(|b| b.is_ascii_digit()) { Value::True } else { Value::False })
}
fn ctype_alnum(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    Ok(if !s.is_empty() && s.as_bytes().iter().all(|b| b.is_ascii_alphanumeric()) { Value::True } else { Value::False })
}
fn ctype_upper(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    Ok(if !s.is_empty() && s.as_bytes().iter().all(|b| b.is_ascii_uppercase()) { Value::True } else { Value::False })
}
fn ctype_lower(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    Ok(if !s.is_empty() && s.as_bytes().iter().all(|b| b.is_ascii_lowercase()) { Value::True } else { Value::False })
}
fn ctype_space(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    Ok(if !s.is_empty() && s.as_bytes().iter().all(|b| b.is_ascii_whitespace()) { Value::True } else { Value::False })
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
            if f.is_infinite() || f.is_nan() { "null".to_string() } else { format!("{}", f) }
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
            let is_list = arr.iter().enumerate().all(|(i, (k, _))| {
                matches!(k, goro_core::array::ArrayKey::Int(n) if *n == i as i64)
            });
            if is_list {
                let parts: Vec<String> = arr.values().map(|v| json_encode_value(v)).collect();
                format!("[{}]", parts.join(","))
            } else {
                let parts: Vec<String> = arr.iter().map(|(k, v)| {
                    let key_str = match k {
                        goro_core::array::ArrayKey::Int(n) => format!("\"{}\"", n),
                        goro_core::array::ArrayKey::String(s) => format!("\"{}\"", s.to_string_lossy()),
                    };
                    format!("{}:{}", key_str, json_encode_value(v))
                }).collect();
                format!("{{{}}}", parts.join(","))
            }
        }
        Value::Object(_) => "null".to_string(), // TODO: implement object JSON encoding
    }
}

fn json_decode(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Null) // stub
}
fn json_last_error(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::Long(0)) }
fn json_last_error_msg(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(PhpString::from_bytes(b"No error")))
}

// === File stubs ===
fn file_exists_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn is_file_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn is_dir_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn is_readable_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn is_writable_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn file_get_contents_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn file_put_contents_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn realpath_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn dirname_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let s = path.to_string_lossy();
    let dir = std::path::Path::new(&s).parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|| ".".to_string());
    Ok(Value::String(PhpString::from_string(dir)))
}
fn basename_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    let suffix = args.get(1).map(|v| v.to_php_string());
    let s = path.to_string_lossy();
    let mut base = std::path::Path::new(&s).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
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
fn preg_match(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::Long(0)) }
fn preg_match_all(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::Long(0)) }
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
fn register_shutdown_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::Null) }
fn interface_exists_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn trait_exists_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn gc_collect_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::Long(0)) }
fn gc_enabled_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::True) }
fn gc_disable_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::Null) }
fn gc_enable_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::Null) }
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
                    goro_core::array::ArrayKey::String(s) => result.push_str(&format!("s:{}:\"{}\";", s.len(), s.to_string_lossy())),
                }
                result.push_str(&serialize_value(val));
            }
            result.push('}');
            result
        }
        Value::Object(_) => "N;".to_string(), // TODO: proper object serialization
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
fn sleep_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::Long(0)) }
fn usleep_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::Null) }
fn uniqid_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    use std::time::SystemTime;
    let t = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
    Ok(Value::String(PhpString::from_string(format!("{:x}{:05x}", t.as_secs(), t.subsec_micros()))))
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
fn putenv_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::True) }
fn spl_autoload_register_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::True) }
fn class_alias_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::True) }
fn is_a_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn is_subclass_of_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn get_parent_class_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn get_called_class_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn get_defined_vars_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
}
fn get_defined_functions_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
}
fn array_first_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let arr = arr.borrow();
        Ok(arr.iter().next().map(|(_, v)| v.clone()).unwrap_or(Value::Null))
    } else { Ok(Value::Null) }
}
fn array_last_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let arr = arr.borrow();
        Ok(arr.iter().last().map(|(_, v)| v.clone()).unwrap_or(Value::Null))
    } else { Ok(Value::Null) }
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
    let html = format!("<code><span style=\"color: #000000\">{}</span>\n</code>", code.to_string_lossy());
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
fn fclose_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::True) }
fn fread_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn fwrite_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn fgets_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn feof_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::True) }
fn rewind_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::True) }
fn fseek_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::Long(0)) }
fn ftell_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::Long(0)) }
fn fflush_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::True) }
fn unlink_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn rename_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn copy_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn mkdir_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn rmdir_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn glob_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new())))) }
fn scandir_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn header_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::Null) }
fn headers_sent_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn http_response_code_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::Long(200)) }
