pub mod math;
pub mod misc;
pub mod output;
pub mod regex;
pub mod strings;
pub mod type_funcs;

use goro_core::vm::Vm;

/// Register all standard extension functions
pub fn register_standard_functions(vm: &mut Vm) {
    vm.register_extension(b"standard");
    output::register(vm);
    strings::register(vm);
    type_funcs::register(vm);
    math::register(vm);
    misc::register(vm);
    regex::register(vm);
    register_builtin_param_names(vm);
}

/// Register parameter names for built-in functions (for named argument support)
fn register_builtin_param_names(vm: &mut Vm) {
    // Helper macro to register param names
    macro_rules! params {
        ($name:expr) => {
            vm.builtin_param_names.insert($name.to_vec(), vec![]);
        };
        ($name:expr, $($p:expr),+) => {
            vm.builtin_param_names.insert($name.to_vec(), vec![$($p.to_vec()),+]);
        }
    }

    // String functions
    params!(b"strlen", b"string");
    params!(b"substr", b"string", b"offset", b"length");
    params!(b"strpos", b"haystack", b"needle", b"offset");
    params!(b"strrpos", b"haystack", b"needle", b"offset");
    params!(b"str_contains", b"haystack", b"needle");
    params!(b"str_starts_with", b"haystack", b"prefix");
    params!(b"str_ends_with", b"haystack", b"suffix");
    params!(b"str_replace", b"search", b"replace", b"subject", b"count");
    params!(b"str_ireplace", b"search", b"replace", b"subject", b"count");
    params!(b"str_pad", b"string", b"length", b"pad_string", b"pad_type");
    params!(b"str_repeat", b"string", b"times");
    params!(b"str_word_count", b"string", b"format", b"characters");
    params!(b"str_split", b"string", b"length");
    params!(b"strrev", b"string");
    params!(b"strtolower", b"string");
    params!(b"strtoupper", b"string");
    params!(b"trim", b"string", b"characters");
    params!(b"ltrim", b"string", b"characters");
    params!(b"rtrim", b"string", b"characters");
    params!(b"explode", b"separator", b"string", b"limit");
    params!(b"implode", b"separator", b"array");
    params!(b"sprintf", b"format", b"values");
    params!(b"printf", b"format", b"values");
    params!(b"substr_count", b"haystack", b"needle", b"offset", b"length");
    params!(b"substr_replace", b"string", b"replace", b"offset", b"length");
    params!(b"str_getcsv", b"string", b"separator", b"enclosure", b"escape");
    params!(b"nl2br", b"string", b"use_xhtml");
    params!(b"chunk_split", b"string", b"length", b"separator");
    params!(b"wordwrap", b"string", b"width", b"break_str", b"cut_long_words");
    params!(b"ucfirst", b"string");
    params!(b"lcfirst", b"string");
    params!(b"ucwords", b"string", b"separators");
    params!(b"md5", b"string", b"binary");
    params!(b"sha1", b"string", b"binary");
    params!(b"crc32", b"string");
    params!(b"number_format", b"num", b"decimals", b"decimal_separator", b"thousands_separator");
    params!(b"ord", b"character");
    params!(b"chr", b"codepoint");
    params!(b"hex2bin", b"string");
    params!(b"bin2hex", b"string");
    params!(b"base64_encode", b"string");
    params!(b"base64_decode", b"string", b"strict");
    params!(b"htmlspecialchars", b"string", b"flags", b"encoding", b"double_encode");
    params!(b"htmlentities", b"string", b"flags", b"encoding", b"double_encode");
    params!(b"htmlspecialchars_decode", b"string", b"flags");
    params!(b"html_entity_decode", b"string", b"flags", b"encoding");
    params!(b"strip_tags", b"string", b"allowed_tags");
    params!(b"addslashes", b"string");
    params!(b"stripslashes", b"string");
    params!(b"addcslashes", b"string", b"characters");
    params!(b"stripcslashes", b"string");
    params!(b"quoted_printable_encode", b"string");
    params!(b"quoted_printable_decode", b"string");
    params!(b"rawurlencode", b"string");
    params!(b"rawurldecode", b"string");
    params!(b"urlencode", b"string");
    params!(b"urldecode", b"string");
    params!(b"http_build_query", b"data", b"numeric_prefix", b"arg_separator", b"encoding_type");
    params!(b"convert_uuencode", b"string");
    params!(b"convert_uudecode", b"string");
    params!(b"str_rot13", b"string");
    params!(b"ctype_alnum", b"text");
    params!(b"ctype_alpha", b"text");
    params!(b"ctype_cntrl", b"text");
    params!(b"ctype_digit", b"text");
    params!(b"ctype_graph", b"text");
    params!(b"ctype_lower", b"text");
    params!(b"ctype_print", b"text");
    params!(b"ctype_punct", b"text");
    params!(b"ctype_space", b"text");
    params!(b"ctype_upper", b"text");
    params!(b"ctype_xdigit", b"text");

    params!(b"exec", b"command", b"output", b"result_code");
    params!(b"system", b"command", b"result_code");
    params!(b"shell_exec", b"command");
    params!(b"passthru", b"command", b"result_code");
    params!(b"escapeshellarg", b"arg");
    params!(b"escapeshellcmd", b"command");
    params!(b"mb_strcut", b"string", b"start", b"length", b"encoding");
    params!(b"mb_detect_order", b"encoding");

    // Array functions
    params!(b"array_keys", b"array", b"filter_value", b"strict");
    params!(b"array_values", b"array");
    params!(b"array_unique", b"array", b"flags");
    params!(b"array_flip", b"array");
    params!(b"array_reverse", b"array", b"preserve_keys");
    params!(b"array_slice", b"array", b"offset", b"length", b"preserve_keys");
    params!(b"array_splice", b"array", b"offset", b"length", b"replacement");
    params!(b"array_search", b"needle", b"haystack", b"strict");
    params!(b"array_key_exists", b"key", b"array");
    params!(b"array_pop", b"array");
    params!(b"array_push", b"array", b"values");
    params!(b"array_shift", b"array");
    params!(b"array_unshift", b"array", b"values");
    params!(b"array_combine", b"keys", b"values");
    params!(b"array_chunk", b"array", b"length", b"preserve_keys");
    params!(b"array_pad", b"array", b"length", b"value");
    params!(b"array_fill", b"start_index", b"count", b"value");
    params!(b"array_fill_keys", b"keys", b"value");
    params!(b"array_column", b"array", b"column_key", b"index_key");
    params!(b"array_count_values", b"array");
    params!(b"array_map", b"callback", b"array", b"arrays");
    params!(b"array_filter", b"array", b"callback", b"mode");
    params!(b"array_walk", b"array", b"callback", b"arg");
    params!(b"array_walk_recursive", b"array", b"callback", b"arg");
    params!(b"array_reduce", b"array", b"callback", b"initial");
    params!(b"array_sum", b"array");
    params!(b"array_product", b"array");
    params!(b"array_rand", b"array", b"num");
    params!(b"array_merge", b"arrays");
    params!(b"array_merge_recursive", b"arrays");
    params!(b"array_intersect", b"array", b"arrays");
    params!(b"array_intersect_key", b"array", b"arrays");
    params!(b"array_diff", b"array", b"arrays");
    params!(b"array_diff_key", b"array", b"arrays");
    params!(b"array_diff_assoc", b"array", b"arrays");
    params!(b"array_intersect_assoc", b"array", b"arrays");
    params!(b"in_array", b"needle", b"haystack", b"strict");
    params!(b"count", b"value", b"mode");
    params!(b"sizeof", b"value", b"mode");
    params!(b"sort", b"array", b"flags");
    params!(b"rsort", b"array", b"flags");
    params!(b"asort", b"array", b"flags");
    params!(b"arsort", b"array", b"flags");
    params!(b"ksort", b"array", b"flags");
    params!(b"krsort", b"array", b"flags");
    params!(b"usort", b"array", b"callback");
    params!(b"uasort", b"array", b"callback");
    params!(b"uksort", b"array", b"callback");
    params!(b"array_multisort", b"array", b"rest");
    params!(b"compact", b"var_names", b"vars");
    params!(b"extract", b"array", b"flags", b"prefix");
    params!(b"range", b"start", b"end", b"step");
    params!(b"list", b"vars");
    params!(b"shuffle", b"array");

    // Math functions
    params!(b"abs", b"num");
    params!(b"ceil", b"num");
    params!(b"floor", b"num");
    params!(b"round", b"num", b"precision", b"mode");
    params!(b"max", b"value", b"values");
    params!(b"min", b"value", b"values");
    params!(b"pow", b"base", b"exp");
    params!(b"sqrt", b"num");
    params!(b"log", b"num", b"base");
    params!(b"log2", b"num");
    params!(b"log10", b"num");
    params!(b"intdiv", b"num1", b"num2");
    params!(b"fmod", b"num1", b"num2");
    params!(b"base_convert", b"num", b"from_base", b"to_base");
    params!(b"bindec", b"binary_string");
    params!(b"octdec", b"octal_string");
    params!(b"hexdec", b"hex_string");
    params!(b"decbin", b"num");
    params!(b"decoct", b"num");
    params!(b"dechex", b"num");
    params!(b"pi");
    params!(b"sin", b"num");
    params!(b"cos", b"num");
    params!(b"tan", b"num");
    params!(b"asin", b"num");
    params!(b"acos", b"num");
    params!(b"atan", b"num");
    params!(b"atan2", b"y", b"x");
    params!(b"exp", b"num");
    params!(b"rand", b"min", b"max");
    params!(b"mt_rand", b"min", b"max");
    params!(b"random_int", b"min", b"max");

    // Type functions
    params!(b"gettype", b"value");
    params!(b"settype", b"var", b"type");
    params!(b"intval", b"value", b"base");
    params!(b"floatval", b"value");
    params!(b"strval", b"value");
    params!(b"boolval", b"value");
    params!(b"is_null", b"value");
    params!(b"is_bool", b"value");
    params!(b"is_int", b"value");
    params!(b"is_integer", b"value");
    params!(b"is_long", b"value");
    params!(b"is_float", b"value");
    params!(b"is_double", b"value");
    params!(b"is_string", b"value");
    params!(b"is_array", b"value");
    params!(b"is_object", b"value");
    params!(b"is_numeric", b"value");
    params!(b"is_callable", b"value", b"syntax_only", b"callable_name");
    params!(b"is_a", b"object", b"class_name", b"allow_string");
    params!(b"is_subclass_of", b"object_or_class", b"class_name", b"allow_string");
    params!(b"isset", b"var");
    params!(b"unset", b"var");
    params!(b"empty", b"var");
    params!(b"var_dump", b"value", b"values");
    params!(b"print_r", b"value", b"return");
    params!(b"var_export", b"value", b"return");
    params!(b"debug_zval_refcount", b"variable");

    // Misc functions
    params!(b"assert", b"assertion", b"description");
    params!(b"call_user_func", b"callback", b"args");
    params!(b"call_user_func_array", b"callback", b"args");
    params!(b"defined", b"constant_name");
    params!(b"define", b"constant_name", b"value", b"case_insensitive");
    params!(b"constant", b"name");
    params!(b"function_exists", b"function");
    params!(b"class_exists", b"class", b"autoload");
    params!(b"interface_exists", b"interface", b"autoload");
    params!(b"enum_exists", b"enum", b"autoload");
    params!(b"get_class", b"object");
    params!(b"get_parent_class", b"object_or_class");
    params!(b"property_exists", b"object_or_class", b"property");
    params!(b"method_exists", b"object_or_class", b"method");
    params!(b"trigger_error", b"message", b"error_level");
    params!(b"user_error", b"message", b"error_level");
    params!(b"set_error_handler", b"callback", b"error_levels");
    params!(b"restore_error_handler");
    params!(b"set_exception_handler", b"callback");
    params!(b"restore_exception_handler");
    params!(b"header", b"header", b"replace", b"response_code");
    params!(b"http_response_code", b"response_code");
    params!(b"php_uname", b"mode");
    params!(b"php_sapi_name");
    params!(b"phpversion", b"extension");

    // Output functions
    params!(b"echo", b"arg");
    params!(b"print", b"arg");
    params!(b"ob_start", b"callback", b"chunk_size", b"flags");
    params!(b"ob_end_clean");
    params!(b"ob_end_flush");
    params!(b"ob_get_clean");
    params!(b"ob_get_contents");
    params!(b"ob_get_level");
    params!(b"ob_flush");

    // PCRE functions
    params!(b"preg_match", b"pattern", b"subject", b"matches", b"flags", b"offset");
    params!(b"preg_match_all", b"pattern", b"subject", b"matches", b"flags", b"offset");
    params!(b"preg_replace", b"pattern", b"replacement", b"subject", b"limit", b"count");
    params!(b"preg_split", b"pattern", b"subject", b"limit", b"flags");
    params!(b"preg_quote", b"str", b"delimiter");
    params!(b"preg_replace_callback", b"pattern", b"callback", b"subject", b"limit", b"count", b"flags");

    // JSON
    params!(b"json_encode", b"value", b"flags", b"depth");
    params!(b"json_decode", b"json", b"associative", b"depth", b"flags");
    params!(b"json_last_error");
    params!(b"json_last_error_msg");

    // Date functions
    params!(b"time");
    params!(b"microtime", b"as_float");
    params!(b"date", b"format", b"timestamp");
    params!(b"strtotime", b"datetime", b"baseTimestamp");
    params!(b"mktime", b"hour", b"minute", b"second", b"month", b"day", b"year");
    params!(b"gmdate", b"format", b"timestamp");

    // File functions
    params!(b"file_get_contents", b"filename", b"use_include_path", b"context", b"offset", b"length");
    params!(b"file_put_contents", b"filename", b"data", b"flags", b"context");
    params!(b"file_exists", b"filename");
    params!(b"realpath", b"path");
    params!(b"dirname", b"path", b"levels");
    params!(b"basename", b"path", b"suffix");
    params!(b"pathinfo", b"path", b"flags");

    // Iterator/SPL functions moved to goro-ext-spl
}
