use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::array::{ArrayKey, PhpArray};
use crate::object::{ClassEntry, PhpObject, Visibility};
use crate::opcode::{OpArray, OpCode, OperandType, ParamType};
use crate::string::PhpString;
use crate::value::Value;

/// Built-in function signature
pub type BuiltinFn = fn(&mut Vm, &[Value]) -> Result<Value, VmError>;

/// VM runtime error
#[derive(Debug, Clone)]
pub struct VmError {
    pub message: String,
    pub line: u32,
}

impl std::fmt::Display for VmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Fatal error on line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for VmError {}

/// Pending function call being assembled
struct PendingCall {
    name: PhpString,
    args: Vec<Value>,
    /// Named arguments: (param_name, value) pairs to be reordered at call time
    named_args: Vec<(Vec<u8>, Value)>,
}

impl PendingCall {
    /// Resolve named arguments by matching them to parameter positions.
    /// Named args are placed at the correct index based on cv_names.
    /// Positional args keep their order; named args fill in remaining slots.
    /// Returns an error message if there's a problem (unknown param, duplicate, etc.)
    /// variadic_param: if Some(idx), extra named args are collected into the variadic.
    fn resolve_named_args(
        &mut self,
        cv_names: &[Vec<u8>],
        implicit_args: usize,
        variadic_param: Option<u32>,
    ) -> Result<(), String> {
        if self.named_args.is_empty() {
            return Ok(());
        }

        // Determine the number of regular (non-variadic) parameters
        let regular_param_count = variadic_param
            .map(|v| v as usize)
            .unwrap_or(cv_names.len());

        // Build resolved slots for regular parameters only
        let mut resolved = vec![None; regular_param_count];

        // Place positional args into regular param slots starting from index 0
        // (this includes $this for methods - it's already at args[0])
        let positional_count = self.args.len();
        for (i, arg) in self.args.iter().enumerate() {
            if i < regular_param_count {
                resolved[i] = Some(arg.clone());
            }
        }

        // Collect extra positional args (those beyond regular params) for variadics
        let extra_positional: Vec<Value> = if positional_count > regular_param_count {
            self.args[regular_param_count..].to_vec()
        } else {
            Vec::new()
        };

        // Extra named args that don't match any parameter go here (for variadics)
        let mut extra_named: Vec<(Vec<u8>, Value)> = Vec::new();

        // Place named args by matching against cv_names
        // Skip implicit params (like $this) when matching by name
        for (name, val) in self.named_args.drain(..) {
            let mut found = false;
            for (idx, cv_name) in cv_names.iter().enumerate() {
                // Skip implicit params ($this) - they can't be set by name
                if idx < implicit_args {
                    continue;
                }
                // Skip the variadic param itself when matching by name
                if variadic_param == Some(idx as u32) {
                    continue;
                }
                if idx < regular_param_count && *cv_name == name {
                    // Check for duplicate: named arg overwrites a previously set position
                    if resolved[idx].is_some() {
                        return Err(format!(
                            "Named parameter ${} overwrites previous argument",
                            String::from_utf8_lossy(&name)
                        ));
                    }
                    resolved[idx] = Some(val.clone());
                    found = true;
                    break;
                }
            }
            if !found {
                if variadic_param.is_some() {
                    // Collect extra named args for the variadic parameter
                    extra_named.push((name, val));
                } else {
                    // Unknown named parameter - error
                    return Err(format!(
                        "Unknown named parameter ${}",
                        String::from_utf8_lossy(&name)
                    ));
                }
            }
        }

        // Rebuild self.args: resolved regular params + extra positional args
        self.args.clear();
        for slot in resolved {
            self.args.push(slot.unwrap_or(Value::Undef));
        }
        // Append extra positional args (for variadic)
        self.args.extend(extra_positional);

        // Store extra named args back into named_args for variadic collection
        self.named_args = extra_named;

        Ok(())
    }

    /// Resolve named arguments for builtin functions by matching against parameter names.
    /// Returns an error (exception_class, message) if there's a problem.
    fn resolve_named_args_builtin(
        &mut self,
        param_names: &[&[u8]],
    ) -> Result<(), String> {
        if self.named_args.is_empty() {
            return Ok(());
        }

        // Track which positions were explicitly set by positional args
        let positional_count = self.args.len();

        // Process named args
        for (name, val) in self.named_args.drain(..) {
            let mut found = false;
            for (idx, pname) in param_names.iter().enumerate() {
                if *pname == name.as_slice() {
                    // Check for duplicate: only error if a positional arg was at this position
                    if idx < positional_count {
                        return Err(format!(
                            "Named parameter ${} overwrites previous argument",
                            String::from_utf8_lossy(&name)
                        ));
                    }
                    // Extend args if needed, using Null for skipped positions
                    while self.args.len() <= idx {
                        self.args.push(Value::Null);
                    }
                    self.args[idx] = val.clone();
                    found = true;
                    break;
                }
            }
            if !found {
                return Err(format!(
                    "Unknown named parameter ${}",
                    String::from_utf8_lossy(&name)
                ));
            }
        }

        Ok(())
    }
}

/// The virtual machine / executor
pub struct Vm {
    /// Output buffer
    output: Vec<u8>,
    /// Output buffer stack for ob_start/ob_end_clean/ob_get_contents
    pub ob_stack: Vec<Vec<u8>>,
    /// Registered built-in functions
    pub functions: HashMap<Vec<u8>, BuiltinFn>,
    /// Parameter names for built-in functions (lowercase fn name -> list of param names)
    pub builtin_param_names: HashMap<Vec<u8>, Vec<Vec<u8>>>,
    /// User-defined functions (compiled op arrays)
    pub user_functions: HashMap<Vec<u8>, OpArray>,
    /// Stack of pending function calls (supports nested calls)
    pending_calls: Vec<PendingCall>,
    /// Static variable storage (keyed by "funcname::varname")
    static_vars: HashMap<Vec<u8>, Value>,
    /// Global variables
    globals: HashMap<Vec<u8>, Value>,
    /// Class table
    pub classes: HashMap<Vec<u8>, ClassEntry>,
    /// Next object ID
    next_object_id: u64,
    /// Pending class definitions (from compiler, indexed by position)
    pending_classes: Vec<ClassEntry>,
    /// Whether we're executing the top-level script (vs a function)
    is_global_scope: bool,
    /// User-defined constants (from define())
    pub constants: HashMap<Vec<u8>, Value>,
    /// Current exception being thrown (used during try/catch)
    pub current_exception: Option<Value>,
    /// Error reporting level
    pub error_reporting: i64,
    /// Recursion depth for magic methods (prevent infinite recursion)
    magic_depth: u32,
    /// Objects with __destruct methods, tracked for shutdown-time destruction
    destructible_objects: Vec<Value>,
    /// Stack of "called class" names for late static binding (static::)
    pub called_class_stack: Vec<Vec<u8>>,
    /// Stack of "defining class" names for visibility checks (lowercase)
    /// This tracks the class where the currently executing method was defined/declared.
    class_scope_stack: Vec<Vec<u8>>,
    /// Current call depth (to prevent stack overflow from infinite recursion)
    call_depth: u32,
    /// Call stack for stack trace generation: (function_name, file, line_called_from, args, is_instance_method)
    pub call_stack: Vec<(String, String, u32, Vec<Value>, bool)>,
    /// Stack of saved error_reporting levels (for @ operator)
    pub error_reporting_stack: Vec<i64>,
    /// Pending return value (for deferred return in finally blocks)
    pending_return: Option<Value>,
    /// User error handler callback (from set_error_handler)
    pub error_handler: Option<Value>,
    /// Next ID for bound closure names
    next_bound_closure_id: u64,
    /// JSON last error code (0 = no error)
    pub json_last_error: i64,
    /// Named args being forwarded to a builtin function call (for call_user_func etc.)
    pub pending_named_args: Vec<(Vec<u8>, Value)>,
    /// Current executing file path
    pub current_file: String,
    /// Line number of the last return statement (for return type error messages)
    last_return_line: u32,
    /// Current executing line number (updated during execution for warning/error reporting)
    pub current_line: u32,
}

impl Vm {
    pub fn new() -> Self {
        Self {
            output: Vec::new(),
                ob_stack: Vec::new(),
            functions: HashMap::new(),
            builtin_param_names: HashMap::new(),
            user_functions: HashMap::new(),
            pending_calls: Vec::new(),
            static_vars: HashMap::new(),
            globals: HashMap::new(),
            classes: HashMap::new(),
            next_object_id: 1,
            pending_classes: Vec::new(),
            is_global_scope: true,
            current_exception: None,
            error_reporting: 32767, // E_ALL
            magic_depth: 0,
            destructible_objects: Vec::new(),
            called_class_stack: Vec::new(),
            class_scope_stack: Vec::new(),
            call_depth: 0,
            call_stack: Vec::new(),
            error_reporting_stack: Vec::new(),
            pending_return: None,
            error_handler: None,
            next_bound_closure_id: 0,
            json_last_error: 0,
            pending_named_args: Vec::new(),
            current_file: "Unknown.php".to_string(),
            last_return_line: 0,
            current_line: 0,
            constants: {
                let mut c = HashMap::new();
                // Default ini values
                c.insert(
                    b"arg_separator.output".to_vec(),
                    Value::String(PhpString::from_bytes(b"&")),
                );
                c.insert(b"precision".to_vec(), Value::Long(14));
                c.insert(b"serialize_precision".to_vec(), Value::Long(-1));
                c.insert(b"error_reporting".to_vec(), Value::Long(32767));
                c.insert(b"display_errors".to_vec(), Value::Long(1));
                c.insert(
                    b"memory_limit".to_vec(),
                    Value::String(PhpString::from_bytes(b"128M")),
                );
                c.insert(b"max_execution_time".to_vec(), Value::Long(30));
                c.insert(
                    b"default_charset".to_vec(),
                    Value::String(PhpString::from_bytes(b"UTF-8")),
                );
                // PHP constants
                c.insert(
                    b"PHP_EOL".to_vec(),
                    Value::String(PhpString::from_bytes(b"\n")),
                );
                c.insert(b"PHP_INT_MAX".to_vec(), Value::Long(i64::MAX));
                c.insert(b"PHP_INT_MIN".to_vec(), Value::Long(i64::MIN));
                c.insert(b"PHP_INT_SIZE".to_vec(), Value::Long(8));
                c.insert(b"PHP_FLOAT_MAX".to_vec(), Value::Double(f64::MAX));
                c.insert(b"PHP_FLOAT_MIN".to_vec(), Value::Double(f64::MIN_POSITIVE));
                c.insert(b"PHP_FLOAT_EPSILON".to_vec(), Value::Double(f64::EPSILON));
                c.insert(b"PHP_FLOAT_DIG".to_vec(), Value::Long(15));
                c.insert(b"PHP_FLOAT_INF".to_vec(), Value::Double(f64::INFINITY));
                c.insert(b"PHP_FLOAT_NAN".to_vec(), Value::Double(f64::NAN));
                c.insert(b"PHP_MAXPATHLEN".to_vec(), Value::Long(4096));
                c.insert(
                    b"PHP_OS".to_vec(),
                    Value::String(PhpString::from_bytes(b"Linux")),
                );
                c.insert(
                    b"PHP_OS_FAMILY".to_vec(),
                    Value::String(PhpString::from_bytes(b"Linux")),
                );
                c.insert(
                    b"PHP_SAPI".to_vec(),
                    Value::String(PhpString::from_bytes(b"cli")),
                );
                c.insert(
                    b"PHP_VERSION".to_vec(),
                    Value::String(PhpString::from_bytes(b"8.5.4")),
                );
                c.insert(b"PHP_MAJOR_VERSION".to_vec(), Value::Long(8));
                c.insert(b"PHP_MINOR_VERSION".to_vec(), Value::Long(5));
                c.insert(b"PHP_RELEASE_VERSION".to_vec(), Value::Long(4));
                c.insert(b"PHP_VERSION_ID".to_vec(), Value::Long(80504));
                c.insert(
                    b"PHP_EXTRA_VERSION".to_vec(),
                    Value::String(PhpString::from_bytes(b"")),
                );
                c.insert(b"PHP_DEBUG".to_vec(), Value::Long(0));
                c.insert(b"PHP_ZTS".to_vec(), Value::Long(0));
                c.insert(
                    b"PHP_PREFIX".to_vec(),
                    Value::String(PhpString::from_bytes(b"/usr")),
                );
                c.insert(
                    b"PHP_BINDIR".to_vec(),
                    Value::String(PhpString::from_bytes(b"/usr/bin")),
                );
                c.insert(
                    b"PHP_LIBDIR".to_vec(),
                    Value::String(PhpString::from_bytes(b"/usr/lib")),
                );
                c.insert(
                    b"PHP_DATADIR".to_vec(),
                    Value::String(PhpString::from_bytes(b"/usr/share")),
                );
                c.insert(
                    b"PHP_SYSCONFDIR".to_vec(),
                    Value::String(PhpString::from_bytes(b"/etc")),
                );
                c.insert(
                    b"PHP_EXTENSION_DIR".to_vec(),
                    Value::String(PhpString::from_bytes(b"")),
                );
                c.insert(
                    b"PHP_BINARY".to_vec(),
                    Value::String(PhpString::from_bytes(b"goro")),
                );
                // File handles - use Long as resource placeholders (non-zero so they are truthy)
                c.insert(b"STDIN".to_vec(), Value::Long(1));
                c.insert(b"STDOUT".to_vec(), Value::Long(2));
                c.insert(b"STDERR".to_vec(), Value::Long(3));
                c.insert(
                    b"DIRECTORY_SEPARATOR".to_vec(),
                    Value::String(PhpString::from_bytes(b"/")),
                );
                c.insert(
                    b"PATH_SEPARATOR".to_vec(),
                    Value::String(PhpString::from_bytes(b":")),
                );
                // Error levels
                c.insert(b"E_ERROR".to_vec(), Value::Long(1));
                c.insert(b"E_WARNING".to_vec(), Value::Long(2));
                c.insert(b"E_PARSE".to_vec(), Value::Long(4));
                c.insert(b"E_NOTICE".to_vec(), Value::Long(8));
                c.insert(b"E_CORE_ERROR".to_vec(), Value::Long(16));
                c.insert(b"E_CORE_WARNING".to_vec(), Value::Long(32));
                c.insert(b"E_COMPILE_ERROR".to_vec(), Value::Long(64));
                c.insert(b"E_COMPILE_WARNING".to_vec(), Value::Long(128));
                c.insert(b"E_USER_ERROR".to_vec(), Value::Long(256));
                c.insert(b"E_USER_WARNING".to_vec(), Value::Long(512));
                c.insert(b"E_USER_NOTICE".to_vec(), Value::Long(1024));
                c.insert(b"E_STRICT".to_vec(), Value::Long(2048));
                c.insert(b"E_RECOVERABLE_ERROR".to_vec(), Value::Long(4096));
                c.insert(b"E_DEPRECATED".to_vec(), Value::Long(8192));
                c.insert(b"E_USER_DEPRECATED".to_vec(), Value::Long(16384));
                c.insert(b"E_ALL".to_vec(), Value::Long(32767));
                // Array/sort constants
                c.insert(b"CASE_LOWER".to_vec(), Value::Long(0));
                c.insert(b"CASE_UPPER".to_vec(), Value::Long(1));
                c.insert(b"SORT_REGULAR".to_vec(), Value::Long(0));
                c.insert(b"SORT_NUMERIC".to_vec(), Value::Long(1));
                c.insert(b"SORT_STRING".to_vec(), Value::Long(2));
                c.insert(b"SORT_ASC".to_vec(), Value::Long(4));
                c.insert(b"SORT_DESC".to_vec(), Value::Long(3));
                c.insert(b"SORT_LOCALE_STRING".to_vec(), Value::Long(5));
                c.insert(b"SORT_NATURAL".to_vec(), Value::Long(6));
                c.insert(b"SORT_FLAG_CASE".to_vec(), Value::Long(8));
                c.insert(b"ARRAY_FILTER_USE_BOTH".to_vec(), Value::Long(1));
                c.insert(b"ARRAY_FILTER_USE_KEY".to_vec(), Value::Long(2));
                c.insert(b"ARRAY_UNIQUE_REGULAR".to_vec(), Value::Long(0));
                c.insert(b"COUNT_NORMAL".to_vec(), Value::Long(0));
                c.insert(b"COUNT_RECURSIVE".to_vec(), Value::Long(1));
                // Pathinfo constants
                c.insert(b"PATHINFO_DIRNAME".to_vec(), Value::Long(1));
                c.insert(b"PATHINFO_BASENAME".to_vec(), Value::Long(2));
                c.insert(b"PATHINFO_EXTENSION".to_vec(), Value::Long(4));
                c.insert(b"PATHINFO_FILENAME".to_vec(), Value::Long(8));
                c.insert(b"PATHINFO_ALL".to_vec(), Value::Long(15));
                // File constants
                c.insert(b"FILE_USE_INCLUDE_PATH".to_vec(), Value::Long(1));
                c.insert(b"FILE_APPEND".to_vec(), Value::Long(8));
                c.insert(b"FILE_IGNORE_NEW_LINES".to_vec(), Value::Long(2));
                c.insert(b"FILE_SKIP_EMPTY_LINES".to_vec(), Value::Long(4));
                c.insert(b"LOCK_EX".to_vec(), Value::Long(2));
                c.insert(b"LOCK_SH".to_vec(), Value::Long(1));
                c.insert(b"LOCK_UN".to_vec(), Value::Long(3));
                c.insert(b"LOCK_NB".to_vec(), Value::Long(4));
                c.insert(b"SEEK_SET".to_vec(), Value::Long(0));
                c.insert(b"SEEK_CUR".to_vec(), Value::Long(1));
                c.insert(b"SEEK_END".to_vec(), Value::Long(2));
                c.insert(b"GLOB_MARK".to_vec(), Value::Long(1));
                c.insert(b"GLOB_NOSORT".to_vec(), Value::Long(2));
                c.insert(b"GLOB_NOCHECK".to_vec(), Value::Long(16));
                c.insert(b"GLOB_NOESCAPE".to_vec(), Value::Long(64));
                c.insert(b"GLOB_BRACE".to_vec(), Value::Long(128));
                c.insert(b"GLOB_ONLYDIR".to_vec(), Value::Long(1073741824));
                // PREG constants
                c.insert(b"PREG_SPLIT_NO_EMPTY".to_vec(), Value::Long(1));
                c.insert(b"PREG_SPLIT_DELIM_CAPTURE".to_vec(), Value::Long(2));
                c.insert(b"PREG_SPLIT_OFFSET_CAPTURE".to_vec(), Value::Long(4));
                c.insert(b"PREG_OFFSET_CAPTURE".to_vec(), Value::Long(256));
                c.insert(b"PREG_UNMATCHED_AS_NULL".to_vec(), Value::Long(512));
                c.insert(b"PREG_SET_ORDER".to_vec(), Value::Long(2));
                c.insert(b"PREG_PATTERN_ORDER".to_vec(), Value::Long(1));
                c.insert(b"PREG_GREP_INVERT".to_vec(), Value::Long(1));
                c.insert(b"PREG_NO_ERROR".to_vec(), Value::Long(0));
                c.insert(b"PREG_INTERNAL_ERROR".to_vec(), Value::Long(1));
                c.insert(b"PREG_BACKTRACK_LIMIT_ERROR".to_vec(), Value::Long(2));
                c.insert(b"PREG_RECURSION_LIMIT_ERROR".to_vec(), Value::Long(3));
                c.insert(b"PREG_BAD_UTF8_ERROR".to_vec(), Value::Long(4));
                c.insert(b"PREG_BAD_UTF8_OFFSET_ERROR".to_vec(), Value::Long(5));
                c.insert(b"PREG_JIT_STACKLIMIT_ERROR".to_vec(), Value::Long(6));
                c.insert(b"PCRE_JIT_SUPPORT".to_vec(), Value::False);
                c.insert(b"PCRE_VERSION".to_vec(), Value::String(PhpString::from_bytes(b"10.42 2022-12-11")));
                // Hash constants
                c.insert(b"HASH_HMAC".to_vec(), Value::Long(1));
                // Assert/INI defaults
                c.insert(b"zend.assertions".to_vec(), Value::Long(1));
                c.insert(b"assert.active".to_vec(), Value::Long(1));
                c.insert(b"assert.exception".to_vec(), Value::Long(1));
                // Assert option constants
                c.insert(b"ASSERT_ACTIVE".to_vec(), Value::Long(1));
                c.insert(b"ASSERT_WARNING".to_vec(), Value::Long(2));
                c.insert(b"ASSERT_BAIL".to_vec(), Value::Long(3));
                c.insert(b"ASSERT_QUIET_EVAL".to_vec(), Value::Long(4));
                c.insert(b"ASSERT_CALLBACK".to_vec(), Value::Long(5));
                c.insert(b"ASSERT_EXCEPTION".to_vec(), Value::Long(6));
                // String constants
                c.insert(b"STR_PAD_RIGHT".to_vec(), Value::Long(1));
                c.insert(b"STR_PAD_LEFT".to_vec(), Value::Long(0));
                c.insert(b"STR_PAD_BOTH".to_vec(), Value::Long(2));
                // Math constants
                c.insert(b"M_PI".to_vec(), Value::Double(std::f64::consts::PI));
                c.insert(b"M_E".to_vec(), Value::Double(std::f64::consts::E));
                c.insert(b"M_LOG2E".to_vec(), Value::Double(std::f64::consts::LOG2_E));
                c.insert(
                    b"M_LOG10E".to_vec(),
                    Value::Double(std::f64::consts::LOG10_E),
                );
                c.insert(b"M_LN2".to_vec(), Value::Double(std::f64::consts::LN_2));
                c.insert(b"M_LN10".to_vec(), Value::Double(std::f64::consts::LN_10));
                c.insert(
                    b"M_PI_2".to_vec(),
                    Value::Double(std::f64::consts::FRAC_PI_2),
                );
                c.insert(
                    b"M_PI_4".to_vec(),
                    Value::Double(std::f64::consts::FRAC_PI_4),
                );
                c.insert(
                    b"M_1_PI".to_vec(),
                    Value::Double(std::f64::consts::FRAC_1_PI),
                );
                c.insert(
                    b"M_2_PI".to_vec(),
                    Value::Double(std::f64::consts::FRAC_2_PI),
                );
                c.insert(
                    b"M_2_SQRTPI".to_vec(),
                    Value::Double(std::f64::consts::FRAC_2_SQRT_PI),
                );
                c.insert(b"M_SQRT2".to_vec(), Value::Double(std::f64::consts::SQRT_2));
                c.insert(b"M_SQRT3".to_vec(), Value::Double(1.7320508075688772));
                c.insert(
                    b"M_SQRT1_2".to_vec(),
                    Value::Double(std::f64::consts::FRAC_1_SQRT_2),
                );
                c.insert(b"M_EULER".to_vec(), Value::Double(0.5772156649015329));
                c.insert(b"M_SQRTPI".to_vec(), Value::Double(1.772453850905516));
                c.insert(b"M_LNPI".to_vec(), Value::Double(1.1447298858494002));
                c.insert(b"INF".to_vec(), Value::Double(f64::INFINITY));
                c.insert(b"NAN".to_vec(), Value::Double(f64::NAN));
                // JSON constants
                c.insert(b"JSON_HEX_TAG".to_vec(), Value::Long(1));
                c.insert(b"JSON_BIGINT_AS_STRING".to_vec(), Value::Long(2));
                c.insert(b"JSON_HEX_AMP".to_vec(), Value::Long(2));
                c.insert(b"JSON_HEX_APOS".to_vec(), Value::Long(4));
                c.insert(b"JSON_HEX_QUOT".to_vec(), Value::Long(8));
                c.insert(b"JSON_FORCE_OBJECT".to_vec(), Value::Long(16));
                c.insert(b"JSON_NUMERIC_CHECK".to_vec(), Value::Long(32));
                c.insert(b"JSON_UNESCAPED_SLASHES".to_vec(), Value::Long(64));
                c.insert(b"JSON_PRETTY_PRINT".to_vec(), Value::Long(128));
                c.insert(b"JSON_UNESCAPED_UNICODE".to_vec(), Value::Long(256));
                c.insert(b"JSON_PARTIAL_OUTPUT_ON_ERROR".to_vec(), Value::Long(512));
                c.insert(b"JSON_PRESERVE_ZERO_FRACTION".to_vec(), Value::Long(1024));
                c.insert(
                    b"JSON_UNESCAPED_LINE_TERMINATORS".to_vec(),
                    Value::Long(2048),
                );
                c.insert(b"JSON_INVALID_UTF8_IGNORE".to_vec(), Value::Long(1048576));
                c.insert(
                    b"JSON_INVALID_UTF8_SUBSTITUTE".to_vec(),
                    Value::Long(2097152),
                );
                c.insert(b"JSON_THROW_ON_ERROR".to_vec(), Value::Long(4194304));
                c.insert(b"JSON_OBJECT_AS_ARRAY".to_vec(), Value::Long(1));
                // JSON error constants
                c.insert(b"JSON_ERROR_NONE".to_vec(), Value::Long(0));
                c.insert(b"JSON_ERROR_DEPTH".to_vec(), Value::Long(1));
                c.insert(b"JSON_ERROR_STATE_MISMATCH".to_vec(), Value::Long(2));
                c.insert(b"JSON_ERROR_CTRL_CHAR".to_vec(), Value::Long(3));
                c.insert(b"JSON_ERROR_SYNTAX".to_vec(), Value::Long(4));
                c.insert(b"JSON_ERROR_UTF8".to_vec(), Value::Long(5));
                c.insert(b"JSON_ERROR_RECURSION".to_vec(), Value::Long(6));
                c.insert(b"JSON_ERROR_INF_OR_NAN".to_vec(), Value::Long(7));
                c.insert(b"JSON_ERROR_UNSUPPORTED_TYPE".to_vec(), Value::Long(8));
                c.insert(b"JSON_ERROR_INVALID_PROPERTY_NAME".to_vec(), Value::Long(9));
                c.insert(b"JSON_ERROR_UTF16".to_vec(), Value::Long(10));
                // URL constants
                c.insert(b"PHP_URL_SCHEME".to_vec(), Value::Long(0));
                c.insert(b"PHP_URL_HOST".to_vec(), Value::Long(1));
                c.insert(b"PHP_URL_PORT".to_vec(), Value::Long(2));
                c.insert(b"PHP_URL_USER".to_vec(), Value::Long(3));
                c.insert(b"PHP_URL_PASS".to_vec(), Value::Long(4));
                c.insert(b"PHP_URL_PATH".to_vec(), Value::Long(5));
                c.insert(b"PHP_URL_QUERY".to_vec(), Value::Long(6));
                c.insert(b"PHP_URL_FRAGMENT".to_vec(), Value::Long(7));
                // Other
                c.insert(b"SEEK_SET".to_vec(), Value::Long(0));
                c.insert(b"SEEK_CUR".to_vec(), Value::Long(1));
                c.insert(b"SEEK_END".to_vec(), Value::Long(2));
                c.insert(b"FILE_APPEND".to_vec(), Value::Long(8));
                c.insert(b"LOCK_EX".to_vec(), Value::Long(2));
                c.insert(b"PREG_SPLIT_NO_EMPTY".to_vec(), Value::Long(1));
                c.insert(b"PREG_SPLIT_DELIM_CAPTURE".to_vec(), Value::Long(2));
                c.insert(b"T_STRING".to_vec(), Value::Long(319));
                // Rounding mode constants
                c.insert(b"PHP_ROUND_HALF_UP".to_vec(), Value::Long(0));
                c.insert(b"PHP_ROUND_HALF_DOWN".to_vec(), Value::Long(1));
                c.insert(b"PHP_ROUND_HALF_EVEN".to_vec(), Value::Long(2));
                c.insert(b"PHP_ROUND_HALF_ODD".to_vec(), Value::Long(3));
                c.insert(b"PHP_ROUND_CEILING".to_vec(), Value::Long(4));
                c.insert(b"PHP_ROUND_FLOOR".to_vec(), Value::Long(5));
                c.insert(b"PHP_ROUND_TOWARD_ZERO".to_vec(), Value::Long(6));
                c.insert(b"PHP_ROUND_AWAY_FROM_ZERO".to_vec(), Value::Long(7));
                // INI constants
                c.insert(b"INI_USER".to_vec(), Value::Long(1));
                c.insert(b"INI_PERDIR".to_vec(), Value::Long(2));
                c.insert(b"INI_SYSTEM".to_vec(), Value::Long(4));
                c.insert(b"INI_ALL".to_vec(), Value::Long(7));
                c.insert(b"INI_SCANNER_NORMAL".to_vec(), Value::Long(0));
                c.insert(b"INI_SCANNER_RAW".to_vec(), Value::Long(1));
                c.insert(b"INI_SCANNER_TYPED".to_vec(), Value::Long(2));
                // Date format constants
                c.insert(b"DATE_ATOM".to_vec(), Value::String(PhpString::from_bytes(b"Y-m-d\\TH:i:sP")));
                c.insert(b"DATE_ISO8601".to_vec(), Value::String(PhpString::from_bytes(b"Y-m-d\\TH:i:sO")));
                c.insert(b"DATE_ISO8601_EXPANDED".to_vec(), Value::String(PhpString::from_bytes(b"X-m-d\\TH:i:sP")));
                c.insert(b"DATE_RFC822".to_vec(), Value::String(PhpString::from_bytes(b"D, d M y H:i:s O")));
                c.insert(b"DATE_RFC850".to_vec(), Value::String(PhpString::from_bytes(b"l, d-M-y H:i:s T")));
                c.insert(b"DATE_RFC1036".to_vec(), Value::String(PhpString::from_bytes(b"D, d M y H:i:s O")));
                c.insert(b"DATE_RFC1123".to_vec(), Value::String(PhpString::from_bytes(b"D, d M Y H:i:s O")));
                c.insert(b"DATE_RFC2822".to_vec(), Value::String(PhpString::from_bytes(b"D, d M Y H:i:s O")));
                c.insert(b"DATE_RFC3339".to_vec(), Value::String(PhpString::from_bytes(b"Y-m-d\\TH:i:sP")));
                c.insert(b"DATE_RFC3339_EXTENDED".to_vec(), Value::String(PhpString::from_bytes(b"Y-m-d\\TH:i:s.vP")));
                c.insert(b"DATE_RFC7231".to_vec(), Value::String(PhpString::from_bytes(b"D, d M Y H:i:s \\G\\M\\T")));
                c.insert(b"DATE_RSS".to_vec(), Value::String(PhpString::from_bytes(b"D, d M Y H:i:s O")));
                c.insert(b"DATE_W3C".to_vec(), Value::String(PhpString::from_bytes(b"Y-m-d\\TH:i:sP")));
                c.insert(b"DATE_COOKIE".to_vec(), Value::String(PhpString::from_bytes(b"l, d-M-Y H:i:s T")));
                // HTML entity constants
                c.insert(b"ENT_COMPAT".to_vec(), Value::Long(2));
                c.insert(b"ENT_QUOTES".to_vec(), Value::Long(3));
                c.insert(b"ENT_NOQUOTES".to_vec(), Value::Long(0));
                c.insert(b"ENT_HTML401".to_vec(), Value::Long(0));
                c.insert(b"ENT_XML1".to_vec(), Value::Long(16));
                c.insert(b"ENT_XHTML".to_vec(), Value::Long(32));
                c.insert(b"ENT_HTML5".to_vec(), Value::Long(48));
                c.insert(b"ENT_SUBSTITUTE".to_vec(), Value::Long(8));
                c.insert(b"ENT_DISALLOWED".to_vec(), Value::Long(128));
                c.insert(b"HTML_SPECIALCHARS".to_vec(), Value::Long(0));
                c.insert(b"HTML_ENTITIES".to_vec(), Value::Long(1));
                // Misc PHP constants
                c.insert(b"PHP_OUTPUT_HANDLER_START".to_vec(), Value::Long(1));
                c.insert(b"PHP_OUTPUT_HANDLER_WRITE".to_vec(), Value::Long(0));
                c.insert(b"PHP_OUTPUT_HANDLER_FLUSH".to_vec(), Value::Long(4));
                c.insert(b"PHP_OUTPUT_HANDLER_CLEAN".to_vec(), Value::Long(2));
                c.insert(b"PHP_OUTPUT_HANDLER_FINAL".to_vec(), Value::Long(8));
                c.insert(b"PHP_OUTPUT_HANDLER_CONT".to_vec(), Value::Long(0));
                c.insert(b"PHP_OUTPUT_HANDLER_END".to_vec(), Value::Long(8));
                c.insert(b"UPLOAD_ERR_OK".to_vec(), Value::Long(0));
                c.insert(b"UPLOAD_ERR_INI_SIZE".to_vec(), Value::Long(1));
                c.insert(b"UPLOAD_ERR_FORM_SIZE".to_vec(), Value::Long(2));
                c.insert(b"UPLOAD_ERR_PARTIAL".to_vec(), Value::Long(3));
                c.insert(b"UPLOAD_ERR_NO_FILE".to_vec(), Value::Long(4));
                c.insert(b"UPLOAD_ERR_NO_TMP_DIR".to_vec(), Value::Long(6));
                c.insert(b"UPLOAD_ERR_CANT_WRITE".to_vec(), Value::Long(7));
                c.insert(b"UPLOAD_ERR_EXTENSION".to_vec(), Value::Long(8));
                // Ctype constants
                c.insert(b"CTYPE_ALPHA".to_vec(), Value::Long(1));
                // Additional misc constants
                c.insert(b"ARRAY_UNIQUE_REGULAR".to_vec(), Value::Long(0));
                c.insert(b"LC_ALL".to_vec(), Value::Long(6));
                c.insert(b"LC_COLLATE".to_vec(), Value::Long(3));
                c.insert(b"LC_CTYPE".to_vec(), Value::Long(0));
                c.insert(b"LC_MONETARY".to_vec(), Value::Long(4));
                c.insert(b"LC_NUMERIC".to_vec(), Value::Long(1));
                c.insert(b"LC_TIME".to_vec(), Value::Long(2));
                c.insert(b"LC_MESSAGES".to_vec(), Value::Long(5));
                // EXTR_ constants
                c.insert(b"EXTR_OVERWRITE".to_vec(), Value::Long(0));
                c.insert(b"EXTR_SKIP".to_vec(), Value::Long(1));
                c.insert(b"EXTR_PREFIX_SAME".to_vec(), Value::Long(2));
                c.insert(b"EXTR_PREFIX_ALL".to_vec(), Value::Long(3));
                c.insert(b"EXTR_PREFIX_INVALID".to_vec(), Value::Long(4));
                c.insert(b"EXTR_IF_EXISTS".to_vec(), Value::Long(6));
                c.insert(b"EXTR_PREFIX_IF_EXISTS".to_vec(), Value::Long(7));
                c.insert(b"EXTR_REFS".to_vec(), Value::Long(256));
                // String comparison constants
                c.insert(b"CHAR_MAX".to_vec(), Value::Long(127));
                // PHP int constants for sizes
                c.insert(b"PHP_INT_MAX".to_vec(), Value::Long(i64::MAX));
                c.insert(b"PHP_INT_MIN".to_vec(), Value::Long(i64::MIN));
                // FILTER constants
                c.insert(b"FILTER_DEFAULT".to_vec(), Value::Long(516));
                c.insert(b"FILTER_VALIDATE_INT".to_vec(), Value::Long(257));
                c.insert(b"FILTER_VALIDATE_FLOAT".to_vec(), Value::Long(259));
                c.insert(b"FILTER_VALIDATE_BOOLEAN".to_vec(), Value::Long(258));
                c.insert(b"FILTER_VALIDATE_EMAIL".to_vec(), Value::Long(274));
                c.insert(b"FILTER_VALIDATE_URL".to_vec(), Value::Long(273));
                c.insert(b"FILTER_VALIDATE_IP".to_vec(), Value::Long(275));
                c.insert(b"FILTER_SANITIZE_STRING".to_vec(), Value::Long(513));
                c.insert(b"FILTER_SANITIZE_EMAIL".to_vec(), Value::Long(517));
                c.insert(b"FILTER_SANITIZE_URL".to_vec(), Value::Long(518));
                c.insert(b"FILTER_SANITIZE_NUMBER_INT".to_vec(), Value::Long(519));
                c.insert(b"FILTER_SANITIZE_NUMBER_FLOAT".to_vec(), Value::Long(520));
                c.insert(b"FILTER_SANITIZE_SPECIAL_CHARS".to_vec(), Value::Long(515));
                c.insert(b"FILTER_SANITIZE_ENCODED".to_vec(), Value::Long(514));
                c.insert(b"FILTER_SANITIZE_ADD_SLASHES".to_vec(), Value::Long(523));
                c.insert(b"FILTER_CALLBACK".to_vec(), Value::Long(1024));
                // DNS constants
                c.insert(b"DNS_A".to_vec(), Value::Long(1));
                c.insert(b"DNS_NS".to_vec(), Value::Long(2));
                c.insert(b"DNS_CNAME".to_vec(), Value::Long(16));
                c.insert(b"DNS_SOA".to_vec(), Value::Long(32));
                c.insert(b"DNS_MX".to_vec(), Value::Long(16384));
                c.insert(b"DNS_TXT".to_vec(), Value::Long(32768));
                c.insert(b"DNS_AAAA".to_vec(), Value::Long(134217728));
                c.insert(b"DNS_ALL".to_vec(), Value::Long(251713587));
                c.insert(b"DNS_ANY".to_vec(), Value::Long(268435456));
                // STR constants
                c.insert(b"CRYPT_BLOWFISH".to_vec(), Value::Long(1));
                c.insert(b"CRYPT_MD5".to_vec(), Value::Long(1));
                c.insert(b"CRYPT_SHA256".to_vec(), Value::Long(1));
                c.insert(b"CRYPT_SHA512".to_vec(), Value::Long(1));
                c.insert(b"CRYPT_SALT_LENGTH".to_vec(), Value::Long(123));
                // Miscellaneous
                c.insert(b"CONNECTION_NORMAL".to_vec(), Value::Long(0));
                c.insert(b"CONNECTION_ABORTED".to_vec(), Value::Long(1));
                c.insert(b"CONNECTION_TIMEOUT".to_vec(), Value::Long(2));
                c.insert(b"CREDITS_GROUP".to_vec(), Value::Long(1));
                c.insert(b"CREDITS_GENERAL".to_vec(), Value::Long(2));
                c.insert(b"CREDITS_SAPI".to_vec(), Value::Long(4));
                c.insert(b"CREDITS_MODULES".to_vec(), Value::Long(8));
                c.insert(b"CREDITS_DOCS".to_vec(), Value::Long(16));
                c.insert(b"CREDITS_FULLPAGE".to_vec(), Value::Long(32));
                c.insert(b"CREDITS_QA".to_vec(), Value::Long(64));
                c.insert(b"CREDITS_ALL".to_vec(), Value::Long(0xFFFF));
                c.insert(b"INFO_GENERAL".to_vec(), Value::Long(1));
                c.insert(b"INFO_CREDITS".to_vec(), Value::Long(2));
                c.insert(b"INFO_CONFIGURATION".to_vec(), Value::Long(4));
                c.insert(b"INFO_MODULES".to_vec(), Value::Long(8));
                c.insert(b"INFO_ENVIRONMENT".to_vec(), Value::Long(16));
                c.insert(b"INFO_VARIABLES".to_vec(), Value::Long(32));
                c.insert(b"INFO_LICENSE".to_vec(), Value::Long(64));
                c.insert(b"INFO_ALL".to_vec(), Value::Long(0xFFFF));
                // SCANDIR constants
                c.insert(b"SCANDIR_SORT_ASCENDING".to_vec(), Value::Long(0));
                c.insert(b"SCANDIR_SORT_DESCENDING".to_vec(), Value::Long(1));
                c.insert(b"SCANDIR_SORT_NONE".to_vec(), Value::Long(2));
                // PASSWORD constants
                c.insert(b"PASSWORD_DEFAULT".to_vec(), Value::String(PhpString::from_bytes(b"2y")));
                c.insert(b"PASSWORD_BCRYPT".to_vec(), Value::String(PhpString::from_bytes(b"2y")));
                c.insert(b"PASSWORD_BCRYPT_DEFAULT_COST".to_vec(), Value::Long(10));
                // GLOB constants
                c.insert(b"GLOB_ERR".to_vec(), Value::Long(4));
                c.insert(b"GLOB_AVAILABLE_FLAGS".to_vec(), Value::Long(1073741951));
                // CRYPT constants
                c.insert(b"CRYPT_STD_DES".to_vec(), Value::Long(1));
                c.insert(b"CRYPT_EXT_DES".to_vec(), Value::Long(1));
                // Stream constants
                c.insert(b"STREAM_FILTER_READ".to_vec(), Value::Long(1));
                c.insert(b"STREAM_FILTER_WRITE".to_vec(), Value::Long(2));
                c.insert(b"STREAM_FILTER_ALL".to_vec(), Value::Long(3));
                // ENT_IGNORE
                c.insert(b"ENT_IGNORE".to_vec(), Value::Long(4));
                // File mode constants
                c.insert(b"FILE_BINARY".to_vec(), Value::Long(0));
                c.insert(b"FILE_TEXT".to_vec(), Value::Long(0));
                // Sun functions constants
                c.insert(b"SUNFUNCS_RET_TIMESTAMP".to_vec(), Value::Long(0));
                c.insert(b"SUNFUNCS_RET_STRING".to_vec(), Value::Long(1));
                c.insert(b"SUNFUNCS_RET_DOUBLE".to_vec(), Value::Long(2));
                // Debug backtrace
                c.insert(b"DEBUG_BACKTRACE_PROVIDE_OBJECT".to_vec(), Value::Long(1));
                c.insert(b"DEBUG_BACKTRACE_IGNORE_ARGS".to_vec(), Value::Long(2));
                // PHP_QUERY constants
                c.insert(b"PHP_QUERY_RFC1738".to_vec(), Value::Long(1));
                c.insert(b"PHP_QUERY_RFC3986".to_vec(), Value::Long(2));
                // MB constants
                c.insert(b"MB_CASE_UPPER".to_vec(), Value::Long(0));
                c.insert(b"MB_CASE_LOWER".to_vec(), Value::Long(1));
                c.insert(b"MB_CASE_TITLE".to_vec(), Value::Long(2));
                c.insert(b"MB_CASE_FOLD".to_vec(), Value::Long(0));
                c.insert(b"MB_CASE_UPPER_SIMPLE".to_vec(), Value::Long(3));
                c.insert(b"MB_CASE_LOWER_SIMPLE".to_vec(), Value::Long(4));
                c.insert(b"MB_CASE_FOLD_SIMPLE".to_vec(), Value::Long(5));
                // LOG constants
                c.insert(b"LOG_EMERG".to_vec(), Value::Long(0));
                c.insert(b"LOG_ALERT".to_vec(), Value::Long(1));
                c.insert(b"LOG_CRIT".to_vec(), Value::Long(2));
                c.insert(b"LOG_ERR".to_vec(), Value::Long(3));
                c.insert(b"LOG_WARNING".to_vec(), Value::Long(4));
                c.insert(b"LOG_NOTICE".to_vec(), Value::Long(5));
                c.insert(b"LOG_INFO".to_vec(), Value::Long(6));
                c.insert(b"LOG_DEBUG".to_vec(), Value::Long(7));
                c.insert(b"LOG_KERN".to_vec(), Value::Long(0));
                c.insert(b"LOG_USER".to_vec(), Value::Long(8));
                c.insert(b"LOG_LOCAL0".to_vec(), Value::Long(128));
                c.insert(b"LOG_PID".to_vec(), Value::Long(1));
                c.insert(b"LOG_CONS".to_vec(), Value::Long(2));
                c.insert(b"LOG_NDELAY".to_vec(), Value::Long(8));
                c.insert(b"LOG_ODELAY".to_vec(), Value::Long(4));
                c.insert(b"LOG_PERROR".to_vec(), Value::Long(32));
                // FILTER_VALIDATE_REGEXP
                c.insert(b"FILTER_VALIDATE_REGEXP".to_vec(), Value::Long(272));
                // GMP constants
                c.insert(b"GMP_BIG_ENDIAN".to_vec(), Value::Long(2));
                c.insert(b"GMP_LITTLE_ENDIAN".to_vec(), Value::Long(4));
                c.insert(b"GMP_NATIVE_ENDIAN".to_vec(), Value::Long(16));
                c.insert(b"GMP_MSW_FIRST".to_vec(), Value::Long(1));
                c.insert(b"GMP_LSW_FIRST".to_vec(), Value::Long(8));
                c.insert(b"GMP_ROUND_ZERO".to_vec(), Value::Long(0));
                c.insert(b"GMP_ROUND_PLUSINF".to_vec(), Value::Long(1));
                c.insert(b"GMP_ROUND_MINUSINF".to_vec(), Value::Long(2));
                c.insert(b"GMP_VERSION".to_vec(), Value::String(PhpString::from_bytes(b"6.2.1")));
                // Array constants
                c.insert(b"ARRAY_FILTER_USE_VALUE".to_vec(), Value::Long(0));
                // Stream constants
                c.insert(b"STREAM_SERVER_BIND".to_vec(), Value::Long(4));
                c.insert(b"STREAM_SERVER_LISTEN".to_vec(), Value::Long(8));
                c.insert(b"STREAM_CLIENT_CONNECT".to_vec(), Value::Long(4));
                c.insert(b"STREAM_CLIENT_ASYNC_CONNECT".to_vec(), Value::Long(2));
                c.insert(b"STREAM_CLIENT_PERSISTENT".to_vec(), Value::Long(1));
                c.insert(b"STREAM_NOTIFY_CONNECT".to_vec(), Value::Long(2));
                c.insert(b"STREAM_NOTIFY_AUTH_REQUIRED".to_vec(), Value::Long(3));
                c.insert(b"STREAM_IS_URL".to_vec(), Value::Long(1));
                c.insert(b"STREAM_URL_STAT_LINK".to_vec(), Value::Long(1));
                c.insert(b"STREAM_URL_STAT_QUIET".to_vec(), Value::Long(2));
                c.insert(b"STREAM_MKDIR_RECURSIVE".to_vec(), Value::Long(1));
                c.insert(b"STREAM_META_TOUCH".to_vec(), Value::Long(1));
                c.insert(b"STREAM_META_OWNER".to_vec(), Value::Long(2));
                c.insert(b"STREAM_META_OWNER_NAME".to_vec(), Value::Long(3));
                c.insert(b"STREAM_META_GROUP".to_vec(), Value::Long(4));
                c.insert(b"STREAM_META_GROUP_NAME".to_vec(), Value::Long(5));
                c.insert(b"STREAM_META_ACCESS".to_vec(), Value::Long(6));
                c.insert(b"STREAM_BUFFER_NONE".to_vec(), Value::Long(0));
                c.insert(b"STREAM_BUFFER_LINE".to_vec(), Value::Long(1));
                c.insert(b"STREAM_BUFFER_FULL".to_vec(), Value::Long(2));
                c.insert(b"STREAM_CAST_FOR_SELECT".to_vec(), Value::Long(3));
                c.insert(b"STREAM_CAST_AS_STREAM".to_vec(), Value::Long(0));
                c.insert(b"STREAM_OPTION_BLOCKING".to_vec(), Value::Long(1));
                c.insert(b"STREAM_OPTION_READ_TIMEOUT".to_vec(), Value::Long(4));
                c.insert(b"STREAM_OPTION_READ_BUFFER".to_vec(), Value::Long(2));
                c.insert(b"STREAM_OPTION_WRITE_BUFFER".to_vec(), Value::Long(3));
                c.insert(b"STREAM_SHUT_RD".to_vec(), Value::Long(0));
                c.insert(b"STREAM_SHUT_WR".to_vec(), Value::Long(1));
                c.insert(b"STREAM_SHUT_RDWR".to_vec(), Value::Long(2));
                c.insert(b"STREAM_PF_INET".to_vec(), Value::Long(2));
                c.insert(b"STREAM_PF_UNIX".to_vec(), Value::Long(1));
                c.insert(b"STREAM_IPPROTO_TCP".to_vec(), Value::Long(6));
                c.insert(b"STREAM_IPPROTO_UDP".to_vec(), Value::Long(17));
                c.insert(b"STREAM_IPPROTO_ICMP".to_vec(), Value::Long(1));
                c.insert(b"STREAM_SOCK_STREAM".to_vec(), Value::Long(1));
                c.insert(b"STREAM_SOCK_DGRAM".to_vec(), Value::Long(2));
                c.insert(b"STREAM_SOCK_RAW".to_vec(), Value::Long(3));
                c.insert(b"STREAM_PEEK".to_vec(), Value::Long(2));
                c.insert(b"STREAM_OOB".to_vec(), Value::Long(1));
                c.insert(b"STREAM_USE_PATH".to_vec(), Value::Long(1));
                c.insert(b"STREAM_REPORT_ERRORS".to_vec(), Value::Long(8));
                c.insert(b"STREAM_CRYPTO_METHOD_SSLv23_CLIENT".to_vec(), Value::Long(57));
                c.insert(b"STREAM_CRYPTO_METHOD_TLS_CLIENT".to_vec(), Value::Long(57));
                c.insert(b"STREAM_CRYPTO_METHOD_ANY_CLIENT".to_vec(), Value::Long(63));
                // Image type constants
                c.insert(b"IMAGETYPE_GIF".to_vec(), Value::Long(1));
                c.insert(b"IMAGETYPE_JPEG".to_vec(), Value::Long(2));
                c.insert(b"IMAGETYPE_PNG".to_vec(), Value::Long(3));
                c.insert(b"IMAGETYPE_SWF".to_vec(), Value::Long(4));
                c.insert(b"IMAGETYPE_PSD".to_vec(), Value::Long(5));
                c.insert(b"IMAGETYPE_BMP".to_vec(), Value::Long(6));
                c.insert(b"IMAGETYPE_TIFF_II".to_vec(), Value::Long(7));
                c.insert(b"IMAGETYPE_TIFF_MM".to_vec(), Value::Long(8));
                c.insert(b"IMAGETYPE_JPC".to_vec(), Value::Long(9));
                c.insert(b"IMAGETYPE_JP2".to_vec(), Value::Long(10));
                c.insert(b"IMAGETYPE_JPX".to_vec(), Value::Long(11));
                c.insert(b"IMAGETYPE_JB2".to_vec(), Value::Long(12));
                c.insert(b"IMAGETYPE_SWC".to_vec(), Value::Long(13));
                c.insert(b"IMAGETYPE_IFF".to_vec(), Value::Long(14));
                c.insert(b"IMAGETYPE_WBMP".to_vec(), Value::Long(15));
                c.insert(b"IMAGETYPE_XBM".to_vec(), Value::Long(16));
                c.insert(b"IMAGETYPE_ICO".to_vec(), Value::Long(17));
                c.insert(b"IMAGETYPE_WEBP".to_vec(), Value::Long(18));
                c.insert(b"IMAGETYPE_AVIF".to_vec(), Value::Long(19));
                c.insert(b"IMAGETYPE_UNKNOWN".to_vec(), Value::Long(0));
                c.insert(b"IMAGETYPE_COUNT".to_vec(), Value::Long(20));
                // Token constants
                c.insert(b"T_LNUMBER".to_vec(), Value::Long(260));
                c.insert(b"T_DNUMBER".to_vec(), Value::Long(261));
                c.insert(b"T_CONSTANT_ENCAPSED_STRING".to_vec(), Value::Long(318));
                c.insert(b"T_ENCAPSED_AND_WHITESPACE".to_vec(), Value::Long(322));
                c.insert(b"T_VARIABLE".to_vec(), Value::Long(320));
                c.insert(b"T_OPEN_TAG".to_vec(), Value::Long(392));
                c.insert(b"T_CLOSE_TAG".to_vec(), Value::Long(393));
                c.insert(b"T_INLINE_HTML".to_vec(), Value::Long(321));
                c.insert(b"T_WHITESPACE".to_vec(), Value::Long(394));
                c.insert(b"T_COMMENT".to_vec(), Value::Long(395));
                c.insert(b"T_DOC_COMMENT".to_vec(), Value::Long(396));
                // Boolean constants
                c.insert(b"TRUE".to_vec(), Value::True);
                c.insert(b"FALSE".to_vec(), Value::False);
                c.insert(b"NULL".to_vec(), Value::Null);
                // Additional type-checking constants
                c.insert(b"STDIN".to_vec(), Value::Long(1));
                c.insert(b"STDOUT".to_vec(), Value::Long(2));
                c.insert(b"STDERR".to_vec(), Value::Long(3));
                c


            },
        }
    }

    /// Resolve "static" to the actual called class name (for late static binding).
    /// Returns the class name from the called_class_stack, or the original name if not "static".
    fn resolve_static_class<'a>(&'a self, class_name: &'a [u8]) -> &'a [u8] {
        if class_name.eq_ignore_ascii_case(b"static") {
            if let Some(called) = self.called_class_stack.last() {
                called.as_slice()
            } else {
                class_name
            }
        } else {
            class_name
        }
    }

    /// Get the current calling class scope, if any.
    /// This is the class where the currently executing method was defined.
    /// Used for visibility checks. Falls back to called_class_stack if class_scope_stack is empty.
    /// Returns the canonical (original case) class name by looking up the class table.
    fn current_class_scope(&self) -> Option<Vec<u8>> {
        let scope_lower = self.class_scope_stack.last().cloned()
            .or_else(|| self.called_class_stack.last().map(|n| n.iter().map(|b| b.to_ascii_lowercase()).collect()));
        scope_lower.map(|lower| {
            // Look up canonical class name from class table
            self.classes.get(&lower)
                .map(|c| c.name.clone())
                .unwrap_or(lower)
        })
    }

    /// Check if `caller_class` (lowercase) has access to a member with the given visibility
    /// declared in `declaring_class` (lowercase), where the member belongs to `target_class` (lowercase).
    /// Returns None if access is allowed, or Some(error_message) if denied.
    fn check_visibility(
        &self,
        visibility: Visibility,
        declaring_class: &[u8],
        target_class_display: &[u8],
        member_name: &str,
        is_property: bool,
        caller_scope: Option<&[u8]>,
    ) -> Option<String> {
        match visibility {
            Visibility::Public => None,
            Visibility::Protected => {
                if let Some(caller) = caller_scope {
                    let caller_lower: Vec<u8> = caller.iter().map(|b| b.to_ascii_lowercase()).collect();
                    let declaring_lower: Vec<u8> = declaring_class.iter().map(|b| b.to_ascii_lowercase()).collect();
                    // For protected: find the root declaring class (highest ancestor that declares this member)
                    let member_name_lower: Vec<u8> = member_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
                    let root_declaring = if is_property {
                        self.find_root_declaring_class_for_property(&declaring_lower, member_name.as_bytes())
                    } else {
                        self.find_root_declaring_class_for_method(&declaring_lower, &member_name_lower)
                    };
                    // Same class, or caller extends root declaring, or root declaring extends caller
                    if caller_lower == root_declaring
                        || self.class_extends(&caller_lower, &root_declaring)
                        || self.class_extends(&root_declaring, &caller_lower)
                    {
                        None
                    } else {
                        Some(Self::format_access_error("protected", target_class_display, member_name, is_property, Some(caller)))
                    }
                } else {
                    Some(Self::format_access_error("protected", target_class_display, member_name, is_property, None))
                }
            }
            Visibility::Private => {
                if let Some(caller) = caller_scope {
                    let caller_lower: Vec<u8> = caller.iter().map(|b| b.to_ascii_lowercase()).collect();
                    let declaring_lower: Vec<u8> = declaring_class.iter().map(|b| b.to_ascii_lowercase()).collect();
                    if caller_lower == declaring_lower {
                        None
                    } else {
                        Some(Self::format_access_error("private", target_class_display, member_name, is_property, Some(caller)))
                    }
                } else {
                    Some(Self::format_access_error("private", target_class_display, member_name, is_property, None))
                }
            }
        }
    }

    fn format_access_error(vis_str: &str, target_class_display: &[u8], member_name: &str, is_property: bool, caller: Option<&[u8]>) -> String {
        let target_display = String::from_utf8_lossy(target_class_display).to_string();
        if is_property {
            format!("Cannot access {} property {}::${}", vis_str, target_display, member_name)
        } else {
            // For constructors, PHP omits "method "; for other methods, it includes "method "
            let is_constructor = member_name.eq_ignore_ascii_case("__construct");
            let method_word = if is_constructor { "" } else { "method " };
            let scope = if let Some(caller_bytes) = caller {
                format!("scope {}", String::from_utf8_lossy(caller_bytes))
            } else {
                "global scope".to_string()
            };
            format!("Call to {} {}{}::{}() from {}", vis_str, method_word, target_display, member_name, scope)
        }
    }

    /// Find the root declaring class for a method (the highest ancestor that declares it).
    /// This is needed for protected access checks: if a method is declared protected in class A,
    /// and overridden in B extends A, then any class that extends A can still access B::method().
    fn find_root_declaring_class_for_method(&self, class_name_lower: &[u8], method_name_lower: &[u8]) -> Vec<u8> {
        let mut root = class_name_lower.to_vec();
        let mut current = class_name_lower.to_vec();
        for _ in 0..50 {
            if let Some(class) = self.classes.get(&current) {
                if let Some(parent) = &class.parent {
                    let parent_lower: Vec<u8> = parent.iter().map(|b| b.to_ascii_lowercase()).collect();
                    if let Some(parent_class) = self.classes.get(&parent_lower) {
                        if let Some(parent_method) = parent_class.get_method(method_name_lower) {
                            // Only walk up if the parent method is not private
                            // (private methods are not inherited for visibility purposes)
                            if parent_method.visibility != Visibility::Private {
                                root = parent_lower.clone();
                                current = parent_lower;
                                continue;
                            }
                        }
                    }
                }
            }
            break;
        }
        root
    }

    /// Find the root declaring class for a property (the highest ancestor that declares it).
    fn find_root_declaring_class_for_property(&self, class_name_lower: &[u8], prop_name: &[u8]) -> Vec<u8> {
        let mut root = class_name_lower.to_vec();
        let mut current = class_name_lower.to_vec();
        for _ in 0..50 {
            if let Some(class) = self.classes.get(&current) {
                if let Some(parent) = &class.parent {
                    let parent_lower: Vec<u8> = parent.iter().map(|b| b.to_ascii_lowercase()).collect();
                    if let Some(parent_class) = self.classes.get(&parent_lower) {
                        if let Some(parent_prop) = parent_class.properties.iter().find(|p| p.name == prop_name) {
                            // Only walk up if the parent property is not private
                            if parent_prop.visibility != Visibility::Private {
                                root = parent_lower.clone();
                                current = parent_lower;
                                continue;
                            }
                        }
                    }
                }
            }
            break;
        }
        root
    }

    /// Find the PropertyDef for a given property name in a class (by lowercase class name).
    /// Walks up the parent chain to find the property definition.
    /// Returns (visibility, declaring_class, is_readonly).
    fn find_property_def(&self, class_name_lower: &[u8], prop_name: &[u8]) -> Option<(Visibility, Vec<u8>, bool)> {
        let mut current = class_name_lower.to_vec();
        for _ in 0..50 {
            if let Some(class) = self.classes.get(&current) {
                for prop in &class.properties {
                    if prop.name == prop_name {
                        return Some((prop.visibility, prop.declaring_class.clone(), prop.is_readonly));
                    }
                }
                if let Some(parent) = &class.parent {
                    current = parent.iter().map(|b| b.to_ascii_lowercase()).collect();
                } else {
                    return None;
                }
            } else {
                return None;
            }
        }
        None
    }

    /// Emit a PHP warning message to output
    /// Call the user error handler if set. Returns true if the handler was called and handled the error.
    pub fn call_user_error_handler(&mut self, errno: i64, errstr: &str, line: u32) -> bool {
        if let Some(handler) = self.error_handler.clone() {
            let errno_val = Value::Long(errno);
            let errstr_val = Value::String(PhpString::from_string(errstr.to_string()));
            let file_val = Value::String(PhpString::from_string(self.current_file.clone()));
            let line_val = Value::Long(line as i64);
            let args = vec![errno_val, errstr_val, file_val, line_val];
            match &handler {
                Value::String(s) => {
                    let func_lower: Vec<u8> = s.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
                    if let Some(user_fn) = self.user_functions.get(&func_lower).cloned() {
                        let mut cvs = vec![Value::Undef; user_fn.cv_names.len()];
                        for (i, arg) in args.iter().enumerate() {
                            if i < cvs.len() {
                                cvs[i] = arg.clone();
                            }
                        }
                        let _ = self.execute_op_array(&user_fn, cvs);
                        return true;
                    } else if let Some(builtin) = self.functions.get(&func_lower).copied() {
                        let _ = builtin(self, &args);
                        return true;
                    }
                }
                Value::Array(arr) => {
                    let arr_b = arr.borrow();
                    let vals: Vec<Value> = arr_b.values().cloned().collect();
                    drop(arr_b);
                    if vals.len() >= 2 {
                        let method_name = vals[1].to_php_string();
                        let method_lower: Vec<u8> = method_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
                        if let Value::Object(obj) = &vals[0] {
                            let class_name_lower: Vec<u8> = obj.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                            if let Some(class) = self.classes.get(&class_name_lower).cloned() {
                                if let Some(method) = class.get_method(&method_lower) {
                                    let op = method.op_array.clone();
                                    let mut cvs = vec![Value::Undef; op.cv_names.len()];
                                    // $this is CV 0 for instance methods
                                    cvs[0] = Value::Object(obj.clone());
                                    for (i, arg) in args.iter().enumerate() {
                                        if i + 1 < cvs.len() {
                                            cvs[i + 1] = arg.clone();
                                        }
                                    }
                                    let _ = self.execute_op_array(&op, cvs);
                                    return true;
                                }
                            }
                        } else if let Value::String(class_name) = &vals[0] {
                            let class_lower: Vec<u8> = class_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
                            if let Some(class) = self.classes.get(&class_lower).cloned() {
                                if let Some(method) = class.get_method(&method_lower) {
                                    let op = method.op_array.clone();
                                    let mut cvs = vec![Value::Undef; op.cv_names.len()];
                                    for (i, arg) in args.iter().enumerate() {
                                        if i < cvs.len() {
                                            cvs[i] = arg.clone();
                                        }
                                    }
                                    let _ = self.execute_op_array(&op, cvs);
                                    return true;
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        false
    }

    pub fn emit_warning(&mut self, msg: &str) {
        let line = self.current_line;
        self.emit_warning_at(msg, line);
    }

    /// Emit a raw warning (no user error handler, no line tracking)
    pub fn emit_warning_raw(&mut self, msg: &str) {
        if self.error_reporting & 2 != 0 {
            let warning = format!("\nWarning: {} in {} on line {}\n", msg, self.current_file, self.current_line);
            self.output.extend_from_slice(warning.as_bytes());
        }
    }

    /// Emit a PHP warning with line number
    pub fn emit_warning_at(&mut self, msg: &str, line: u32) {
        if self.error_reporting & 2 != 0 {
            if self.call_user_error_handler(2, msg, line) {
                return;
            }
            let warning = format!("\nWarning: {} in {} on line {}\n", msg, self.current_file, line);
            self.output.extend_from_slice(warning.as_bytes());
        }
    }

    /// Emit a PHP notice
    pub fn emit_notice_at(&mut self, msg: &str, line: u32) {
        if self.error_reporting & 8 != 0 {
            // E_NOTICE = 8
            if self.call_user_error_handler(8, msg, line) {
                return;
            }
            self.emit_notice_raw(msg, line);
        }
    }

    /// Emit a raw notice (no user error handler)
    pub fn emit_notice_raw(&mut self, msg: &str, line: u32) {
        if self.error_reporting & 8 != 0 {
            let notice = format!("\nNotice: {} in {} on line {}\n", msg, self.current_file, line);
            self.output.extend_from_slice(notice.as_bytes());
        }
    }

    /// Emit a PHP deprecated warning
    pub fn emit_deprecated_at(&mut self, msg: &str, line: u32) {
        if self.error_reporting & 8192 != 0 {
            // E_DEPRECATED = 8192
            if self.call_user_error_handler(8192, msg, line) {
                return;
            }
            self.emit_deprecated_raw(msg, line);
        }
    }

    /// Emit a raw deprecated warning (no user error handler)
    pub fn emit_deprecated_raw(&mut self, msg: &str, line: u32) {
        if self.error_reporting & 8192 != 0 {
            let deprec = format!("\nDeprecated: {} in {} on line {}\n", msg, self.current_file, line);
            self.output.extend_from_slice(deprec.as_bytes());
        }
    }

    /// Return a type name string for error messages.
    /// For objects, this returns the class name (e.g. "stdClass") instead of just "object".
    /// For generators, this returns "Generator".
    pub fn value_type_name(val: &Value) -> String {
        match val {
            Value::Null | Value::Undef => "null".to_string(),
            Value::True => "true".to_string(),
            Value::False => "false".to_string(),
            Value::Long(_) => "int".to_string(),
            Value::Double(_) => "float".to_string(),
            Value::String(_) => "string".to_string(),
            Value::Array(_) => "array".to_string(),
            Value::Object(obj) => String::from_utf8_lossy(&obj.borrow().class_name).into_owned(),
            Value::Generator(_) => "Generator".to_string(),
            Value::Reference(r) => Self::value_type_name(&r.borrow()),
        }
    }

    /// Check if an arithmetic operation has unsupported operand types (array vs non-array).
    /// Returns Some(error_message) if the types are incompatible, None if OK.
    /// For "add" (`op_symbol` = "+"), array + array is allowed (union).
    /// For all other arithmetic ops, arrays are never valid operands.
    fn check_unsupported_operand_types(a: &Value, b: &Value, op_symbol: &str) -> Option<String> {
        let a_deref = a.deref();
        let b_deref = b.deref();
        let a_is_array = matches!(a_deref, Value::Array(_));
        let b_is_array = matches!(b_deref, Value::Array(_));

        if op_symbol == "+" {
            // array + array is valid (union), but array + non-array or non-array + array is not
            if a_is_array && b_is_array {
                return None;
            }
            if a_is_array || b_is_array {
                return Some(format!(
                    "Unsupported operand types: {} + {}",
                    Self::value_type_name(&a_deref),
                    Self::value_type_name(&b_deref)
                ));
            }
        } else {
            // For sub, mul, div, mod, pow, **: arrays are never valid
            if a_is_array || b_is_array {
                return Some(format!(
                    "Unsupported operand types: {} {} {}",
                    Self::value_type_name(&a_deref),
                    op_symbol,
                    Self::value_type_name(&b_deref)
                ));
            }
        }

        // PHP 8: bitwise ops on fully non-numeric strings with non-strings throw TypeError
        // Leading-numeric strings (like "45some") produce a Warning but NOT a TypeError
        // Note: arithmetic ops (+, -, *, /, **) produce a Warning, not TypeError
        if matches!(op_symbol, "&" | "|" | "^" | "%" | "<<" | ">>") {
            // Helper: check if string is fully non-numeric (not even leading numeric)
            let is_fully_non_numeric = |s: &crate::string::PhpString| -> bool {
                let bytes = s.as_bytes();
                if bytes.is_empty() {
                    return true; // empty string is non-numeric
                }
                // Check if it starts with optional whitespace then a digit or sign
                let trimmed = std::str::from_utf8(bytes).unwrap_or("").trim();
                if trimmed.is_empty() {
                    return true;
                }
                let first = trimmed.as_bytes()[0];
                !(first.is_ascii_digit() || ((first == b'+' || first == b'-') && trimmed.len() > 1 && trimmed.as_bytes()[1].is_ascii_digit()))
            };

            let a_is_non_numeric_string = if let Value::String(s) = &a_deref {
                is_fully_non_numeric(s)
            } else {
                false
            };
            let b_is_non_numeric_string = if let Value::String(s) = &b_deref {
                is_fully_non_numeric(s)
            } else {
                false
            };

            // For bitwise ops (&, |, ^): string & string is valid (bitwise on bytes)
            // Only error when mixing non-numeric string with a non-string type
            if matches!(op_symbol, "&" | "|" | "^") {
                // Both strings: handled separately (bitwise on strings)
                if matches!(a_deref, Value::String(_)) && matches!(b_deref, Value::String(_)) {
                    return None;
                }
            }

            if (a_is_non_numeric_string && !matches!(b_deref, Value::String(_)))
                || (b_is_non_numeric_string && !matches!(a_deref, Value::String(_)))
            {
                return Some(format!(
                    "Unsupported operand types: {} {} {}",
                    Self::value_type_name(&a_deref),
                    op_symbol,
                    Self::value_type_name(&b_deref)
                ));
            }
        }
        None
    }

    /// Create a TypeError exception object and set it as current_exception.
    /// Returns the error message for use in VmError if no exception handler is available.
    pub fn throw_type_error(&mut self, message: String) -> Value {
        self.create_exception(b"TypeError", &message, self.current_line)
    }

    pub fn create_exception(&mut self, class_name: &[u8], message: &str, line: u32) -> Value {
        let err_id = self.next_object_id;
        self.next_object_id += 1;
        let mut err_obj = PhpObject::new(class_name.to_vec(), err_id);
        err_obj.set_property(
            b"message".to_vec(),
            Value::String(PhpString::from_string(message.to_string())),
        );
        err_obj.set_property(b"code".to_vec(), Value::Long(0));
        err_obj.set_property(b"file".to_vec(), Value::String(PhpString::from_string(self.current_file.clone())));
        err_obj.set_property(b"line".to_vec(), Value::Long(line as i64));
        Value::Object(Rc::new(RefCell::new(err_obj)))
    }

    /// Check if a return value matches the declared return type
    fn value_matches_return_type(&self, value: &Value, ret_type: &ParamType) -> bool {
        match ret_type {
            ParamType::Simple(name) => {
                match name.as_slice() {
                    b"void" => matches!(value, Value::Null | Value::Undef),
                    b"never" => false, // never type means function should never return
                    b"mixed" => true,
                    _ => self.value_matches_type(value, ret_type),
                }
            }
            ParamType::Nullable(_) => {
                if matches!(value, Value::Null | Value::Undef) {
                    return true;
                }
                self.value_matches_type(value, ret_type)
            }
            _ => self.value_matches_type(value, ret_type),
        }
    }

    /// Get a human-readable name for a ParamType
    fn param_type_name(&self, pt: &ParamType) -> String {
        self.param_type_name_inner(pt, false)
    }

    fn param_type_name_inner(&self, pt: &ParamType, in_union: bool) -> String {
        match pt {
            ParamType::Simple(name) => {
                match name.as_slice() {
                    b"self" => {
                        if let Some(scope) = self.class_scope_stack.last() {
                            // Look up original case class name
                            if let Some(class_entry) = self.classes.get(scope) {
                                return String::from_utf8_lossy(&class_entry.name).to_string();
                            }
                            return String::from_utf8_lossy(scope).to_string();
                        }
                    }
                    b"parent" => {
                        if let Some(scope) = self.class_scope_stack.last() {
                            if let Some(class_entry) = self.classes.get(scope) {
                                if let Some(parent_name) = &class_entry.parent {
                                    // Look up original case of parent
                                    let parent_lower: Vec<u8> = parent_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                                    if let Some(parent_entry) = self.classes.get(&parent_lower) {
                                        return String::from_utf8_lossy(&parent_entry.name).to_string();
                                    }
                                    return String::from_utf8_lossy(parent_name).to_string();
                                }
                            }
                        }
                    }
                    b"static" => {
                        if let Some(called) = self.called_class_stack.last() {
                            let called_lower: Vec<u8> = called.iter().map(|b| b.to_ascii_lowercase()).collect();
                            if let Some(class_entry) = self.classes.get(&called_lower) {
                                return String::from_utf8_lossy(&class_entry.name).to_string();
                            }
                            return String::from_utf8_lossy(called).to_string();
                        }
                    }
                    _ => {}
                }
                String::from_utf8_lossy(name).to_string()
            }
            ParamType::Nullable(inner) => format!("?{}", self.param_type_name_inner(inner, false)),
            ParamType::Union(types) => types
                .iter()
                .map(|t| self.param_type_name_inner(t, true))
                .collect::<Vec<_>>()
                .join("|"),
            ParamType::Intersection(types) => {
                let s = types
                    .iter()
                    .map(|t| self.param_type_name_inner(t, false))
                    .collect::<Vec<_>>()
                    .join("&");
                if in_union {
                    format!("({})", s)
                } else {
                    s
                }
            }
        }
    }

    /// Check if a value matches a single ParamType constraint.
    /// Returns true if the value is acceptable for the given type.
    fn value_matches_type(&self, value: &Value, param_type: &ParamType) -> bool {
        match param_type {
            ParamType::Simple(type_name) => {
                match type_name.as_slice() {
                    b"int" | b"integer" => {
                        // Non-strict: accept int, float (truncatable), bool, numeric strings
                        matches!(
                            value,
                            Value::Long(_) | Value::Double(_) | Value::True | Value::False
                        ) || matches!(value, Value::String(s) if crate::value::parse_numeric_string(s.as_bytes()).is_some())
                    }
                    b"float" | b"double" => {
                        matches!(
                            value,
                            Value::Double(_) | Value::Long(_) | Value::True | Value::False
                        ) || matches!(value, Value::String(s) if crate::value::parse_numeric_string(s.as_bytes()).is_some())
                    }
                    b"string" => {
                        // Non-strict: accept string, int, float, bool (all coercible)
                        matches!(
                            value,
                            Value::String(_)
                                | Value::Long(_)
                                | Value::Double(_)
                                | Value::True
                                | Value::False
                        )
                    }
                    b"bool" | b"boolean" => {
                        // Non-strict: accept any scalar
                        matches!(
                            value,
                            Value::True
                                | Value::False
                                | Value::Long(_)
                                | Value::Double(_)
                                | Value::String(_)
                                | Value::Null
                        )
                    }
                    b"array" => matches!(value, Value::Array(_)),
                    b"object" => matches!(value, Value::Object(_)),
                    b"callable" => {
                        // Callable: string (function name), array [obj/class, method], or closure
                        match value {
                            Value::String(_) => true,
                            Value::Array(_) => true,
                            Value::Object(obj) => {
                                let obj_borrow = obj.borrow();
                                let class_lower: Vec<u8> = obj_borrow
                                    .class_name
                                    .iter()
                                    .map(|b| b.to_ascii_lowercase())
                                    .collect();
                                class_lower == b"closure"
                            }
                            _ => false,
                        }
                    }
                    b"iterable" => matches!(value, Value::Array(_) | Value::Generator(_)),
                    b"mixed" => true,
                    b"null" => matches!(value, Value::Null),
                    b"void" => true, // void is for return types, skip checking
                    b"self" => {
                        // self type: value must be an object of the declaring class
                        if let Value::Object(obj) = value {
                            let obj_class_lower: Vec<u8> = obj.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                            if let Some(scope) = self.class_scope_stack.last() {
                                let scope_lower: Vec<u8> = scope.iter().map(|b| b.to_ascii_lowercase()).collect();
                                obj_class_lower == scope_lower || {
                                    // Check if the object's class extends the scope class
                                    if let Some(class_entry) = self.classes.get(&obj_class_lower) {
                                        self.class_is_a(class_entry, &scope_lower)
                                    } else {
                                        false
                                    }
                                }
                            } else {
                                true // no class context, skip checking
                            }
                        } else {
                            false
                        }
                    }
                    b"parent" => {
                        // parent type: value must be an object of the parent class
                        if let Value::Object(obj) = value {
                            let obj_class_lower: Vec<u8> = obj.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                            if let Some(scope) = self.class_scope_stack.last() {
                                let scope_lower: Vec<u8> = scope.iter().map(|b| b.to_ascii_lowercase()).collect();
                                if let Some(class_entry) = self.classes.get(&scope_lower) {
                                    if let Some(parent_name) = &class_entry.parent {
                                        let parent_lower: Vec<u8> = parent_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                                        obj_class_lower == parent_lower || {
                                            if let Some(obj_entry) = self.classes.get(&obj_class_lower) {
                                                self.class_is_a(obj_entry, &parent_lower)
                                            } else {
                                                false
                                            }
                                        }
                                    } else {
                                        false
                                    }
                                } else {
                                    true
                                }
                            } else {
                                true
                            }
                        } else {
                            false
                        }
                    }
                    b"static" => {
                        // static type: value must be an object of the called class (LSB)
                        if let Value::Object(obj) = value {
                            let obj_class_lower: Vec<u8> = obj.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                            if let Some(called) = self.called_class_stack.last() {
                                let called_lower: Vec<u8> = called.iter().map(|b| b.to_ascii_lowercase()).collect();
                                obj_class_lower == called_lower || {
                                    if let Some(class_entry) = self.classes.get(&obj_class_lower) {
                                        self.class_is_a(class_entry, &called_lower)
                                    } else {
                                        false
                                    }
                                }
                            } else {
                                true // no called class context, skip checking
                            }
                        } else {
                            false
                        }
                    }
                    b"false" => matches!(value, Value::False),
                    b"true" => matches!(value, Value::True),
                    b"never" => false, // never matches nothing
                    class_name => {
                        // Class/interface name check: value must be an object whose class matches
                        // class_name may have original case, so compare case-insensitively
                        let class_name_lower: Vec<u8> =
                            class_name.iter().map(|b| b.to_ascii_lowercase()).collect();

                        // Special case: Closure type - in goro-rs closures are strings/arrays, not objects
                        if class_name_lower == b"closure" {
                            return match value {
                                Value::String(s) => {
                                    let bytes = s.as_bytes();
                                    bytes.starts_with(b"__closure_")
                                        || bytes.starts_with(b"__arrow_")
                                        || self.user_functions.contains_key(bytes)
                                }
                                Value::Array(arr) => {
                                    // Closure with captured vars: [name, val1, val2, ...]
                                    let arr_borrow = arr.borrow();
                                    if let Some(first) = arr_borrow.values().next() {
                                        if let Value::String(s) = first {
                                            let bytes = s.as_bytes();
                                            bytes.starts_with(b"__closure_")
                                                || bytes.starts_with(b"__arrow_")
                                        } else {
                                            false
                                        }
                                    } else {
                                        false
                                    }
                                }
                                Value::Object(obj) => {
                                    let obj_borrow = obj.borrow();
                                    obj_borrow.class_name.eq_ignore_ascii_case(b"closure")
                                }
                                _ => false,
                            };
                        }

                        if let Value::Object(obj) = value {
                            let obj_borrow = obj.borrow();
                            let obj_class_lower: Vec<u8> = obj_borrow
                                .class_name
                                .iter()
                                .map(|b| b.to_ascii_lowercase())
                                .collect();
                            if obj_class_lower == class_name_lower {
                                return true;
                            }
                            // Check parent chain and interfaces
                            if let Some(class_entry) = self.classes.get(&obj_class_lower) {
                                return self.class_is_a(class_entry, &class_name_lower);
                            }
                            // Check built-in class hierarchy
                            if is_builtin_subclass(&obj_class_lower, &class_name_lower) {
                                return true;
                            }
                            // If target is a common interface/abstract class we don't track,
                            // be permissive and accept any object
                            let common_interfaces = [
                                &b"iterator"[..],
                                b"traversable",
                                b"countable",
                                b"arrayaccess",
                                b"serializable",
                                b"stringable",
                                b"iteratoraggregate",
                                b"throwable",
                                b"jsonserializable",
                            ];
                            if common_interfaces.contains(&class_name_lower.as_slice()) {
                                return true; // Accept any object for unresolvable interfaces
                            }
                            false
                        } else {
                            false
                        }
                    }
                }
            }
            ParamType::Nullable(inner) => {
                matches!(value, Value::Null) || self.value_matches_type(value, inner)
            }
            ParamType::Union(types) => types.iter().any(|t| self.value_matches_type(value, t)),
            ParamType::Intersection(types) => {
                types.iter().all(|t| self.value_matches_type(value, t))
            }
        }
    }

    /// Check if a class is (or inherits from / implements) a given type name
    fn class_is_a(&self, class_entry: &ClassEntry, target_lower: &[u8]) -> bool {
        // Check parent
        if let Some(parent_name) = &class_entry.parent {
            let parent_lower: Vec<u8> =
                parent_name.iter().map(|b| b.to_ascii_lowercase()).collect();
            if parent_lower == target_lower {
                return true;
            }
            if let Some(parent_entry) = self.classes.get(&parent_lower) {
                if self.class_is_a(parent_entry, target_lower) {
                    return true;
                }
            }
        }
        // Check interfaces
        for iface in &class_entry.interfaces {
            let iface_lower: Vec<u8> = iface.iter().map(|b| b.to_ascii_lowercase()).collect();
            if iface_lower == target_lower {
                return true;
            }
        }
        false
    }

    /// Get the display name for a ParamType (for error messages)
    fn param_type_display(&self, param_type: &ParamType) -> String {
        self.param_type_display_inner(param_type, false)
    }

    fn param_type_display_inner(&self, param_type: &ParamType, in_union: bool) -> String {
        match param_type {
            ParamType::Simple(name) => {
                // Resolve self/parent/static to actual class names
                match name.as_slice() {
                    b"self" | b"parent" | b"static" => {
                        return self.param_type_name(param_type);
                    }
                    _ => {}
                }
                String::from_utf8_lossy(name).to_string()
            }
            ParamType::Nullable(inner) => format!("?{}", self.param_type_display_inner(inner, false)),
            ParamType::Union(types) => types
                .iter()
                .map(|t| self.param_type_display_inner(t, true))
                .collect::<Vec<_>>()
                .join("|"),
            ParamType::Intersection(types) => {
                let s = types
                    .iter()
                    .map(|t| self.param_type_display_inner(t, false))
                    .collect::<Vec<_>>()
                    .join("&");
                if in_union {
                    format!("({})", s)
                } else {
                    s
                }
            }
        }
    }

    /// Check parameter types for a user function call. Returns an error message string if
    /// there is a type mismatch, or None if all checks pass.
    ///
    /// `implicit_args` is the number of leading arguments that are implicit (e.g., $this for
    /// methods) and should not be counted in the user-visible argument number.
    fn check_param_types(
        &self,
        user_fn: &OpArray,
        args: &[Value],
        func_display_name: &str,
        implicit_args: usize,
        line: u32,
    ) -> Option<String> {
        for (i, arg) in args.iter().enumerate() {
            if i >= user_fn.param_types.len() {
                continue;
            }
            // Skip Undef args - these are parameters not provided by the caller,
            // which will get their default values when the function body executes
            if matches!(arg, Value::Undef) {
                continue;
            }
            if let Some(type_info) = &user_fn.param_types[i] {
                let val = arg.deref();
                if !self.value_matches_type(&val, &type_info.param_type) {
                    let expected = self.param_type_display(&type_info.param_type);
                    let given = Self::value_type_name(&val);
                    let param_name = String::from_utf8_lossy(&type_info.param_name);
                    // Argument number is 1-based, excluding implicit args like $this
                    let arg_num = i + 1 - implicit_args;
                    return Some(format!(
                        "{}(): Argument #{} (${}) \
                         must be of type {}, {} given, called in {} on line {}",
                        func_display_name, arg_num, param_name, expected, given, self.current_file, line
                    ));
                }
            }
        }
        None
    }

    /// Execute a function OpArray with given CVs (public interface for ext crates)
    pub fn execute_fn(&mut self, op_array: &OpArray, cvs: Vec<Value>) -> Result<Value, VmError> {
        self.execute_op_array(op_array, cvs)
    }

    /// Execute a user function with named argument resolution.
    /// Takes positional args and named args, resolves them against the function's parameters,
    /// then calls the function.
    pub fn execute_fn_with_named_args(
        &mut self,
        op_array: &OpArray,
        positional_args: Vec<Value>,
        named_args: Vec<(Vec<u8>, Value)>,
        this_val: Option<Value>,
    ) -> Result<Value, VmError> {
        let implicit_args = if op_array.cv_names.first().map(|n| n.as_slice()) == Some(b"this") {
            1
        } else {
            0
        };

        // Build a PendingCall to resolve named args
        let mut call = PendingCall {
            name: PhpString::from_bytes(b""),
            args: if let Some(this) = this_val {
                let mut a = vec![this];
                a.extend(positional_args);
                a
            } else {
                positional_args
            },
            named_args,
        };

        if !call.named_args.is_empty() {
            if let Err(err_msg) = call.resolve_named_args(&op_array.cv_names, implicit_args, op_array.variadic_param) {
                let line = self.current_line;
                let exc_val = self.create_exception(b"Error", &err_msg, line);
                self.current_exception = Some(exc_val);
                return Err(VmError {
                    message: format!("Uncaught Error: {}", err_msg),
                    line,
                });
            }
        }

        // Set up CVs
        let mut func_cvs = vec![Value::Undef; op_array.cv_names.len()];
        if let Some(variadic_idx) = op_array.variadic_param {
            let vi = variadic_idx as usize;
            for (i, arg) in call.args.iter().enumerate() {
                if i < vi && i < func_cvs.len() {
                    func_cvs[i] = arg.clone();
                }
            }
            let mut variadic_arr = crate::array::PhpArray::new();
            for arg in call.args.iter().skip(vi) {
                variadic_arr.push(arg.clone());
            }
            // Add extra named args with string keys
            for (name, val) in call.named_args.drain(..) {
                variadic_arr.set(
                    ArrayKey::String(PhpString::from_vec(name)),
                    val,
                );
            }
            if vi < func_cvs.len() {
                func_cvs[vi] = Value::Array(Rc::new(RefCell::new(variadic_arr)));
            }
        } else {
            for (i, arg) in call.args.iter().enumerate() {
                if i < func_cvs.len() {
                    func_cvs[i] = arg.clone();
                }
            }
        }

        self.execute_op_array(op_array, func_cvs)
    }

    /// Register a class (from the compiler's compiled_classes list)
    pub fn register_class(&mut self, class: ClassEntry) {
        self.pending_classes.push(class);
    }

    /// Register a user-defined function
    pub fn register_user_function(&mut self, name: &[u8], op_array: OpArray) {
        self.user_functions
            .insert(name.to_ascii_lowercase(), op_array);
    }

    /// Bind a closure to a new $this and/or scope class.
    /// Returns the new closure value (string or array), or Value::Null on failure.
    pub fn bind_closure(&mut self, closure_val: &Value, new_this: Value, scope: Value, scope_provided: bool) -> Value {
        // Extract the closure function name and any existing captured values
        let (closure_name, captured_values) = match closure_val {
            Value::String(s) => {
                (s.as_bytes().to_vec(), Vec::new())
            }
            Value::Array(arr) => {
                let arr_borrow = arr.borrow();
                let mut values: Vec<Value> = arr_borrow.values().cloned().collect();
                if values.is_empty() {
                    return Value::Null;
                }
                let name = values.remove(0).to_php_string().as_bytes().to_vec();
                (name, values)
            }
            _ => return Value::Null,
        };

        // Look up the original closure's OpArray
        let closure_name_lower: Vec<u8> = closure_name.iter().map(|b| b.to_ascii_lowercase()).collect();
        let original_op = match self.user_functions.get(&closure_name_lower) {
            Some(op) => op.clone(),
            None => return Value::Null,
        };

        // Validate scope argument type
        if scope_provided {
            match &scope {
                Value::Null | Value::String(_) | Value::Object(_) => {}
                _ => {
                    let type_name = match &scope {
                        Value::Array(_) => "array",
                        Value::Long(_) => "int",
                        Value::Double(_) => "float",
                        Value::True | Value::False => "bool",
                        _ => "unknown",
                    };
                    let exc = self.throw_type_error(format!(
                        "Closure::bindTo(): Argument #2 ($newScope) must be of type object|string|null, {} given",
                        type_name
                    ));
                    self.current_exception = Some(exc);
                    return Value::Null;
                }
            }
        }

        // Check if trying to bind an instance to a static closure
        if !matches!(new_this, Value::Null) && original_op.is_static_closure {
            self.emit_warning("Cannot bind an instance to a static closure, this will be an error in PHP 9");
        }

        // Find $this CV position in the original closure
        let this_cv_pos = original_op.cv_names.iter().position(|n| n == b"this");

        // Check if closure uses $this - if trying to unbind, warn and return NULL
        if matches!(new_this, Value::Null) && this_cv_pos == Some(0) {
            // Check if $this was actually captured (i.e., it was in the captured values)
            let had_this_captured = !captured_values.is_empty();
            if had_this_captured {
                // Check if the closure actually uses $this in opcodes
                let uses_this = original_op.ops.iter().any(|instr| {
                    matches!(instr.op1, OperandType::Cv(0))
                        || matches!(instr.op2, OperandType::Cv(0))
                        || matches!(instr.result, OperandType::Cv(0))
                });
                if uses_this {
                    self.emit_warning("Cannot unbind $this of closure using $this, this will be an error in PHP 9");
                    return Value::Null;
                }
            }
        }

        // Determine the new scope class
        let new_scope: Option<Vec<u8>> = if scope_provided {
            match &scope {
                Value::Null => None,
                Value::String(s) => {
                    let bytes = s.as_bytes();
                    if bytes.eq_ignore_ascii_case(b"static") {
                        original_op.scope_class.clone()
                    } else {
                        Some(bytes.to_ascii_lowercase())
                    }
                }
                Value::Object(obj) => {
                    let obj_borrow = obj.borrow();
                    Some(obj_borrow.class_name.to_ascii_lowercase())
                }
                _ => original_op.scope_class.clone(),
            }
        } else {
            if !matches!(new_this, Value::Null) {
                if let Some(scope) = &original_op.scope_class {
                    Some(scope.clone())
                } else if let Value::Object(obj) = &new_this {
                    let obj_borrow = obj.borrow();
                    Some(obj_borrow.class_name.to_ascii_lowercase())
                } else {
                    None
                }
            } else {
                original_op.scope_class.clone()
            }
        };

        // Create a new bound closure name
        let bound_id = self.next_bound_closure_id;
        self.next_bound_closure_id += 1;
        let new_closure_name = format!("__bound_closure_{}", bound_id).into_bytes();

        // Clone and modify the OpArray
        let mut new_op = original_op.clone();
        new_op.name = new_closure_name.clone();
        new_op.scope_class = new_scope;

        // Determine the $this CV position in the new OpArray
        let needs_this_cv = !matches!(new_this, Value::Null);
        if needs_this_cv {
            if let Some(pos) = this_cv_pos {
                if pos != 0 {
                    // $this exists but not at CV[0] - move it there
                    // Remove from current position
                    new_op.cv_names.remove(pos);
                    // Insert at 0
                    new_op.cv_names.insert(0, b"this".to_vec());
                    // Remap CV references in opcodes:
                    // Old CV[pos] -> new CV[0]
                    // Old CV[0..pos-1] -> new CV[1..pos]
                    // Old CV[pos+1..] -> unchanged
                    let pos_u32 = pos as u32;
                    for op_instr in &mut new_op.ops {
                        remap_cv_operand(&mut op_instr.op1, pos_u32);
                        remap_cv_operand(&mut op_instr.op2, pos_u32);
                        remap_cv_operand(&mut op_instr.result, pos_u32);
                    }
                }
                // If pos == 0, it's already there, nothing to do
            } else {
                // $this doesn't exist at all - insert at CV[0], shifting everything
                new_op.cv_names.insert(0, b"this".to_vec());
                for op_instr in &mut new_op.ops {
                    shift_cv_operand(&mut op_instr.op1);
                    shift_cv_operand(&mut op_instr.op2);
                    shift_cv_operand(&mut op_instr.result);
                }
                if !new_op.param_types.is_empty() {
                    new_op.param_types.insert(0, None);
                }
            }
        };

        // Register the new function
        self.register_user_function(&new_closure_name, new_op);

        // Build the result capture array
        // The capture array: [closure_name, captured_val_for_cv0, captured_val_for_cv1, ...]
        // These captured values map sequentially to CVs starting at CV[0].
        // Parameters come after via SendVal.

        if needs_this_cv || !captured_values.is_empty() {
            let mut result_values: Vec<Value> = Vec::new();
            result_values.push(Value::String(PhpString::from_vec(new_closure_name)));

            if needs_this_cv {
                if this_cv_pos == Some(0) {
                    // $this was at CV[0] and stays there - replace in captures
                    result_values.push(new_this);
                    if captured_values.len() > 1 {
                        result_values.extend(captured_values.into_iter().skip(1));
                    }
                } else {
                    // $this was either at a non-zero position or didn't exist
                    // After remap/insert, it's now at CV[0]
                    // Add $this as the first captured value
                    result_values.push(new_this);
                    // Add the rest of the captured values (they map to CV[1], CV[2], ...)
                    result_values.extend(captured_values);
                }
            } else {
                // Not binding $this
                if this_cv_pos == Some(0) && !captured_values.is_empty() {
                    // Unbinding $this - skip it from captures
                    result_values.extend(captured_values.into_iter().skip(1));
                } else {
                    result_values.extend(captured_values);
                }
                if result_values.len() == 1 {
                    // Only the name, return as string
                    return result_values.remove(0);
                }
            }

            let mut arr = crate::array::PhpArray::new();
            for v in result_values {
                arr.push(v);
            }
            Value::Array(Rc::new(RefCell::new(arr)))
        } else {
            Value::String(PhpString::from_vec(new_closure_name))
        }
    }

    /// Register a built-in function
    pub fn register_function(&mut self, name: &[u8], func: BuiltinFn) {
        self.functions.insert(name.to_ascii_lowercase(), func);
    }

    /// Register a built-in function with parameter names (for named argument support)
    pub fn register_function_with_params(&mut self, name: &[u8], func: BuiltinFn, params: &[&[u8]]) {
        let lower = name.to_ascii_lowercase();
        self.functions.insert(lower.clone(), func);
        self.builtin_param_names.insert(lower, params.iter().map(|p| p.to_vec()).collect());
    }

    /// Get the output buffer contents
    /// Format the current call stack as a PHP-style stack trace string
    /// Check method signature compatibility between child and parent methods.
    /// Returns None if compatible, Some(error_message) if not.
    fn check_method_compatibility(
        child_class: &[u8],
        child_method: &crate::object::MethodDef,
        parent_class: &[u8],
        parent_method: &crate::object::MethodDef,
    ) -> Option<String> {
        let child_op = &child_method.op_array;
        let parent_op = &parent_method.op_array;

        // Subtract $this from param counts for non-static methods
        let child_offset: u32 = if child_method.is_static { 0 } else { 1 };
        let parent_offset: u32 = if parent_method.is_static { 0 } else { 1 };
        let child_params = child_op.param_count.saturating_sub(child_offset);
        let parent_params = parent_op.param_count.saturating_sub(parent_offset);
        let child_required = child_op.required_param_count;
        let parent_required = parent_op.required_param_count;
        let child_variadic = child_op.variadic_param.is_some();
        let parent_variadic = parent_op.variadic_param.is_some();

        // Build parameter signature strings for error messages
        let format_param_sig = |op: &crate::opcode::OpArray, class_name: &[u8]| -> String {
            let mut parts = Vec::new();
            // Determine offset: methods have $this at CV 0, functions don't
            let cv_offset = if op.scope_class.is_some() { 1usize } else { 0 };
            let actual_param_count = (op.param_count as usize).saturating_sub(cv_offset);
            for i in 0..actual_param_count {
                let cv_idx = i + cv_offset;
                let param_name = op.cv_names.get(cv_idx)
                    .map(|n| String::from_utf8_lossy(n).to_string())
                    .unwrap_or_else(|| format!("arg{}", i));

                let mut part = String::new();
                // Add type hint if available
                if let Some(Some(pt)) = op.param_types.get(i) {
                    let type_str = Self::format_param_type_for_sig(&pt.param_type, class_name);
                    if !type_str.is_empty() {
                        part.push_str(&type_str);
                        part.push(' ');
                    }
                }

                // Check if variadic
                if op.variadic_param == Some(cv_idx as u32) {
                    part.push_str("...");
                }

                part.push('$');
                part.push_str(&param_name.trim_start_matches('$'));

                // Check if has default value (optional)
                if i as u32 >= op.required_param_count && op.variadic_param != Some(cv_idx as u32) {
                    part.push_str(" = null");
                }

                parts.push(part);
            }
            parts.join(", ")
        };

        // Check: child must not have more required parameters than parent
        // Check: child must accept at least as many parameters as parent
        let incompatible = if !parent_variadic && !child_variadic {
            // Neither is variadic
            child_required > parent_required || child_params < parent_params
        } else if parent_variadic && !child_variadic {
            // Parent is variadic but child is not - child must accept at least parent's non-variadic params
            child_params < (parent_params.saturating_sub(1))
        } else {
            // Child is variadic - generally OK as long as required params match
            child_required > parent_required
        };

        // Check return type compatibility (basic: if parent has return type, child must also have it)
        let return_type_incompatible = if let Some(parent_rt) = &parent_op.return_type {
            if let Some(_child_rt) = &child_op.return_type {
                // Both have return types - for now, don't check covariance (complex)
                false
            } else {
                // Parent has return type but child doesn't
                // This is OK in PHP 8.x (it's only a warning in strict mode)
                false
            }
        } else {
            // Parent has no return type - child can add one
            false
        };

        if incompatible || return_type_incompatible {
            let child_class_str = String::from_utf8_lossy(child_class);
            let parent_class_str = String::from_utf8_lossy(parent_class);
            let method_name = String::from_utf8_lossy(&child_method.name);
            let parent_method_name = String::from_utf8_lossy(&parent_method.name);

            let child_sig = format_param_sig(child_op, child_class);
            let parent_sig = format_param_sig(parent_op, parent_class);

            // Format return types
            let child_ret = child_op.return_type.as_ref()
                .map(|rt| format!(": {}", Self::format_param_type_for_sig(rt, child_class)))
                .unwrap_or_default();
            let parent_ret = parent_op.return_type.as_ref()
                .map(|rt| format!(": {}", Self::format_param_type_for_sig(rt, parent_class)))
                .unwrap_or_default();

            Some(format!(
                "Declaration of {}::{}({}){} must be compatible with {}::{}({}){}",
                child_class_str, method_name, child_sig, child_ret,
                parent_class_str, parent_method_name, parent_sig, parent_ret,
            ))
        } else {
            None
        }
    }

    /// Format a ParamType for signature display
    fn format_param_type_for_sig(pt: &crate::opcode::ParamType, _class_name: &[u8]) -> String {
        use crate::opcode::ParamType;
        match pt {
            ParamType::Simple(name) => String::from_utf8_lossy(name).to_string(),
            ParamType::Nullable(inner) => format!("?{}", Self::format_param_type_for_sig(inner, _class_name)),
            ParamType::Union(types) => types.iter()
                .map(|t| Self::format_param_type_for_sig(t, _class_name))
                .collect::<Vec<_>>()
                .join("|"),
            ParamType::Intersection(types) => types.iter()
                .map(|t| Self::format_param_type_for_sig(t, _class_name))
                .collect::<Vec<_>>()
                .join("&"),
        }
    }

    pub fn format_stack_trace(&self) -> String {
        let mut lines = Vec::new();
        // The call stack is ordered from outermost to innermost
        // PHP shows it innermost first
        for (i, (func_name, file, line, args, is_instance)) in self.call_stack.iter().rev().enumerate() {
            let file_display = if file == "Unknown.php" || file.is_empty() {
                &self.current_file
            } else {
                file
            };
            let args_str = format_trace_args(args);
            // For instance methods, replace :: with ->
            let display_name = if *is_instance {
                func_name.replacen("::", "->", 1)
            } else {
                func_name.clone()
            };
            lines.push(format!("#{} {}({}): {}({})", i, file_display, line, display_name, args_str));
        }
        lines.push(format!("#{} {{main}}", self.call_stack.len()));
        lines.join("\n")
    }

    pub fn output(&self) -> &[u8] {
        &self.output
    }

    /// Format a single argument value for a stack trace
    pub fn format_trace_arg(val: &Value) -> String {
        match val {
            Value::Null | Value::Undef => "NULL".to_string(),
            Value::True => "true".to_string(),
            Value::False => "false".to_string(),
            Value::Long(n) => n.to_string(),
            Value::Double(f) => {
                if f.is_infinite() {
                    if *f > 0.0 { "INF".to_string() } else { "-INF".to_string() }
                } else if f.is_nan() {
                    "NAN".to_string()
                } else {
                    crate::value::format_php_float(*f)
                }
            }
            Value::String(s) => {
                let lossy = s.to_string_lossy();
                // PHP truncates to 15 chars with "..." suffix by default
                if lossy.len() > 15 {
                    format!("'{:.15}...'", lossy)
                } else {
                    format!("'{}'", lossy)
                }
            }
            Value::Array(_) => "Array".to_string(),
            Value::Object(obj) => {
                let obj_ref = obj.borrow();
                format!("Object({})", String::from_utf8_lossy(&obj_ref.class_name))
            }
            Value::Reference(r) => {
                let inner = r.borrow();
                Self::format_trace_arg(&inner)
            }
            Value::Generator(_) => "Object(Generator)".to_string(),
        }
    }

    /// Take the output buffer
    pub fn take_output(&mut self) -> Vec<u8> {
        // Flush all output buffers (ob_start without matching ob_end_flush)
        while let Some(buf) = self.ob_stack.pop() {
            // Each buffer's content goes to the parent buffer or main output
            if let Some(parent) = self.ob_stack.last_mut() {
                parent.extend_from_slice(&buf);
            } else {
                self.output.extend_from_slice(&buf);
            }
        }
        std::mem::take(&mut self.output)
    }

    /// Write to the output buffer
    /// Check if class_name extends target_name through the parent chain
    pub fn class_extends(&self, class_name: &[u8], target_name: &[u8]) -> bool {
        let mut current: Vec<u8> = class_name.to_vec();
        for _ in 0..50 {
            // prevent infinite loops
            let parent = match self.classes.get(&current) {
                Some(ce) => match &ce.parent {
                    Some(p) => p.iter().map(|b| b.to_ascii_lowercase()).collect::<Vec<u8>>(),
                    None => return false,
                },
                None => {
                    // Current class is not user-defined; check if it's a built-in
                    // that extends the target through the built-in hierarchy
                    return is_builtin_subclass(&current, target_name);
                }
            };
            if parent == target_name {
                return true;
            }
            current = parent;
        }
        false
    }

    /// Check if a class (by lowercase name) implements a given interface (by lowercase name).
    /// Walks up the parent chain and checks each class's interfaces list.
    fn class_implements_interface(&self, class_name: &[u8], iface_name: &[u8]) -> bool {
        let mut current: Vec<u8> = class_name.to_vec();
        for _ in 0..50 {
            if let Some(ce) = self.classes.get(&current) {
                // Check this class's interfaces
                for iface in &ce.interfaces {
                    let iface_lower: Vec<u8> = iface.iter().map(|b| b.to_ascii_lowercase()).collect();
                    if iface_lower == iface_name {
                        return true;
                    }
                    // Check if the interface itself extends the target interface
                    if self.class_implements_interface(&iface_lower, iface_name) {
                        return true;
                    }
                }
                // Walk up to parent
                match &ce.parent {
                    Some(p) => {
                        current = p.iter().map(|b| b.to_ascii_lowercase()).collect();
                    }
                    None => return false,
                }
            } else {
                // Built-in class - check known built-in interfaces
                return self.builtin_implements_interface(&current, iface_name);
            }
        }
        false
    }

    /// Check if a built-in class implements a given interface
    fn builtin_implements_interface(&self, class_lower: &[u8], iface_name: &[u8]) -> bool {
        match iface_name {
            b"iterator" => matches!(
                class_lower,
                b"arrayiterator" | b"splfixedarray" | b"spldoublylinkedlist"
                    | b"splstack" | b"splqueue" | b"splpriorityqueue"
            ),
            b"iteratoraggregate" => matches!(class_lower, b"arrayobject"),
            _ => false,
        }
    }

    /// Dispatch method calls for SPL built-in classes
    fn dispatch_spl_method(
        &mut self,
        class_lower: &[u8],
        method_lower: &[u8],
        obj: &Rc<RefCell<PhpObject>>,
    ) -> Option<Value> {
        match class_lower {
            b"arrayobject" | b"arrayiterator" => {
                self.spl_array_method(method_lower, obj)
            }
            b"splfixedarray" => {
                self.spl_fixed_array_method(method_lower, obj)
            }
            b"spldoublylinkedlist" | b"splstack" | b"splqueue" => {
                self.spl_linked_list_method(method_lower, obj)
            }
            b"splobjectstorage" => {
                self.spl_object_storage_method(method_lower, obj)
            }
            b"splpriorityqueue" => {
                self.spl_priority_queue_method(method_lower, obj)
            }
            b"datetime" | b"datetimeimmutable" => {
                match method_lower {
                    b"gettimestamp" => {
                        let ob = obj.borrow();
                        Some(ob.get_property(b"timestamp"))
                    }
                    _ => None, // format() and others handled via __spl:: dispatch
                }
            }
            _ => None,
        }
    }

    fn spl_array_method(
        &mut self,
        method: &[u8],
        obj: &Rc<RefCell<PhpObject>>,
    ) -> Option<Value> {
        let storage_prop = b"__spl_array";
        match method {
            b"offsetget" => {
                // Retrieved from pending call args later
                None
            }
            b"offsetset" => None,
            b"offsetexists" => None,
            b"offsetunset" => None,
            b"count" => {
                let ob = obj.borrow();
                let arr = ob.get_property(storage_prop);
                if let Value::Array(a) = arr {
                    Some(Value::Long(a.borrow().len() as i64))
                } else {
                    Some(Value::Long(0))
                }
            }
            b"getarraycopy" => {
                let ob = obj.borrow();
                let arr = ob.get_property(storage_prop);
                if let Value::Array(a) = arr {
                    Some(Value::Array(Rc::new(RefCell::new(a.borrow().clone()))))
                } else {
                    Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
                }
            }
            b"append" => None,
            b"getflags" => {
                let ob = obj.borrow();
                let flags = ob.get_property(b"__spl_flags");
                Some(flags)
            }
            b"setflags" => None,
            b"getiterator" => None,
            _ => None,
        }
    }

    fn spl_fixed_array_method(
        &mut self,
        method: &[u8],
        obj: &Rc<RefCell<PhpObject>>,
    ) -> Option<Value> {
        match method {
            b"count" | b"getsize" => {
                let ob = obj.borrow();
                let size = ob.get_property(b"__spl_size");
                Some(size)
            }
            b"setsize" => None,
            b"toarray" => {
                let ob = obj.borrow();
                let arr = ob.get_property(b"__spl_array");
                if let Value::Array(a) = arr {
                    Some(Value::Array(Rc::new(RefCell::new(a.borrow().clone()))))
                } else {
                    Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
                }
            }
            _ => None,
        }
    }

    fn spl_linked_list_method(
        &mut self,
        method: &[u8],
        obj: &Rc<RefCell<PhpObject>>,
    ) -> Option<Value> {
        let storage_prop = b"__spl_array";
        match method {
            b"count" => {
                let ob = obj.borrow();
                let arr = ob.get_property(storage_prop);
                if let Value::Array(a) = arr {
                    Some(Value::Long(a.borrow().len() as i64))
                } else {
                    Some(Value::Long(0))
                }
            }
            b"isempty" => {
                let ob = obj.borrow();
                let arr = ob.get_property(storage_prop);
                if let Value::Array(a) = arr {
                    Some(if a.borrow().len() == 0 { Value::True } else { Value::False })
                } else {
                    Some(Value::True)
                }
            }
            b"top" => {
                let ob = obj.borrow();
                let arr = ob.get_property(storage_prop);
                if let Value::Array(a) = arr {
                    let a = a.borrow();
                    if a.len() == 0 {
                        // top() on empty list should throw, but for now return null
                        Some(Value::Null)
                    } else {
                        Some(a.values().last().cloned().unwrap_or(Value::Null))
                    }
                } else {
                    Some(Value::Null)
                }
            }
            b"current" => {
                let ob = obj.borrow();
                let arr = ob.get_property(storage_prop);
                let pos = ob.get_property(b"__spl_pos");
                let iter_mode = ob.get_property(b"__spl_iter_mode");
                let mode = if let Value::Long(m) = iter_mode { m } else { 0 };
                let pos_val = if let Value::Long(p) = pos { p as usize } else { 0 };
                if let Value::Array(a) = arr {
                    let a = a.borrow();
                    let len = a.len();
                    if len == 0 {
                        Some(Value::Null)
                    } else {
                        // Check IT_MODE_LIFO (bit 1) - if set, iterate in reverse
                        let actual_pos = if (mode & 2) != 0 {
                            if pos_val < len { len - 1 - pos_val } else { return Some(Value::Null); }
                        } else {
                            pos_val
                        };
                        Some(a.values().nth(actual_pos).cloned().unwrap_or(Value::Null))
                    }
                } else {
                    Some(Value::Null)
                }
            }
            b"bottom" => {
                let ob = obj.borrow();
                let arr = ob.get_property(storage_prop);
                if let Value::Array(a) = arr {
                    let a = a.borrow();
                    if a.len() == 0 {
                        Some(Value::Null)
                    } else {
                        Some(a.values().next().cloned().unwrap_or(Value::Null))
                    }
                } else {
                    Some(Value::Null)
                }
            }
            b"valid" => {
                let ob = obj.borrow();
                let arr = ob.get_property(storage_prop);
                let pos = ob.get_property(b"__spl_pos");
                let pos_val = if let Value::Long(p) = pos { p } else { 0 };
                if let Value::Array(a) = arr {
                    let a = a.borrow();
                    Some(if (pos_val as usize) < a.len() { Value::True } else { Value::False })
                } else {
                    Some(Value::False)
                }
            }
            b"key" => {
                let ob = obj.borrow();
                let pos = ob.get_property(b"__spl_pos");
                Some(if let Value::Long(p) = pos { Value::Long(p) } else { Value::Long(0) })
            }
            b"next" => {
                let mut ob = obj.borrow_mut();
                let pos = ob.get_property(b"__spl_pos");
                let pos_val = if let Value::Long(p) = pos { p } else { 0 };
                ob.set_property(b"__spl_pos".to_vec(), Value::Long(pos_val + 1));
                Some(Value::Null)
            }
            b"prev" => {
                let mut ob = obj.borrow_mut();
                let pos = ob.get_property(b"__spl_pos");
                let pos_val = if let Value::Long(p) = pos { p } else { 0 };
                ob.set_property(b"__spl_pos".to_vec(), Value::Long(if pos_val > 0 { pos_val - 1 } else { 0 }));
                Some(Value::Null)
            }
            b"rewind" => {
                let mut ob = obj.borrow_mut();
                ob.set_property(b"__spl_pos".to_vec(), Value::Long(0));
                Some(Value::Null)
            }
            b"getiteratormode" => {
                let ob = obj.borrow();
                let mode = ob.get_property(b"__spl_iter_mode");
                Some(if let Value::Long(_) = mode { mode } else { Value::Long(0) })
            }
            _ => None,
        }
    }

    fn spl_object_storage_method(
        &mut self,
        method: &[u8],
        obj: &Rc<RefCell<PhpObject>>,
    ) -> Option<Value> {
        match method {
            b"count" => {
                let ob = obj.borrow();
                let arr = ob.get_property(b"__spl_array");
                if let Value::Array(a) = arr {
                    Some(Value::Long(a.borrow().len() as i64))
                } else {
                    Some(Value::Long(0))
                }
            }
            b"contains" => None,
            _ => None,
        }
    }

    fn spl_priority_queue_method(
        &mut self,
        method: &[u8],
        obj: &Rc<RefCell<PhpObject>>,
    ) -> Option<Value> {
        match method {
            b"count" => {
                let ob = obj.borrow();
                let arr = ob.get_property(b"__spl_array");
                if let Value::Array(a) = arr {
                    Some(Value::Long(a.borrow().len() as i64))
                } else {
                    Some(Value::Long(0))
                }
            }
            b"isempty" => {
                let ob = obj.borrow();
                let arr = ob.get_property(b"__spl_array");
                if let Value::Array(a) = arr {
                    Some(if a.borrow().len() == 0 { Value::True } else { Value::False })
                } else {
                    Some(Value::True)
                }
            }
            _ => None,
        }
    }

    /// Check if this is an SPL method that needs call arguments
    fn is_spl_args_method(&self, class: &[u8], method: &[u8]) -> bool {
        match class {
            b"arrayobject" | b"arrayiterator" | b"recursivearrayiterator" => matches!(
                method,
                b"offsetget" | b"offsetset" | b"offsetexists" | b"offsetunset"
                    | b"append" | b"exchangearray" | b"setflags"
            ),
            b"spldoublylinkedlist" | b"splstack" | b"splqueue" => matches!(
                method,
                b"push" | b"pop" | b"shift" | b"unshift" | b"enqueue" | b"dequeue"
                    | b"setiteratormode" | b"offsetget" | b"offsetset"
                    | b"offsetexists" | b"offsetunset" | b"add"
            ),
            b"splfixedarray" => matches!(
                method,
                b"offsetget" | b"offsetset" | b"offsetexists" | b"offsetunset" | b"setsize"
            ),
            b"splobjectstorage" => matches!(
                method,
                b"attach" | b"detach" | b"contains" | b"offsetget" | b"offsetset"
            ),
            b"splpriorityqueue" => matches!(method, b"insert" | b"extract"),
            b"datetime" | b"datetimeimmutable" => matches!(method, b"format" | b"modify" | b"settimezone" | b"settime" | b"setdate" | b"settimestamp" | b"add" | b"sub" | b"diff"),
            _ => false,
        }
    }

    /// Handle SPL method calls with arguments at DoCall time
    fn handle_spl_docall(
        &mut self,
        class: &[u8],
        method: &[u8],
        args: &[Value],
    ) -> Option<Value> {
        // args[0] is $this, rest are method args
        let this = args.first()?;
        if let Value::Object(obj) = this {
            match class {
                b"arrayobject" | b"arrayiterator" | b"recursivearrayiterator" => {
                    match method {
                        b"offsetget" => {
                            let key = args.get(1)?;
                            let ob = obj.borrow();
                            let arr = ob.get_property(b"__spl_array");
                            if let Value::Array(a) = arr {
                                let k = Self::value_to_array_key(key.clone());
                                Some(a.borrow().get(&k).cloned().unwrap_or(Value::Null))
                            } else {
                                Some(Value::Null)
                            }
                        }
                        b"offsetset" => {
                            let key = args.get(1).cloned().unwrap_or(Value::Null);
                            let val = args.get(2).cloned().unwrap_or(Value::Null);
                            let mut ob = obj.borrow_mut();
                            let arr = ob.get_property(b"__spl_array");
                            if let Value::Array(a) = arr {
                                if matches!(key, Value::Null) {
                                    a.borrow_mut().push(val);
                                } else {
                                    let k = Self::value_to_array_key(key);
                                    a.borrow_mut().set(k, val);
                                }
                            }
                            Some(Value::Null)
                        }
                        b"offsetexists" => {
                            let key = args.get(1)?;
                            let ob = obj.borrow();
                            let arr = ob.get_property(b"__spl_array");
                            if let Value::Array(a) = arr {
                                let k = Self::value_to_array_key(key.clone());
                                Some(if a.borrow().get(&k).is_some() { Value::True } else { Value::False })
                            } else {
                                Some(Value::False)
                            }
                        }
                        b"offsetunset" => {
                            let key = args.get(1)?;
                            let ob = obj.borrow();
                            let arr = ob.get_property(b"__spl_array");
                            if let Value::Array(a) = arr {
                                let k = Self::value_to_array_key(key.clone());
                                a.borrow_mut().remove(&k);
                            }
                            Some(Value::Null)
                        }
                        b"append" => {
                            let val = args.get(1).cloned().unwrap_or(Value::Null);
                            let mut ob = obj.borrow_mut();
                            let arr = ob.get_property(b"__spl_array");
                            if let Value::Array(a) = arr {
                                a.borrow_mut().push(val);
                            }
                            Some(Value::Null)
                        }
                        b"exchangearray" => {
                            let new_arr = args.get(1).cloned().unwrap_or(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
                            let mut ob = obj.borrow_mut();
                            let old = ob.get_property(b"__spl_array");
                            ob.set_property(b"__spl_array".to_vec(), new_arr);
                            Some(old)
                        }
                        b"setflags" => {
                            let flags = args.get(1).cloned().unwrap_or(Value::Long(0));
                            let mut ob = obj.borrow_mut();
                            ob.set_property(b"__spl_flags".to_vec(), flags);
                            Some(Value::Null)
                        }
                        _ => None,
                    }
                }
                b"spldoublylinkedlist" | b"splstack" | b"splqueue" => {
                    match method {
                        b"push" | b"enqueue" => {
                            let val = args.get(1).cloned().unwrap_or(Value::Null);
                            let ob = obj.borrow();
                            let arr = ob.get_property(b"__spl_array");
                            if let Value::Array(a) = arr {
                                a.borrow_mut().push(val);
                            }
                            Some(Value::Null)
                        }
                        b"pop" => {
                            let ob = obj.borrow();
                            let arr = ob.get_property(b"__spl_array");
                            if let Value::Array(a) = arr {
                                let mut a = a.borrow_mut();
                                a.pop().map(|v| Some(v)).unwrap_or(Some(Value::Null))
                            } else {
                                Some(Value::Null)
                            }
                        }
                        b"dequeue" | b"shift" => {
                            let ob = obj.borrow();
                            let arr = ob.get_property(b"__spl_array");
                            if let Value::Array(a) = arr {
                                let mut a = a.borrow_mut();
                                a.shift().map(|v| Some(v)).unwrap_or(Some(Value::Null))
                            } else {
                                Some(Value::Null)
                            }
                        }
                        b"unshift" => {
                            let val = args.get(1).cloned().unwrap_or(Value::Null);
                            let ob = obj.borrow();
                            let arr = ob.get_property(b"__spl_array");
                            if let Value::Array(a) = arr {
                                let mut new_arr = PhpArray::new();
                                new_arr.push(val);
                                for (_, v) in a.borrow().iter() {
                                    new_arr.push(v.clone());
                                }
                                *a.borrow_mut() = new_arr;
                            }
                            Some(Value::Null)
                        }
                        b"setiteratormode" => {
                            let mode = args.get(1).cloned().unwrap_or(Value::Long(0));
                            let mut ob = obj.borrow_mut();
                            ob.set_property(b"__spl_iter_mode".to_vec(), mode);
                            Some(Value::Null)
                        }
                        b"offsetget" => {
                            let key = args.get(1)?;
                            let ob = obj.borrow();
                            let arr = ob.get_property(b"__spl_array");
                            if let Value::Array(a) = arr {
                                let k = Self::value_to_array_key(key.clone());
                                Some(a.borrow().get(&k).cloned().unwrap_or(Value::Null))
                            } else {
                                Some(Value::Null)
                            }
                        }
                        b"offsetset" => {
                            let key = args.get(1).cloned().unwrap_or(Value::Null);
                            let val = args.get(2).cloned().unwrap_or(Value::Null);
                            let ob = obj.borrow();
                            let arr = ob.get_property(b"__spl_array");
                            if let Value::Array(a) = arr {
                                if matches!(key, Value::Null) {
                                    a.borrow_mut().push(val);
                                } else {
                                    let k = Self::value_to_array_key(key);
                                    a.borrow_mut().set(k, val);
                                }
                            }
                            Some(Value::Null)
                        }
                        b"offsetexists" => {
                            let key = args.get(1)?;
                            let ob = obj.borrow();
                            let arr = ob.get_property(b"__spl_array");
                            if let Value::Array(a) = arr {
                                let k = Self::value_to_array_key(key.clone());
                                Some(if a.borrow().get(&k).is_some() { Value::True } else { Value::False })
                            } else {
                                Some(Value::False)
                            }
                        }
                        b"offsetunset" => {
                            let key = args.get(1)?;
                            let ob = obj.borrow();
                            let arr = ob.get_property(b"__spl_array");
                            if let Value::Array(a) = arr {
                                let k = Self::value_to_array_key(key.clone());
                                a.borrow_mut().remove(&k);
                            }
                            Some(Value::Null)
                        }
                        b"add" => {
                            let index = args.get(1).cloned().unwrap_or(Value::Long(0)).to_long();
                            let val = args.get(2).cloned().unwrap_or(Value::Null);
                            let ob = obj.borrow();
                            let arr = ob.get_property(b"__spl_array");
                            if let Value::Array(a) = arr {
                                let k = ArrayKey::Int(index);
                                a.borrow_mut().set(k, val);
                            }
                            Some(Value::Null)
                        }
                        _ => None,
                    }
                }
                b"splfixedarray" => {
                    match method {
                        b"offsetget" => {
                            let key = args.get(1)?;
                            let ob = obj.borrow();
                            let arr = ob.get_property(b"__spl_array");
                            if let Value::Array(a) = arr {
                                let idx = key.to_long();
                                Some(a.borrow().get(&ArrayKey::Int(idx)).cloned().unwrap_or(Value::Null))
                            } else {
                                Some(Value::Null)
                            }
                        }
                        b"offsetset" => {
                            let key = args.get(1).cloned().unwrap_or(Value::Null);
                            let val = args.get(2).cloned().unwrap_or(Value::Null);
                            let ob = obj.borrow();
                            let arr = ob.get_property(b"__spl_array");
                            if let Value::Array(a) = arr {
                                let idx = key.to_long();
                                a.borrow_mut().set(ArrayKey::Int(idx), val);
                            }
                            Some(Value::Null)
                        }
                        b"offsetexists" => {
                            let key = args.get(1)?;
                            let ob = obj.borrow();
                            let arr = ob.get_property(b"__spl_array");
                            if let Value::Array(a) = arr {
                                let idx = key.to_long();
                                Some(if a.borrow().get(&ArrayKey::Int(idx)).is_some() { Value::True } else { Value::False })
                            } else {
                                Some(Value::False)
                            }
                        }
                        b"offsetunset" => {
                            let key = args.get(1)?;
                            let ob = obj.borrow();
                            let arr = ob.get_property(b"__spl_array");
                            if let Value::Array(a) = arr {
                                let idx = key.to_long();
                                a.borrow_mut().set(ArrayKey::Int(idx), Value::Null);
                            }
                            Some(Value::Null)
                        }
                        b"setsize" => {
                            let size = args.get(1).map(|v| v.to_long()).unwrap_or(0);
                            if size < 0 || size > 10_000_000 {
                                // Prevent massive allocation - just return null
                                return Some(Value::Null);
                            }
                            let mut ob = obj.borrow_mut();
                            ob.set_property(b"__spl_size".to_vec(), Value::Long(size));
                            let arr = ob.get_property(b"__spl_array");
                            if let Value::Array(a) = arr {
                                let mut a = a.borrow_mut();
                                let current = a.len();
                                if (size as usize) > current {
                                    for _ in current..(size as usize) {
                                        a.push(Value::Null);
                                    }
                                }
                            }
                            Some(Value::Null)
                        }
                        _ => None,
                    }
                }
                b"splobjectstorage" => {
                    match method {
                        b"attach" => {
                            let key_obj = args.get(1)?;
                            let data = args.get(2).cloned().unwrap_or(Value::Null);
                            if let Value::Object(key_o) = key_obj {
                                let hash = format!("{:016x}", key_o.borrow().object_id);
                                let ob = obj.borrow();
                                let arr = ob.get_property(b"__spl_array");
                                if let Value::Array(a) = arr {
                                    a.borrow_mut().set(
                                        ArrayKey::String(PhpString::from_string(hash)),
                                        data,
                                    );
                                }
                            }
                            Some(Value::Null)
                        }
                        b"detach" => {
                            let key_obj = args.get(1)?;
                            if let Value::Object(key_o) = key_obj {
                                let hash = format!("{:016x}", key_o.borrow().object_id);
                                let ob = obj.borrow();
                                let arr = ob.get_property(b"__spl_array");
                                if let Value::Array(a) = arr {
                                    a.borrow_mut().remove(&ArrayKey::String(PhpString::from_string(hash)));
                                }
                            }
                            Some(Value::Null)
                        }
                        b"contains" => {
                            let key_obj = args.get(1)?;
                            if let Value::Object(key_o) = key_obj {
                                let hash = format!("{:016x}", key_o.borrow().object_id);
                                let ob = obj.borrow();
                                let arr = ob.get_property(b"__spl_array");
                                if let Value::Array(a) = arr {
                                    Some(if a.borrow().get(&ArrayKey::String(PhpString::from_string(hash))).is_some() {
                                        Value::True
                                    } else {
                                        Value::False
                                    })
                                } else {
                                    Some(Value::False)
                                }
                            } else {
                                Some(Value::False)
                            }
                        }
                        _ => None,
                    }
                }
                b"splpriorityqueue" => {
                    match method {
                        b"insert" => {
                            let val = args.get(1).cloned().unwrap_or(Value::Null);
                            let _priority = args.get(2).cloned().unwrap_or(Value::Long(0));
                            let ob = obj.borrow();
                            let arr = ob.get_property(b"__spl_array");
                            if let Value::Array(a) = arr {
                                a.borrow_mut().push(val);
                            }
                            Some(Value::Null)
                        }
                        b"extract" => {
                            let ob = obj.borrow();
                            let arr = ob.get_property(b"__spl_array");
                            if let Value::Array(a) = arr {
                                let mut a = a.borrow_mut();
                                let len = a.len();
                                if len > 0 {
                                    let key = ArrayKey::Int((len - 1) as i64);
                                    let val = a.get(&key).cloned().unwrap_or(Value::Null);
                                    a.remove(&key);
                                    Some(val)
                                } else {
                                    Some(Value::Null)
                                }
                            } else {
                                Some(Value::Null)
                            }
                        }
                        _ => None,
                    }
                }
                b"datetime" | b"datetimeimmutable" => {
                    match method {
                        b"format" => {
                            let format_str = args.get(1).cloned().unwrap_or(Value::Null).to_php_string().to_string_lossy();
                            let ob = obj.borrow();
                            let timestamp = ob.get_property(b"timestamp").to_long();
                            // Call into the registered date_format function
                            // We implement a basic format here directly
                            let result = self.format_datetime_timestamp(&format_str, timestamp);
                            Some(Value::String(PhpString::from_string(result)))
                        }
                        b"gettimestamp" => {
                            let ob = obj.borrow();
                            Some(ob.get_property(b"timestamp"))
                        }
                        b"settimestamp" => {
                            let ts = args.get(1).cloned().unwrap_or(Value::Null).to_long();
                            obj.borrow_mut().set_property(b"timestamp".to_vec(), Value::Long(ts));
                            Some(this.clone())
                        }
                        _ => None,
                    }
                }
                _ => None,
            }
        } else {
            None
        }
    }

    /// Format a timestamp using PHP date format characters
    fn format_datetime_timestamp(&self, format: &str, secs: i64) -> String {
        let days_since_epoch = secs / 86400;
        let time_of_day = ((secs % 86400) + 86400) % 86400;
        let hours = time_of_day / 3600;
        let minutes = (time_of_day % 3600) / 60;
        let seconds = time_of_day % 60;

        // days_to_ymd helper
        fn days_to_ymd(days: i64) -> (i64, u32, u32) {
            let mut y = 1970i64;
            let mut remaining = days;
            if remaining >= 0 {
                loop {
                    let days_in_year = if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 { 366 } else { 365 };
                    if remaining < days_in_year { break; }
                    remaining -= days_in_year;
                    y += 1;
                }
            } else {
                loop {
                    y -= 1;
                    let days_in_year = if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 { 366 } else { 365 };
                    remaining += days_in_year;
                    if remaining >= 0 { break; }
                }
            }
            let is_leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
            let month_days = [31, if is_leap {29} else {28}, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
            let mut m = 0;
            for md in &month_days {
                if remaining < *md as i64 { break; }
                remaining -= *md as i64;
                m += 1;
            }
            (y, m + 1, remaining as u32 + 1)
        }

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
                b'A' => result.push_str(if hours >= 12 { "PM" } else { "AM" }),
                b'a' => result.push_str(if hours >= 12 { "pm" } else { "am" }),
                b'g' => {
                    let h12 = if hours == 0 { 12 } else if hours > 12 { hours - 12 } else { hours };
                    result.push_str(&format!("{}", h12));
                }
                b'h' => {
                    let h12 = if hours == 0 { 12 } else if hours > 12 { hours - 12 } else { hours };
                    result.push_str(&format!("{:02}", h12));
                }
                b'U' => result.push_str(&format!("{}", secs)),
                b'N' => {
                    let dow = ((days_since_epoch % 7 + 4 + 7) % 7) as i64;
                    result.push_str(&format!("{}", if dow == 0 { 7 } else { dow }));
                }
                b'w' => {
                    let dow = ((days_since_epoch % 7 + 4 + 7) % 7) as i64;
                    result.push_str(&format!("{}", dow));
                }
                b'D' => {
                    let dow = ((days_since_epoch % 7 + 4 + 7) % 7) as usize;
                    let names = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
                    result.push_str(names[dow]);
                }
                b'l' => {
                    let dow = ((days_since_epoch % 7 + 4 + 7) % 7) as usize;
                    let names = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
                    result.push_str(names[dow]);
                }
                b'F' => {
                    let names = ["January", "February", "March", "April", "May", "June",
                        "July", "August", "September", "October", "November", "December"];
                    result.push_str(names[(month - 1) as usize]);
                }
                b'M' => {
                    let names = ["Jan", "Feb", "Mar", "Apr", "May", "Jun",
                        "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
                    result.push_str(names[(month - 1) as usize]);
                }
                b't' => {
                    let is_leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
                    let month_days = [31u32, if is_leap {29} else {28}, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
                    result.push_str(&format!("{}", month_days[(month - 1) as usize]));
                }
                b'L' => {
                    let leap = if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 { 1 } else { 0 };
                    result.push_str(&format!("{}", leap));
                }
                b'e' | b'T' => result.push_str("UTC"),
                b'O' => result.push_str("+0000"),
                b'P' => result.push_str("+00:00"),
                b'p' => result.push_str("Z"),
                b'Z' => result.push_str("0"),
                b'c' => {
                    result.push_str(&format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}+00:00",
                        year, month, day, hours, minutes, seconds));
                }
                b'r' => {
                    let dow = ((days_since_epoch % 7 + 4 + 7) % 7) as usize;
                    let day_names = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
                    let month_names = ["Jan", "Feb", "Mar", "Apr", "May", "Jun",
                        "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
                    result.push_str(&format!("{}, {:02} {} {:04} {:02}:{:02}:{:02} +0000",
                        day_names[dow], day, month_names[(month-1) as usize], year, hours, minutes, seconds));
                }
                _ => result.push(c as char),
            }
            i += 1;
        }
        result
    }

    pub fn write_output(&mut self, data: &[u8]) {
        if let Some(buf) = self.ob_stack.last_mut() {
            buf.extend_from_slice(data);
        } else {
            self.output.extend_from_slice(data);
        }
    }

    /// Call a method on an object by looking up the class method and executing it
    fn call_object_method(
        &mut self,
        obj_val: &Value,
        method_name: &[u8],
        args: &[Value],
    ) -> Option<Value> {
        if let Value::Object(obj) = obj_val {
            let class_lower: Vec<u8> = obj.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
            if let Some(class_def) = self.classes.get(&class_lower) {
                if let Some(method) = class_def.get_method(method_name) {
                    let method_op = method.op_array.clone();
                    let mut fn_cvs = vec![Value::Undef; method_op.cv_names.len()];
                    // CV[0] = $this
                    if !fn_cvs.is_empty() {
                        fn_cvs[0] = obj_val.clone();
                    }
                    // Fill in method arguments
                    for (i, arg) in args.iter().enumerate() {
                        let cv_idx = i + 1; // offset by 1 for $this
                        if cv_idx < fn_cvs.len() {
                            fn_cvs[cv_idx] = arg.clone();
                        }
                    }
                    self.called_class_stack.push(class_lower.clone());
                    self.class_scope_stack.push(method.declaring_class.clone());
                    let result = self.execute_op_array(&method_op, fn_cvs).ok();
                    self.called_class_stack.pop();
                    self.class_scope_stack.pop();
                    return result;
                }
            }
        }
        None
    }

    /// Allocate a new object ID (for use by built-in functions that create objects)
    pub fn next_object_id(&mut self) -> u64 {
        let id = self.next_object_id;
        self.next_object_id += 1;
        id
    }

    /// Execute an op_array (main entry point)
    pub fn execute(&mut self, op_array: &OpArray) -> Result<Value, VmError> {
        self.is_global_scope = true;
        let cvs = vec![Value::Undef; op_array.cv_names.len()];
        let result = self.execute_op_array(op_array, cvs)?;

        // Call __destruct on all tracked objects in reverse creation order
        let destructibles = std::mem::take(&mut self.destructible_objects);
        for obj_val in destructibles.iter().rev() {
            if let Value::Object(obj_rc) = obj_val {
                let class_lower: Vec<u8> = obj_rc
                    .borrow()
                    .class_name
                    .iter()
                    .map(|b| b.to_ascii_lowercase())
                    .collect();
                if let Some(destruct_op) = self
                    .classes
                    .get(&class_lower)
                    .and_then(|c| c.get_method(b"__destruct"))
                    .map(|m| m.op_array.clone())
                {
                    let mut fn_cvs = vec![Value::Undef; destruct_op.cv_names.len()];
                    if !fn_cvs.is_empty() {
                        fn_cvs[0] = obj_val.clone(); // $this
                    }
                    self.called_class_stack.push(obj_rc.borrow().class_name.clone());
                    self.class_scope_stack.push(class_lower.clone());
                    let _ = self.execute_op_array(&destruct_op, fn_cvs);
                    self.class_scope_stack.pop();
                    self.called_class_stack.pop();
                }
            }
        }

        Ok(result)
    }

    /// Execute an op_array with pre-initialized CVs
    fn execute_op_array(
        &mut self,
        op_array: &OpArray,
        mut cvs: Vec<Value>,
    ) -> Result<Value, VmError> {
        self.call_depth += 1;
        if self.call_depth > 100 {
            self.call_depth -= 1;
            return Err(VmError {
                message: "Maximum call depth exceeded (possible infinite recursion)".into(),
                line: self.current_line,
            });
        }
        let result = self.execute_op_array_inner(op_array, cvs);
        self.call_depth -= 1;

        // Check return type if declared
        // Detect implicit return: the compiler emits `Return Null` with line=0 for implicit returns
        // If result is Ok(Null) or Ok(Undef), and there was no explicit return, it's "none returned"
        let implicit_return = matches!(result, Ok(Value::Undef) | Ok(Value::Null))
            && op_array.ops.last().map_or(false, |op| op.opcode == OpCode::Return && op.line == 0)
            && !op_array.ops.iter().any(|op| op.opcode == OpCode::Return && op.line != 0);
        let result = result.map(|v| if matches!(v, Value::Undef) { Value::Null } else { v });
        if let Ok(ref val) = result {
            if let Some(ref ret_type) = op_array.return_type {
                if !self.value_matches_return_type(val, ret_type) {
                    let raw_name = String::from_utf8_lossy(&op_array.name);
                    // Include class name in the function display name for methods
                    let func_name = if let Some(ref scope) = op_array.scope_class {
                        let class_display = self.classes.get(scope)
                            .map(|c| String::from_utf8_lossy(&c.name).to_string())
                            .unwrap_or_else(|| String::from_utf8_lossy(scope).to_string());
                        format!("{}::{}", class_display, raw_name)
                    } else {
                        raw_name.to_string()
                    };
                    // Use "none" when a function implicitly returns (no explicit return statement)
                    let actual_type = if implicit_return {
                        "none".to_string()
                    } else {
                        Self::value_type_name(val)
                    };
                    let expected_type = self.param_type_name(ret_type);
                    let msg = format!(
                        "{}(): Return value must be of type {}, {} returned",
                        func_name, expected_type, actual_type
                    );
                    let ret_line = if self.last_return_line > 0 {
                        self.last_return_line
                    } else if op_array.decl_line > 0 {
                        op_array.decl_line
                    } else {
                        self.current_line
                    };
                    let exc_val = self.create_exception(b"TypeError", &msg, ret_line);
                    self.current_exception = Some(exc_val);
                    return Err(VmError {
                        message: msg,
                        line: ret_line,
                    });
                }
            }
        }

        result
    }

    fn execute_op_array_inner(
        &mut self,
        op_array: &OpArray,
        mut cvs: Vec<Value>,
    ) -> Result<Value, VmError> {
        let mut ip: usize = 0;
        let temp_count = op_array.temp_count as usize;
        let mut tmps: Vec<Value> = vec![Value::Undef; temp_count];
        let mut foreach_positions: HashMap<u32, usize> = HashMap::new();
        // Snapshot of array keys at foreach init (for stable iteration)
        let mut foreach_keys: HashMap<u32, Vec<ArrayKey>> = HashMap::new();
        // Generator key storage for foreach (saved before advancing to next yield)
        let mut foreach_gen_keys: HashMap<u32, Value> = HashMap::new();
        // Maps CV index -> static var key (for saving back on write)
        let mut static_cv_keys: HashMap<u32, Vec<u8>> = HashMap::new();
        // Exception handler stack: (catch_target, finally_target, exception_tmp_idx)
        let mut exception_handlers: Vec<(u32, u32, u32)> = Vec::new();
        // Maps CV index -> global var name (for saving back on write)
        let mut global_cv_keys: HashMap<u32, Vec<u8>> = HashMap::new();

        // Initialize $GLOBALS superglobal if referenced
        for (i, cv_name) in op_array.cv_names.iter().enumerate() {
            if cv_name == b"GLOBALS" {
                // Build an array from current globals
                let mut globals_arr = PhpArray::new();
                for (k, v) in &self.globals {
                    globals_arr.set(
                        ArrayKey::String(PhpString::from_vec(k.clone())),
                        v.clone(),
                    );
                }
                if i < cvs.len() {
                    cvs[i] = Value::Array(Rc::new(RefCell::new(globals_arr)));
                }
                break;
            }
        }

        loop {
            if ip >= op_array.ops.len() {
                // Implicit return - use Undef to signal "none returned" for return type checks
                return Ok(Value::Undef);
            }

            let op = &op_array.ops[ip];
            ip += 1;
            self.current_line = op.line;

            match op.opcode {
                OpCode::Nop => {}

                OpCode::Echo => {
                    let val = self.read_operand_warn(&op.op1, &cvs, &tmps, &op_array.literals, op_array, op.line);
                    if matches!(val, Value::Array(_)) || matches!(&val, Value::Reference(r) if matches!(&*r.borrow(), Value::Array(_))) {
                        self.emit_warning_at("Array to string conversion", op.line);
                    }
                    let s = self.value_to_string(&val);
                    self.write_output(s.as_bytes());
                }

                OpCode::Print => {
                    let val = self.read_operand_warn(&op.op1, &cvs, &tmps, &op_array.literals, op_array, op.line);
                    if matches!(val, Value::Array(_)) || matches!(&val, Value::Reference(r) if matches!(&*r.borrow(), Value::Array(_))) {
                        self.emit_warning_at("Array to string conversion", op.line);
                    }
                    let s = self.value_to_string(&val);
                    self.write_output(s.as_bytes());
                    self.write_operand(
                        &op.result,
                        Value::Long(1),
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }

                OpCode::Assign => {
                    let val = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(&op.op1, val, &mut cvs, &mut tmps, &static_cv_keys);
                }

                OpCode::AssignRef => {
                    // Both op1 and op2 must be CVs. Make them share the same Reference.
                    if let (OperandType::Cv(target_idx), OperandType::Cv(value_idx)) =
                        (op.op1, op.op2)
                    {
                        let ti = target_idx as usize;
                        let vi = value_idx as usize;
                        // Get or create a reference cell for the value variable
                        let ref_cell = if let Value::Reference(r) = &cvs[vi] {
                            // Value is already a reference, share it
                            r.clone()
                        } else {
                            // Wrap the current value in a new reference
                            let r = Rc::new(RefCell::new(cvs[vi].clone()));
                            cvs[vi] = Value::Reference(r.clone());
                            r
                        };
                        // Point the target to the same reference
                        cvs[ti] = Value::Reference(ref_cell);
                    }
                }

                OpCode::Add => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    if let Some(err_msg) = Self::check_unsupported_operand_types(&a, &b, "+") {
                        let exc_val = self.throw_type_error(err_msg.clone());
                        self.current_exception = Some(exc_val);
                        if let Some((catch_target, _, _)) = exception_handlers.pop() {
                            ip = catch_target as usize;
                            continue;
                        } else {
                            return Err(VmError {
                                message: format!("Uncaught TypeError: {}", err_msg),
                                line: op.line,
                            });
                        }
                    }
                    self.write_operand(&op.result, a.add(&b), &mut cvs, &mut tmps, &static_cv_keys);
                }
                OpCode::Sub => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    if let Some(err_msg) = Self::check_unsupported_operand_types(&a, &b, "-") {
                        let exc_val = self.throw_type_error(err_msg.clone());
                        self.current_exception = Some(exc_val);
                        if let Some((catch_target, _, _)) = exception_handlers.pop() {
                            ip = catch_target as usize;
                            continue;
                        } else {
                            return Err(VmError {
                                message: format!("Uncaught TypeError: {}", err_msg),
                                line: op.line,
                            });
                        }
                    }
                    self.write_operand(&op.result, a.sub(&b), &mut cvs, &mut tmps, &static_cv_keys);
                }
                OpCode::Mul => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    if let Some(err_msg) = Self::check_unsupported_operand_types(&a, &b, "*") {
                        let exc_val = self.throw_type_error(err_msg.clone());
                        self.current_exception = Some(exc_val);
                        if let Some((catch_target, _, _)) = exception_handlers.pop() {
                            ip = catch_target as usize;
                            continue;
                        } else {
                            return Err(VmError {
                                message: format!("Uncaught TypeError: {}", err_msg),
                                line: op.line,
                            });
                        }
                    }
                    self.write_operand(&op.result, a.mul(&b), &mut cvs, &mut tmps, &static_cv_keys);
                }
                OpCode::Div => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    if let Some(err_msg) = Self::check_unsupported_operand_types(&a, &b, "/") {
                        let exc_val = self.throw_type_error(err_msg.clone());
                        self.current_exception = Some(exc_val);
                        if let Some((catch_target, _, _)) = exception_handlers.pop() {
                            ip = catch_target as usize;
                            continue;
                        } else {
                            return Err(VmError {
                                message: format!("Uncaught TypeError: {}", err_msg),
                                line: op.line,
                            });
                        }
                    }
                    match a.div(&b) {
                        Ok(result) => self.write_operand(
                            &op.result,
                            result,
                            &mut cvs,
                            &mut tmps,
                            &static_cv_keys,
                        ),
                        Err(msg) => {
                            // Throw DivisionByZeroError
                            let err_id = self.next_object_id;
                            self.next_object_id += 1;
                            let mut err_obj =
                                PhpObject::new(b"DivisionByZeroError".to_vec(), err_id);
                            err_obj.set_property(
                                b"message".to_vec(),
                                Value::String(PhpString::from_string(msg.to_string())),
                            );
                            err_obj.set_property(b"code".to_vec(), Value::Long(0));
                            let exc_val = Value::Object(Rc::new(RefCell::new(err_obj)));
                            self.current_exception = Some(exc_val);
                            if let Some((catch_target, _, _)) = exception_handlers.pop() {
                                ip = catch_target as usize;
                                continue;
                            } else {
                                return Err(VmError {
                                    message: format!("Uncaught DivisionByZeroError: {}", msg),
                                    line: op.line,
                                });
                            }
                        }
                    }
                }
                OpCode::Mod => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    if let Some(err_msg) = Self::check_unsupported_operand_types(&a, &b, "%") {
                        let exc_val = self.throw_type_error(err_msg.clone());
                        self.current_exception = Some(exc_val);
                        if let Some((catch_target, _, _)) = exception_handlers.pop() {
                            ip = catch_target as usize;
                            continue;
                        } else {
                            return Err(VmError {
                                message: format!("Uncaught TypeError: {}", err_msg),
                                line: op.line,
                            });
                        }
                    }
                    // Emit warning for leading-numeric strings
                    for val in [&a, &b] {
                        if let Value::String(s) = val.deref() {
                            if crate::value::parse_numeric_string(s.as_bytes()).is_none()
                                && !s.as_bytes().is_empty()
                            {
                                self.emit_warning_at("A non-numeric value encountered", op.line);
                            }
                        }
                    }
                    match a.modulo(&b) {
                        Ok(result) => self.write_operand(
                            &op.result,
                            result,
                            &mut cvs,
                            &mut tmps,
                            &static_cv_keys,
                        ),
                        Err(msg) => {
                            let err_id = self.next_object_id;
                            self.next_object_id += 1;
                            let mut err_obj =
                                PhpObject::new(b"DivisionByZeroError".to_vec(), err_id);
                            err_obj.set_property(
                                b"message".to_vec(),
                                Value::String(PhpString::from_string(msg.to_string())),
                            );
                            err_obj.set_property(b"code".to_vec(), Value::Long(0));
                            let exc_val = Value::Object(Rc::new(RefCell::new(err_obj)));
                            self.current_exception = Some(exc_val);
                            if let Some((catch_target, _, _)) = exception_handlers.pop() {
                                ip = catch_target as usize;
                                continue;
                            } else {
                                return Err(VmError {
                                    message: format!("Uncaught DivisionByZeroError: {}", msg),
                                    line: op.line,
                                });
                            }
                        }
                    }
                }
                OpCode::Pow => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    if let Some(err_msg) = Self::check_unsupported_operand_types(&a, &b, "**") {
                        let exc_val = self.throw_type_error(err_msg.clone());
                        self.current_exception = Some(exc_val);
                        if let Some((catch_target, _, _)) = exception_handlers.pop() {
                            ip = catch_target as usize;
                            continue;
                        } else {
                            return Err(VmError {
                                message: format!("Uncaught TypeError: {}", err_msg),
                                line: op.line,
                            });
                        }
                    }
                    self.write_operand(&op.result, a.pow(&b), &mut cvs, &mut tmps, &static_cv_keys);
                }
                OpCode::Concat => {
                    let a = self.read_operand_warn(&op.op1, &cvs, &tmps, &op_array.literals, op_array, op.line);
                    let b = self.read_operand_warn(&op.op2, &cvs, &tmps, &op_array.literals, op_array, op.line);
                    if matches!(&a, Value::Array(_)) {
                        self.emit_warning_at("Array to string conversion", op.line);
                    }
                    if matches!(&b, Value::Array(_)) {
                        self.emit_warning_at("Array to string conversion", op.line);
                    }
                    let a_str = self.value_to_string(&a);
                    let b_str = self.value_to_string(&b);
                    let mut result = a_str.as_bytes().to_vec();
                    result.extend_from_slice(b_str.as_bytes());
                    self.write_operand(
                        &op.result,
                        Value::String(PhpString::from_vec(result)),
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::Negate => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        a.negate(),
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }

                OpCode::BitwiseAnd => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    if let Some(err_msg) = Self::check_unsupported_operand_types(&a, &b, "&") {
                        let exc_val = self.throw_type_error(err_msg.clone());
                        self.current_exception = Some(exc_val);
                        if let Some((catch_target, _, _)) = exception_handlers.pop() {
                            ip = catch_target as usize;
                            continue;
                        } else {
                            return Err(VmError {
                                message: format!("Uncaught TypeError: {}", err_msg),
                                line: op.line,
                            });
                        }
                    }
                    let result = if matches!(a.deref(), Value::String(_)) && matches!(b.deref(), Value::String(_)) {
                        let sa = a.to_php_string();
                        let sb = b.to_php_string();
                        let ab = sa.as_bytes();
                        let bb = sb.as_bytes();
                        let len = ab.len().min(bb.len());
                        let res: Vec<u8> = (0..len).map(|i| ab[i] & bb[i]).collect();
                        Value::String(crate::string::PhpString::from_vec(res))
                    } else {
                        Value::Long(a.to_long() & b.to_long())
                    };
                    self.write_operand(
                        &op.result,
                        result,
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::BitwiseOr => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    if let Some(err_msg) = Self::check_unsupported_operand_types(&a, &b, "|") {
                        let exc_val = self.throw_type_error(err_msg.clone());
                        self.current_exception = Some(exc_val);
                        if let Some((catch_target, _, _)) = exception_handlers.pop() {
                            ip = catch_target as usize;
                            continue;
                        } else {
                            return Err(VmError {
                                message: format!("Uncaught TypeError: {}", err_msg),
                                line: op.line,
                            });
                        }
                    }
                    let result = if matches!(a.deref(), Value::String(_)) && matches!(b.deref(), Value::String(_)) {
                        let sa = a.to_php_string();
                        let sb = b.to_php_string();
                        let ab = sa.as_bytes();
                        let bb = sb.as_bytes();
                        let max_len = ab.len().max(bb.len());
                        let res: Vec<u8> = (0..max_len).map(|i| {
                            let a_byte = if i < ab.len() { ab[i] } else { 0 };
                            let b_byte = if i < bb.len() { bb[i] } else { 0 };
                            a_byte | b_byte
                        }).collect();
                        Value::String(crate::string::PhpString::from_vec(res))
                    } else {
                        Value::Long(a.to_long() | b.to_long())
                    };
                    self.write_operand(
                        &op.result,
                        result,
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::BitwiseXor => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    if let Some(err_msg) = Self::check_unsupported_operand_types(&a, &b, "^") {
                        let exc_val = self.throw_type_error(err_msg.clone());
                        self.current_exception = Some(exc_val);
                        if let Some((catch_target, _, _)) = exception_handlers.pop() {
                            ip = catch_target as usize;
                            continue;
                        } else {
                            return Err(VmError {
                                message: format!("Uncaught TypeError: {}", err_msg),
                                line: op.line,
                            });
                        }
                    }
                    let result = if matches!(a.deref(), Value::String(_)) && matches!(b.deref(), Value::String(_)) {
                        let sa = a.to_php_string();
                        let sb = b.to_php_string();
                        let ab = sa.as_bytes();
                        let bb = sb.as_bytes();
                        let len = ab.len().min(bb.len());
                        let res: Vec<u8> = (0..len).map(|i| ab[i] ^ bb[i]).collect();
                        Value::String(crate::string::PhpString::from_vec(res))
                    } else {
                        Value::Long(a.to_long() ^ b.to_long())
                    };
                    self.write_operand(
                        &op.result,
                        result,
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::BitwiseNot => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let result = if matches!(a.deref(), Value::String(_)) {
                        let sa = a.to_php_string();
                        let bytes = sa.as_bytes();
                        let res: Vec<u8> = bytes.iter().map(|b| !b).collect();
                        Value::String(crate::string::PhpString::from_vec(res))
                    } else {
                        Value::Long(!a.to_long())
                    };
                    self.write_operand(
                        &op.result,
                        result,
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::ShiftLeft => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        Value::Long(a.to_long().wrapping_shl(b.to_long() as u32)),
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::ShiftRight => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        Value::Long(a.to_long().wrapping_shr(b.to_long() as u32)),
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }

                OpCode::BooleanNot => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        if a.is_truthy() {
                            Value::False
                        } else {
                            Value::True
                        },
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }

                // Comparisons
                OpCode::Equal => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        if a.equals(&b) {
                            Value::True
                        } else {
                            Value::False
                        },
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::NotEqual => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        if a.equals(&b) {
                            Value::False
                        } else {
                            Value::True
                        },
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::Identical => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        if a.identical(&b) {
                            Value::True
                        } else {
                            Value::False
                        },
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::NotIdentical => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        if a.identical(&b) {
                            Value::False
                        } else {
                            Value::True
                        },
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::Less => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        if a.compare(&b) < 0 {
                            Value::True
                        } else {
                            Value::False
                        },
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::LessEqual => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        if a.compare(&b) <= 0 {
                            Value::True
                        } else {
                            Value::False
                        },
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::Greater => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        if a.compare(&b) > 0 {
                            Value::True
                        } else {
                            Value::False
                        },
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::GreaterEqual => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        if a.compare(&b) >= 0 {
                            Value::True
                        } else {
                            Value::False
                        },
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::Spaceship => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        Value::Long(a.compare(&b)),
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }

                // Compound assignments
                OpCode::AssignAdd => {
                    let cv_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    if let Some(err_msg) = Self::check_unsupported_operand_types(&cv_val, &rhs, "+")
                    {
                        let exc_val = self.throw_type_error(err_msg.clone());
                        self.current_exception = Some(exc_val);
                        if let Some((catch_target, _, _)) = exception_handlers.pop() {
                            ip = catch_target as usize;
                            continue;
                        } else {
                            return Err(VmError {
                                message: format!("Uncaught TypeError: {}", err_msg),
                                line: op.line,
                            });
                        }
                    }
                    self.write_operand(
                        &op.op1,
                        cv_val.add(&rhs),
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::AssignSub => {
                    let cv_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    if let Some(err_msg) = Self::check_unsupported_operand_types(&cv_val, &rhs, "-")
                    {
                        let exc_val = self.throw_type_error(err_msg.clone());
                        self.current_exception = Some(exc_val);
                        if let Some((catch_target, _, _)) = exception_handlers.pop() {
                            ip = catch_target as usize;
                            continue;
                        } else {
                            return Err(VmError {
                                message: format!("Uncaught TypeError: {}", err_msg),
                                line: op.line,
                            });
                        }
                    }
                    self.write_operand(
                        &op.op1,
                        cv_val.sub(&rhs),
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::AssignMul => {
                    let cv_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    if let Some(err_msg) = Self::check_unsupported_operand_types(&cv_val, &rhs, "*")
                    {
                        let exc_val = self.throw_type_error(err_msg.clone());
                        self.current_exception = Some(exc_val);
                        if let Some((catch_target, _, _)) = exception_handlers.pop() {
                            ip = catch_target as usize;
                            continue;
                        } else {
                            return Err(VmError {
                                message: format!("Uncaught TypeError: {}", err_msg),
                                line: op.line,
                            });
                        }
                    }
                    self.write_operand(
                        &op.op1,
                        cv_val.mul(&rhs),
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::AssignDiv => {
                    let cv_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    if let Some(err_msg) = Self::check_unsupported_operand_types(&cv_val, &rhs, "/")
                    {
                        let exc_val = self.throw_type_error(err_msg.clone());
                        self.current_exception = Some(exc_val);
                        if let Some((catch_target, _, _)) = exception_handlers.pop() {
                            ip = catch_target as usize;
                            continue;
                        } else {
                            return Err(VmError {
                                message: format!("Uncaught TypeError: {}", err_msg),
                                line: op.line,
                            });
                        }
                    }
                    match cv_val.div(&rhs) {
                        Ok(result) => self.write_operand(
                            &op.op1,
                            result,
                            &mut cvs,
                            &mut tmps,
                            &static_cv_keys,
                        ),
                        Err(msg) => {
                            return Err(VmError {
                                message: msg.to_string(),
                                line: op.line,
                            });
                        }
                    }
                }
                OpCode::AssignMod => {
                    let cv_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    if let Some(err_msg) = Self::check_unsupported_operand_types(&cv_val, &rhs, "%")
                    {
                        let exc_val = self.throw_type_error(err_msg.clone());
                        self.current_exception = Some(exc_val);
                        if let Some((catch_target, _, _)) = exception_handlers.pop() {
                            ip = catch_target as usize;
                            continue;
                        } else {
                            return Err(VmError {
                                message: format!("Uncaught TypeError: {}", err_msg),
                                line: op.line,
                            });
                        }
                    }
                    // Emit warning for leading-numeric strings
                    for val in [&cv_val, &rhs] {
                        if let Value::String(s) = val.deref() {
                            if crate::value::parse_numeric_string(s.as_bytes()).is_none()
                                && !s.as_bytes().is_empty()
                            {
                                self.emit_warning_at("A non-numeric value encountered", op.line);
                            }
                        }
                    }
                    match cv_val.modulo(&rhs) {
                        Ok(result) => self.write_operand(
                            &op.op1,
                            result,
                            &mut cvs,
                            &mut tmps,
                            &static_cv_keys,
                        ),
                        Err(msg) => {
                            return Err(VmError {
                                message: msg.to_string(),
                                line: op.line,
                            });
                        }
                    }
                }
                OpCode::AssignPow => {
                    let cv_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    if let Some(err_msg) =
                        Self::check_unsupported_operand_types(&cv_val, &rhs, "**")
                    {
                        let exc_val = self.throw_type_error(err_msg.clone());
                        self.current_exception = Some(exc_val);
                        if let Some((catch_target, _, _)) = exception_handlers.pop() {
                            ip = catch_target as usize;
                            continue;
                        } else {
                            return Err(VmError {
                                message: format!("Uncaught TypeError: {}", err_msg),
                                line: op.line,
                            });
                        }
                    }
                    self.write_operand(
                        &op.op1,
                        cv_val.pow(&rhs),
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::AssignConcat => {
                    let cv_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    if matches!(&cv_val, Value::Array(_)) {
                        self.emit_warning_at("Array to string conversion", op.line);
                    }
                    if matches!(&rhs, Value::Array(_)) {
                        self.emit_warning_at("Array to string conversion", op.line);
                    }
                    let a_str = self.value_to_string(&cv_val);
                    let b_str = self.value_to_string(&rhs);
                    let mut result = a_str.as_bytes().to_vec();
                    result.extend_from_slice(b_str.as_bytes());
                    self.write_operand(
                        &op.op1,
                        Value::String(PhpString::from_vec(result)),
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::AssignBitwiseAnd => {
                    let cv_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    if let Some(err_msg) = Self::check_unsupported_operand_types(&cv_val, &rhs, "&") {
                        let exc_val = self.throw_type_error(err_msg.clone());
                        self.current_exception = Some(exc_val);
                        if let Some((catch_target, _, _)) = exception_handlers.pop() {
                            ip = catch_target as usize;
                            continue;
                        } else {
                            return Err(VmError {
                                message: format!("Uncaught TypeError: {}", err_msg),
                                line: op.line,
                            });
                        }
                    }
                    // Emit warning for leading-numeric strings used in mixed-type bitwise ops
                    if !(matches!(cv_val.deref(), Value::String(_)) && matches!(rhs.deref(), Value::String(_))) {
                        for val in [&cv_val, &rhs] {
                            if let Value::String(s) = val.deref() {
                                if crate::value::parse_numeric_string(s.as_bytes()).is_none()
                                    && !s.as_bytes().is_empty()
                                {
                                    self.emit_warning_at("A non-numeric value encountered", op.line);
                                }
                            }
                        }
                    }
                    let result = if matches!(cv_val.deref(), Value::String(_)) && matches!(rhs.deref(), Value::String(_)) {
                        let sa = cv_val.to_php_string();
                        let sb = rhs.to_php_string();
                        let ab = sa.as_bytes();
                        let bb = sb.as_bytes();
                        let len = ab.len().min(bb.len());
                        let res: Vec<u8> = (0..len).map(|i| ab[i] & bb[i]).collect();
                        Value::String(crate::string::PhpString::from_vec(res))
                    } else {
                        Value::Long(cv_val.to_long() & rhs.to_long())
                    };
                    self.write_operand(
                        &op.op1,
                        result,
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::AssignBitwiseOr => {
                    let cv_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    if let Some(err_msg) = Self::check_unsupported_operand_types(&cv_val, &rhs, "|") {
                        let exc_val = self.throw_type_error(err_msg.clone());
                        self.current_exception = Some(exc_val);
                        if let Some((catch_target, _, _)) = exception_handlers.pop() {
                            ip = catch_target as usize;
                            continue;
                        } else {
                            return Err(VmError {
                                message: format!("Uncaught TypeError: {}", err_msg),
                                line: op.line,
                            });
                        }
                    }
                    // Emit warning for leading-numeric strings used in mixed-type bitwise ops
                    if !(matches!(cv_val.deref(), Value::String(_)) && matches!(rhs.deref(), Value::String(_))) {
                        for val in [&cv_val, &rhs] {
                            if let Value::String(s) = val.deref() {
                                if crate::value::parse_numeric_string(s.as_bytes()).is_none()
                                    && !s.as_bytes().is_empty()
                                {
                                    self.emit_warning_at("A non-numeric value encountered", op.line);
                                }
                            }
                        }
                    }
                    let result = if matches!(cv_val.deref(), Value::String(_)) && matches!(rhs.deref(), Value::String(_)) {
                        let sa = cv_val.to_php_string();
                        let sb = rhs.to_php_string();
                        let ab = sa.as_bytes();
                        let bb = sb.as_bytes();
                        let max_len = ab.len().max(bb.len());
                        let res: Vec<u8> = (0..max_len).map(|i| {
                            let a_byte = if i < ab.len() { ab[i] } else { 0 };
                            let b_byte = if i < bb.len() { bb[i] } else { 0 };
                            a_byte | b_byte
                        }).collect();
                        Value::String(crate::string::PhpString::from_vec(res))
                    } else {
                        Value::Long(cv_val.to_long() | rhs.to_long())
                    };
                    self.write_operand(
                        &op.op1,
                        result,
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::AssignBitwiseXor => {
                    let cv_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    if let Some(err_msg) = Self::check_unsupported_operand_types(&cv_val, &rhs, "^") {
                        let exc_val = self.throw_type_error(err_msg.clone());
                        self.current_exception = Some(exc_val);
                        if let Some((catch_target, _, _)) = exception_handlers.pop() {
                            ip = catch_target as usize;
                            continue;
                        } else {
                            return Err(VmError {
                                message: format!("Uncaught TypeError: {}", err_msg),
                                line: op.line,
                            });
                        }
                    }
                    // Emit warning for leading-numeric strings used in mixed-type bitwise ops
                    if !(matches!(cv_val.deref(), Value::String(_)) && matches!(rhs.deref(), Value::String(_))) {
                        for val in [&cv_val, &rhs] {
                            if let Value::String(s) = val.deref() {
                                if crate::value::parse_numeric_string(s.as_bytes()).is_none()
                                    && !s.as_bytes().is_empty()
                                {
                                    self.emit_warning_at("A non-numeric value encountered", op.line);
                                }
                            }
                        }
                    }
                    let result = if matches!(cv_val.deref(), Value::String(_)) && matches!(rhs.deref(), Value::String(_)) {
                        let sa = cv_val.to_php_string();
                        let sb = rhs.to_php_string();
                        let ab = sa.as_bytes();
                        let bb = sb.as_bytes();
                        let len = ab.len().min(bb.len());
                        let res: Vec<u8> = (0..len).map(|i| ab[i] ^ bb[i]).collect();
                        Value::String(crate::string::PhpString::from_vec(res))
                    } else {
                        Value::Long(cv_val.to_long() ^ rhs.to_long())
                    };
                    self.write_operand(
                        &op.op1,
                        result,
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::AssignShiftLeft => {
                    let cv_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(
                        &op.op1,
                        Value::Long(cv_val.to_long().wrapping_shl(rhs.to_long() as u32)),
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::AssignShiftRight => {
                    let cv_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(
                        &op.op1,
                        Value::Long(cv_val.to_long().wrapping_shr(rhs.to_long() as u32)),
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }

                // Increment / Decrement
                OpCode::PreIncrement => {
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let new_val = php_increment(&val);
                    self.write_operand(
                        &op.op1,
                        new_val.clone(),
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                    self.write_operand(&op.result, new_val, &mut cvs, &mut tmps, &static_cv_keys);
                }
                OpCode::PreDecrement => {
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let new_val = php_decrement(&val);
                    self.write_operand(
                        &op.op1,
                        new_val.clone(),
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                    self.write_operand(&op.result, new_val, &mut cvs, &mut tmps, &static_cv_keys);
                }
                OpCode::PostIncrement => {
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let new_val = php_increment(&val);
                    self.write_operand(&op.result, val, &mut cvs, &mut tmps, &static_cv_keys);
                    self.write_operand(&op.op1, new_val, &mut cvs, &mut tmps, &static_cv_keys);
                }
                OpCode::PostDecrement => {
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let new_val = php_decrement(&val);
                    self.write_operand(&op.result, val, &mut cvs, &mut tmps, &static_cv_keys);
                    self.write_operand(&op.op1, new_val, &mut cvs, &mut tmps, &static_cv_keys);
                }

                // Control flow
                OpCode::Jmp => {
                    if let OperandType::JmpTarget(target) = op.op1 {
                        ip = target as usize;
                    }
                }
                OpCode::JmpZ => {
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    if !val.is_truthy()
                        && let OperandType::JmpTarget(target) = op.op2
                    {
                        ip = target as usize;
                    }
                }
                OpCode::JmpNz => {
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    if val.is_truthy()
                        && let OperandType::JmpTarget(target) = op.op2
                    {
                        ip = target as usize;
                    }
                }

                // Function calls
                OpCode::InitFCall => {
                    let name_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    // Check if this is a closure array [name, use_val_1, use_val_2, ...]
                    if let Value::Array(arr) = &name_val {
                        let arr = arr.borrow();
                        let mut values: Vec<Value> = arr.values().cloned().collect();
                        if !values.is_empty() {
                            let name = values.remove(0).to_php_string();
                            // Remaining values are captured use vars - prepend as args
                            self.pending_calls.push(PendingCall {
                                name,
                                args: values, // use vars as initial args
                                named_args: Vec::new(),
                            });
                        } else {
                            self.pending_calls.push(PendingCall {
                                name: PhpString::empty(),
                                args: Vec::new(),
                                named_args: Vec::new(),
                            });
                        }
                    } else if let Value::Object(obj) = &name_val {
                        // Callable object: check for __invoke method
                        let class_lower: Vec<u8> = obj
                            .borrow()
                            .class_name
                            .iter()
                            .map(|b| b.to_ascii_lowercase())
                            .collect();
                        let class_name_orig = obj.borrow().class_name.clone();
                        let has_invoke = self
                            .classes
                            .get(&class_lower)
                            .map(|c| c.methods.contains_key(&b"__invoke".to_vec()))
                            .unwrap_or(false);
                        if has_invoke {
                            let mut func_name = class_name_orig;
                            func_name.extend_from_slice(b"::__invoke");
                            // Register the __invoke method in user_functions so DoFCall can find it
                            if let Some(class) = self.classes.get(&class_lower) {
                                if let Some(method) = class.get_method(b"__invoke") {
                                    self.user_functions.insert(
                                        func_name.to_ascii_lowercase(),
                                        method.op_array.clone(),
                                    );
                                }
                            }
                            self.pending_calls.push(PendingCall {
                                name: PhpString::from_vec(func_name),
                                args: vec![name_val.clone()], // $this
                                named_args: Vec::new(),
                            });
                        } else {
                            let name = name_val.to_php_string();
                            self.pending_calls.push(PendingCall {
                                name,
                                args: Vec::new(),
                                named_args: Vec::new(),
                            });
                        }
                    } else {
                        let name = name_val.to_php_string();
                        self.pending_calls.push(PendingCall {
                            name,
                            args: Vec::new(),
                            named_args: Vec::new(),
                        });
                    }
                }
                OpCode::SendVal => {
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    if let Some(call) = self.pending_calls.last_mut() {
                        call.args.push(val);
                    }
                }
                OpCode::SendNamedVal => {
                    let val = self.read_operand_warn(&op.op1, &cvs, &tmps, &op_array.literals, op_array, op.line);
                    let name_val = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    let name = name_val.to_php_string().as_bytes().to_vec();
                    if let Some(call) = self.pending_calls.last_mut() {
                        call.named_args.push((name, val));
                    }
                }
                OpCode::SendRef => {
                    // Send a value as a reference - for by-ref function arguments
                    // Creates a Reference wrapper so builtins can write back
                    let val = if let OperandType::Cv(idx) = &op.op1 {
                        let i = *idx as usize;
                        if let Some(Value::Reference(r)) = cvs.get(i) {
                            Value::Reference(r.clone())
                        } else {
                            // Create a new reference for the variable
                            let current = cvs.get(i).cloned().unwrap_or(Value::Null);
                            let r = Rc::new(RefCell::new(current));
                            cvs[i] = Value::Reference(r.clone());
                            Value::Reference(r)
                        }
                    } else {
                        self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals)
                    };
                    if let Some(call) = self.pending_calls.last_mut() {
                        call.args.push(val);
                    }
                }
                OpCode::SendUnpack => {
                    // Unpack an array/traversable into individual arguments
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    if let Some(call) = self.pending_calls.last_mut() {
                        // Helper: unpack from an iterator of (key, value) pairs
                        let unpack_result: Result<(), String> = match &val {
                            Value::Array(arr) => {
                                let arr = arr.borrow();
                                let mut had_named = false;
                                let mut result = Ok(());
                                for (k, v) in arr.iter() {
                                    match k {
                                        ArrayKey::String(s) => {
                                            had_named = true;
                                            // Check for duplicate with existing named args
                                            let name_bytes = s.as_bytes().to_vec();
                                            if call.named_args.iter().any(|(n, _)| *n == name_bytes) {
                                                result = Err(format!(
                                                    "Named parameter ${} overwrites previous argument",
                                                    String::from_utf8_lossy(&name_bytes)
                                                ));
                                                break;
                                            }
                                            call.named_args.push((name_bytes, v.clone()));
                                        }
                                        ArrayKey::Int(_) => {
                                            if had_named {
                                                result = Err("Cannot use positional argument after named argument during unpacking".into());
                                                break;
                                            }
                                            call.args.push(v.clone());
                                        }
                                    }
                                }
                                result
                            }
                            Value::Object(obj_rc) => {
                                // Check if it's a Traversable (e.g. ArrayIterator)
                                // Extract the underlying array from ArrayIterator-like objects
                                let obj = obj_rc.borrow();
                                let inner_arr = {
                                    let prop = obj.get_property(b"__spl_array");
                                    if matches!(prop, Value::Array(_)) { Some(prop) } else { None }
                                };
                                drop(obj);
                                if let Some(Value::Array(arr)) = inner_arr {
                                    let arr = arr.borrow();
                                    let mut had_named = false;
                                    let mut result = Ok(());
                                    for (k, v) in arr.iter() {
                                        match k {
                                            ArrayKey::String(s) => {
                                                had_named = true;
                                                let name_bytes = s.as_bytes().to_vec();
                                                if call.named_args.iter().any(|(n, _)| *n == name_bytes) {
                                                    result = Err(format!(
                                                        "Named parameter ${} overwrites previous argument",
                                                        String::from_utf8_lossy(&name_bytes)
                                                    ));
                                                    break;
                                                }
                                                call.named_args.push((name_bytes, v.clone()));
                                            }
                                            ArrayKey::Int(_) => {
                                                if had_named {
                                                    result = Err("Cannot use positional argument after named argument during unpacking".into());
                                                    break;
                                                }
                                                call.args.push(v.clone());
                                            }
                                        }
                                    }
                                    result
                                } else {
                                    call.args.push(val.clone());
                                    Ok(())
                                }
                            }
                            _ => {
                                // Non-array, just push as single arg
                                call.args.push(val.clone());
                                Ok(())
                            }
                        };
                        if let Err(err_msg) = unpack_result {
                            let exc_val = self.create_exception(b"Error", &err_msg, op.line);
                            self.current_exception = Some(exc_val);
                            if let Some((catch_target, _, _)) = exception_handlers.pop() {
                                ip = catch_target as usize;
                                continue;
                            } else {
                                return Err(VmError {
                                    message: format!("Uncaught Error: {}", err_msg),
                                    line: op.line,
                                });
                            }
                        }
                    }
                }
                OpCode::DoFCall => {
                    let mut call = self.pending_calls.pop().ok_or_else(|| VmError {
                        message: "no pending function call".into(),
                        line: op.line,
                    })?;

                    // Resolve "static::" in function names for late static binding
                    let call_name_bytes = call.name.as_bytes().to_vec();
                    if call_name_bytes.len() >= 8 {
                        let prefix: Vec<u8> = call_name_bytes[..8]
                            .iter()
                            .map(|b| b.to_ascii_lowercase())
                            .collect();
                        if prefix == b"static::" {
                            if let Some(called_class) = self.called_class_stack.last() {
                                let mut resolved_name = called_class.clone();
                                resolved_name.extend_from_slice(&call_name_bytes[6..]); // keep "::" and rest
                                call.name = PhpString::from_vec(resolved_name);
                            }
                        }
                    }

                    let mut func_name_lower: Vec<u8> = call
                        .name
                        .as_bytes()
                        .iter()
                        .map(|b| b.to_ascii_lowercase())
                        .collect();

                    // Namespace fallback: if a namespaced function isn't found,
                    // try the global (unqualified) name. This handles calls like
                    // strlen() from within namespace Foo (compiled as Foo\strlen).
                    if func_name_lower.contains(&b'\\') && !func_name_lower.contains(&b':') {
                        // Only for plain function calls (not Class::method)
                        let has_user = self.user_functions.contains_key(&func_name_lower);
                        let has_builtin = self.functions.contains_key(&func_name_lower);
                        if !has_user && !has_builtin {
                            // Try unqualified name (after last backslash)
                            if let Some(last_sep) = func_name_lower.iter().rposition(|&b| b == b'\\') {
                                let global_name = func_name_lower[last_sep + 1..].to_vec();
                                let global_has_user = self.user_functions.contains_key(&global_name);
                                let global_has_builtin = self.functions.contains_key(&global_name);
                                if global_has_user || global_has_builtin {
                                    // Also update call.name to preserve original case of global name
                                    let orig_bytes = call.name.as_bytes();
                                    if let Some(last_sep_orig) = orig_bytes.iter().rposition(|&b| b == b'\\') {
                                        call.name = PhpString::from_vec(orig_bytes[last_sep_orig + 1..].to_vec());
                                    }
                                    func_name_lower = global_name;
                                }
                            }
                        }
                    }

                    // Handle Closure static methods
                    if func_name_lower == b"closure::fromcallable" {
                        // Closure::fromCallable($callable) - return the callable as-is
                        // For string callables and closures, this is basically identity
                        let callable = call.args.first().cloned().unwrap_or(Value::Null);
                        self.write_operand(
                            &op.result,
                            callable,
                            &mut cvs,
                            &mut tmps,
                            &static_cv_keys,
                        );
                    } else if func_name_lower == b"closure::bind"
                        || func_name_lower == b"closure::bindto"
                    {
                        // Closure::bind($closure, $newThis, $newScope)
                        let closure = call.args.first().cloned().unwrap_or(Value::Null);
                        let new_this = call.args.get(1).cloned().unwrap_or(Value::Null);
                        let scope = call.args.get(2).cloned().unwrap_or(Value::String(PhpString::from_bytes(b"static")));
                        let scope_provided = call.args.len() >= 3;
                        let result = self.bind_closure(&closure, new_this, scope, scope_provided);
                        if self.current_exception.is_some() {
                            if let Some((catch_target, _, _)) = exception_handlers.pop() {
                                ip = catch_target as usize;
                                continue;
                            } else {
                                return Err(VmError {
                                    message: "Uncaught TypeError".into(),
                                    line: op.line,
                                });
                            }
                        }
                        self.write_operand(
                            &op.result,
                            result,
                            &mut cvs,
                            &mut tmps,
                            &static_cv_keys,
                        );
                    } else if func_name_lower == b"splfixedarray::fromarray" {
                        // SplFixedArray::fromArray($array, $preserveKeys = true)
                        let input = call.args.first().cloned().unwrap_or(Value::Null);
                        let _preserve_keys = call.args.get(1).cloned().unwrap_or(Value::True);
                        let oid = self.next_object_id();
                        let obj = PhpObject::new(b"SplFixedArray".to_vec(), oid);
                        let obj_rc = Rc::new(RefCell::new(obj));
                        if let Value::Array(src) = &input {
                            let src = src.borrow();
                            let len = src.len();
                            let mut arr = PhpArray::new();
                            for (i, (_, v)) in src.iter().enumerate() {
                                arr.set(ArrayKey::Int(i as i64), v.clone());
                            }
                            {
                                let mut obj_mut = obj_rc.borrow_mut();
                                obj_mut.set_property(
                                    b"__spl_array".to_vec(),
                                    Value::Array(Rc::new(RefCell::new(arr))),
                                );
                                obj_mut.set_property(b"__spl_size".to_vec(), Value::Long(len as i64));
                            }
                        } else {
                            let mut obj_mut = obj_rc.borrow_mut();
                            obj_mut.set_property(
                                b"__spl_array".to_vec(),
                                Value::Array(Rc::new(RefCell::new(PhpArray::new()))),
                            );
                            obj_mut.set_property(b"__spl_size".to_vec(), Value::Long(0));
                        }
                        self.write_operand(
                            &op.result,
                            Value::Object(obj_rc),
                            &mut cvs,
                            &mut tmps,
                            &static_cv_keys,
                        );
                    } else if func_name_lower.starts_with(b"__spl::") {
                        // SPL method call with args
                        let spl_path = &func_name_lower[7..]; // skip "__spl::"
                        if let Some(sep) = spl_path.iter().position(|&b| b == b':') {
                            if sep + 1 < spl_path.len() && spl_path[sep + 1] == b':' {
                                let spl_class = &spl_path[..sep];
                                let spl_method = &spl_path[sep + 2..];
                                let result = self
                                    .handle_spl_docall(spl_class, spl_method, &call.args)
                                    .unwrap_or(Value::Null);
                                self.write_operand(
                                    &op.result,
                                    result,
                                    &mut cvs,
                                    &mut tmps,
                                    &static_cv_keys,
                                );
                            }
                        }
                    } else if func_name_lower == b"__builtin_return" {
                        let result = call.args.first().cloned().unwrap_or(Value::Null);
                        self.write_operand(
                            &op.result,
                            result,
                            &mut cvs,
                            &mut tmps,
                            &static_cv_keys,
                        );
                    } else if func_name_lower == b"__closure_bindto" {
                        // $closure->bindTo($newThis, $scope)
                        // args[0] = closure value, args[1] = $newThis, args[2] = $scope
                        let closure = call.args.first().cloned().unwrap_or(Value::Null);
                        let new_this = call.args.get(1).cloned().unwrap_or(Value::Null);
                        let scope = call.args.get(2).cloned().unwrap_or(Value::String(PhpString::from_bytes(b"static")));
                        let scope_provided = call.args.len() >= 3;
                        let result = self.bind_closure(&closure, new_this, scope, scope_provided);
                        if self.current_exception.is_some() {
                            if let Some((catch_target, _, _)) = exception_handlers.pop() {
                                ip = catch_target as usize;
                                continue;
                            } else {
                                return Err(VmError {
                                    message: "Uncaught TypeError".into(),
                                    line: op.line,
                                });
                            }
                        }
                        self.write_operand(
                            &op.result,
                            result,
                            &mut cvs,
                            &mut tmps,
                            &static_cv_keys,
                        );
                    } else if func_name_lower == b"__closure_call" {
                        // $closure->call($newThis, ...$args)
                        // args[0] = closure value, args[1] = $newThis, args[2..] = extra args
                        let closure = call.args.first().cloned().unwrap_or(Value::Null);
                        let new_this = call.args.get(1).cloned().unwrap_or(Value::Null);
                        let extra_args: Vec<Value> = call.args.get(2..).map(|s| s.to_vec()).unwrap_or_default();

                        let scope = if let Value::Object(obj) = &new_this {
                            Value::Object(obj.clone())
                        } else {
                            Value::Null
                        };
                        let bound = self.bind_closure(&closure, new_this, scope, true);

                        // Now call the bound closure
                        if let Value::Array(arr) = &bound {
                            let arr_borrow = arr.borrow();
                            let mut values: Vec<Value> = arr_borrow.values().cloned().collect();
                            if !values.is_empty() {
                                let name = values.remove(0).to_php_string();
                                values.extend(extra_args);
                                drop(arr_borrow);
                                self.pending_calls.push(PendingCall {
                                    name,
                                    args: values,
                                    named_args: Vec::new(),
                                });
                                ip -= 1;
                                continue;
                            }
                        } else if let Value::String(name) = &bound {
                            let mut args = Vec::new();
                            args.extend(extra_args);
                            self.pending_calls.push(PendingCall {
                                name: name.clone(),
                                args,
                                named_args: Vec::new(),
                            });
                            ip -= 1;
                            continue;
                        }
                        self.write_operand(
                            &op.result,
                            Value::Null,
                            &mut cvs,
                            &mut tmps,
                            &static_cv_keys,
                        );
                    } else if func_name_lower == b"__generator_send" {
                        // Generator send() method: args[0] = generator, args[1] = sent value
                        if let Some(Value::Generator(gen_rc)) = call.args.first() {
                            let sent_value = call.args.get(1).cloned().unwrap_or(Value::Null);
                            let mut gen_borrow = gen_rc.borrow_mut();
                            gen_borrow.send_value = sent_value;
                            gen_borrow.write_send_value();
                            let _ = gen_borrow.resume(self);
                            let result = gen_borrow.current_value.clone();
                            drop(gen_borrow);
                            self.write_operand(
                                &op.result,
                                result,
                                &mut cvs,
                                &mut tmps,
                                &static_cv_keys,
                            );
                        } else {
                            self.write_operand(
                                &op.result,
                                Value::Null,
                                &mut cvs,
                                &mut tmps,
                                &static_cv_keys,
                            );
                        }
                    } else if let Some(func) = self.functions.get(&func_name_lower).copied() {
                        // Built-in function - resolve named args if present
                        if !call.named_args.is_empty() {
                            // Special functions that forward named args to their callbacks
                            let forwards_named = func_name_lower == b"call_user_func"
                                || func_name_lower == b"call_user_func_array"
                                || func_name_lower == b"forward_static_call"
                                || func_name_lower == b"forward_static_call_array";
                            if forwards_named {
                                // Store named args on VM for forwarding
                                self.pending_named_args = call.named_args.drain(..).collect();
                            } else if let Some(param_names) = self.builtin_param_names.get(&func_name_lower).cloned() {
                                let param_refs: Vec<&[u8]> = param_names.iter().map(|p| p.as_slice()).collect();
                                if let Err(err_msg) = call.resolve_named_args_builtin(&param_refs) {
                                    // For variadic builtins, use a different error format
                                    let (exc_class, final_msg) = if err_msg.starts_with("Unknown named parameter") {
                                        let display = call.name.to_string_lossy();
                                        (b"ArgumentCountError" as &[u8], format!("{}() does not accept unknown named parameters", display))
                                    } else {
                                        (b"Error" as &[u8], err_msg)
                                    };
                                    let exc_val = self.create_exception(exc_class, &final_msg, op.line);
                                    self.current_exception = Some(exc_val);
                                    if let Some((catch_target, _, _)) = exception_handlers.pop() {
                                        ip = catch_target as usize;
                                        continue;
                                    } else {
                                        return Err(VmError {
                                            message: format!("Uncaught {}: {}", String::from_utf8_lossy(exc_class), final_msg),
                                            line: op.line,
                                        });
                                    }
                                }
                            } else {
                                // No param names registered for this builtin - error on unknown named params
                                let first_name = String::from_utf8_lossy(&call.named_args[0].0).into_owned();
                                let err_msg = format!("Unknown named parameter ${}", first_name);
                                let exc_val = self.create_exception(b"Error", &err_msg, op.line);
                                self.current_exception = Some(exc_val);
                                if let Some((catch_target, _, _)) = exception_handlers.pop() {
                                    ip = catch_target as usize;
                                    continue;
                                } else {
                                    return Err(VmError {
                                        message: format!("Uncaught Error: {}", err_msg),
                                        line: op.line,
                                    });
                                }
                            }
                        }
                        match func(self, &call.args) {
                            Ok(result) => {
                                self.write_operand(
                                    &op.result,
                                    result,
                                    &mut cvs,
                                    &mut tmps,
                                    &static_cv_keys,
                                );
                            }
                            Err(e) => {
                                // Check if there's a pending exception to catch
                                if self.current_exception.is_some() {
                                    if let Some((catch_target, _, _)) = exception_handlers.pop() {
                                        ip = catch_target as usize;
                                        continue;
                                    }
                                }
                                return Err(VmError {
                                    message: e.message,
                                    line: op.line,
                                });
                            }
                        }
                    } else if let Some(user_fn) = self.user_functions.get(&func_name_lower).cloned()
                    {
                        // Check visibility for static method calls (ClassName::method)
                        if let Some(sep_pos) = func_name_lower.iter().position(|&b| b == b':') {
                            if func_name_lower.get(sep_pos + 1) == Some(&b':') {
                                let class_part_lower = &func_name_lower[..sep_pos];
                                let method_part_lower = &func_name_lower[sep_pos + 2..];
                                // Skip __call, __callstatic, __construct, and other magic methods
                                let is_magic = method_part_lower.starts_with(b"__");
                                if !is_magic {
                                    if let Some(class) = self.classes.get(class_part_lower) {
                                        if let Some(method) = class.get_method(method_part_lower) {
                                            if method.visibility != Visibility::Public {
                                                let method_vis = method.visibility;
                                                let method_declaring = method.declaring_class.clone();
                                                let method_display_name = String::from_utf8_lossy(&method.name).to_string();
                                                // Use the original case class name from call.name
                                                let orig_bytes = call.name.as_bytes();
                                                let class_display = &orig_bytes[..sep_pos];
                                                let caller_scope = self.current_class_scope();
                                                if let Some(err_msg) = self.check_visibility(
                                                    method_vis,
                                                    &method_declaring,
                                                    class_display,
                                                    &method_display_name,
                                                    false,
                                                    caller_scope.as_deref(),
                                                ) {
                                                    let err_id = self.next_object_id;
                                                    self.next_object_id += 1;
                                                    let mut err_obj = PhpObject::new(b"Error".to_vec(), err_id);
                                                    err_obj.set_property(
                                                        b"message".to_vec(),
                                                        Value::String(PhpString::from_string(err_msg.clone())),
                                                    );
                                                    err_obj.set_property(b"code".to_vec(), Value::Long(0));
                                                    self.current_exception =
                                                        Some(Value::Object(Rc::new(RefCell::new(err_obj))));
                                                    if let Some((catch_target, _, _)) = exception_handlers.pop() {
                                                        ip = catch_target as usize;
                                                        continue;
                                                    } else {
                                                        return Err(VmError {
                                                            message: format!("Uncaught Error: {}", err_msg),
                                                            line: op.line,
                                                        });
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // Resolve named arguments by reordering to match parameter positions
                        if !call.named_args.is_empty() {
                            // For __call/__callStatic, pack named args into the args for the $args array
                            let is_magic_call = func_name_lower.ends_with(b"::__call")
                                || func_name_lower.ends_with(b"::__callstatic");
                            if is_magic_call {
                                // Named args become part of the args array with string keys
                                // They'll be packed by the __call handler later
                                // For now, leave them in named_args and handle in the __call packing code
                            } else {
                                let implicit_args_count = if user_fn
                                    .cv_names
                                    .first()
                                    .map(|n| n.as_slice())
                                    == Some(b"this")
                                {
                                    1
                                } else {
                                    0
                                };
                                if let Err(err_msg) = call.resolve_named_args(&user_fn.cv_names, implicit_args_count, user_fn.variadic_param) {
                                    let exc_val = self.create_exception(b"Error", &err_msg, op.line);
                                    self.current_exception = Some(exc_val);
                                    if let Some((catch_target, _, _)) = exception_handlers.pop() {
                                        ip = catch_target as usize;
                                        continue;
                                    } else {
                                        return Err(VmError {
                                            message: format!("Uncaught Error: {}", err_msg),
                                            line: op.line,
                                        });
                                    }
                                }
                            }
                        }

                        // Push call stack frame early for proper error stack traces
                        let early_call_display = String::from_utf8_lossy(call.name.as_bytes()).into_owned();
                        let is_instance = user_fn.cv_names.first().map(|n| n.as_slice()) == Some(b"this");
                        // Capture args for stack trace (skip $this for methods)
                        let trace_args = {
                            let skip = if is_instance { 1 } else { 0 };
                            call.args.iter().skip(skip).cloned().collect::<Vec<_>>()
                        };
                        self.call_stack.push((early_call_display, self.current_file.clone(), op.line, trace_args, is_instance));

                        // Check argument count (too few arguments)
                        {
                            let implicit_args = if user_fn.cv_names.first().map(|n| n.as_slice())
                                == Some(b"this")
                            {
                                1
                            } else {
                                0
                            };
                            let provided = call.args.len() as u32 - implicit_args as u32;
                            let required = user_fn.required_param_count;
                            // Don't check for __call/__callStatic/constructors
                            let is_special = func_name_lower.ends_with(b"::__call")
                                || func_name_lower.ends_with(b"::__callstatic")
                                || func_name_lower.ends_with(b"::__construct");
                            if !is_special && provided < required {
                                let display_name = call.name.to_string_lossy();
                                // Extract just the function name for display
                                let fn_display = if let Some(pos) = display_name.rfind("::") {
                                    &display_name[pos + 2..]
                                } else {
                                    &display_name
                                };
                                let total_params = user_fn.param_count - implicit_args as u32;
                                let has_optional = total_params > required;
                                let qualifier = if has_optional { "at least" } else { "exactly" };
                                // Get filename
                                let file_display = &self.current_file;
                                let err_msg = format!(
                                    "Too few arguments to function {}(), {} passed in {} on line {} and {} {} expected",
                                    fn_display,
                                    provided,
                                    file_display,
                                    op.line,
                                    qualifier,
                                    required,
                                );
                                let exc_val = self.create_exception(b"ArgumentCountError", &err_msg, op.line);
                                self.current_exception = Some(exc_val);
                                if let Some((catch_target, _, _)) = exception_handlers.pop() {
                                    self.call_stack.pop(); // pop early-pushed frame
                                    ip = catch_target as usize;
                                    continue;
                                } else {
                                    return Err(VmError {
                                        message: format!("Uncaught ArgumentCountError: {}", err_msg),
                                        line: op.line,
                                    });
                                }
                            }
                            // Also check for Undef gaps in required parameters (named arg skips)
                            if !is_special && required > 0 {
                                let variadic_start = user_fn.variadic_param.map(|v| v as usize).unwrap_or(usize::MAX);
                                let mut missing_param_err: Option<String> = None;
                                for param_idx in implicit_args..(implicit_args + required as usize) {
                                    if param_idx >= variadic_start {
                                        break;
                                    }
                                    if param_idx < call.args.len() && matches!(call.args[param_idx], Value::Undef) {
                                        let display_name = call.name.to_string_lossy();
                                        let fn_display = if let Some(pos) = display_name.rfind("::") {
                                            &display_name[pos + 2..]
                                        } else {
                                            &display_name
                                        };
                                        let param_name = user_fn.cv_names.get(param_idx)
                                            .map(|n| String::from_utf8_lossy(n).into_owned())
                                            .unwrap_or_else(|| format!("param{}", param_idx));
                                        let arg_num = param_idx - implicit_args + 1;
                                        missing_param_err = Some(format!(
                                            "{}(): Argument #{} (${}) not passed", fn_display, arg_num, param_name
                                        ));
                                        break;
                                    }
                                }
                                if let Some(err_msg) = missing_param_err {
                                    let exc_val = self.create_exception(b"ArgumentCountError", &err_msg, op.line);
                                    self.current_exception = Some(exc_val);
                                    if let Some((catch_target, _, _)) = exception_handlers.pop() {
                                        self.call_stack.pop();
                                        ip = catch_target as usize;
                                        continue;
                                    } else {
                                        return Err(VmError {
                                            message: format!("Uncaught ArgumentCountError: {}", err_msg),
                                            line: op.line,
                                        });
                                    }
                                }
                            }
                        }

                        // Check parameter types before executing
                        if !user_fn.param_types.is_empty() {
                            // Push class scope temporarily for self/parent/static type checking
                            let temp_scope_pushed = if let Some(pos) = func_name_lower.iter().position(|&b| b == b':') {
                                if pos + 1 < func_name_lower.len() && func_name_lower[pos + 1] == b':' {
                                    let class_part_lower = &func_name_lower[..pos];
                                    let method_part_lower = &func_name_lower[pos + 2..];
                                    let defining_class = self.classes.get(class_part_lower)
                                        .and_then(|c| c.get_method(method_part_lower))
                                        .map(|m| m.declaring_class.clone())
                                        .unwrap_or_else(|| class_part_lower.to_vec());
                                    self.class_scope_stack.push(defining_class);
                                    true
                                } else { false }
                            } else if let Some(ref scope) = user_fn.scope_class {
                                self.class_scope_stack.push(scope.clone());
                                true
                            } else { false };
                            // Determine if this is a method call ($this is first implicit arg)
                            let implicit_args = if user_fn.cv_names.first().map(|n| n.as_slice())
                                == Some(b"this")
                            {
                                1
                            } else {
                                0
                            };
                            // Build display name from call.name
                            let display_name = call.name.to_string_lossy();
                            // For method calls like ClassName::method, format as ClassName::method
                            // For regular functions, just the function name
                            let param_err = self.check_param_types(
                                &user_fn,
                                &call.args,
                                &display_name,
                                implicit_args,
                                op.line,
                            );
                            // Pop temp scope
                            if temp_scope_pushed {
                                self.class_scope_stack.pop();
                            }
                            if let Some(err_msg) = param_err {
                                // Use the function definition line for the exception, not the call site
                                let def_line = if user_fn.decl_line > 0 {
                                    user_fn.decl_line
                                } else {
                                    user_fn.ops.first().map(|o| o.line).unwrap_or(op.line)
                                };
                                let exc_val = self.create_exception(b"TypeError", &err_msg, def_line);
                                self.current_exception = Some(exc_val);
                                if let Some((catch_target, _, _)) = exception_handlers.pop() {
                                    self.call_stack.pop(); // pop early-pushed frame
                                    ip = catch_target as usize;
                                    continue;
                                } else {
                                    return Err(VmError {
                                        message: format!("Uncaught TypeError: {}", err_msg),
                                        line: def_line,
                                    });
                                }
                            }
                        }

                        // Special handling for __call/__callStatic:
                        // Pack extra args into an array for the $args parameter
                        if func_name_lower.ends_with(b"::__call")
                            || func_name_lower.ends_with(b"::__callstatic")
                        {
                            // Args: [this, method_name, arg1, arg2, ...]
                            // Need: [this, method_name, [arg1, arg2, ..., name1 => val1, ...]]
                            let mut args_arr = crate::array::PhpArray::new();
                            if call.args.len() > 2 {
                                let extra_args: Vec<Value> = call.args.drain(2..).collect();
                                for arg in extra_args {
                                    args_arr.push(arg);
                                }
                            }
                            // Add named args with string keys
                            for (name, val) in call.named_args.drain(..) {
                                args_arr.set(
                                    ArrayKey::String(PhpString::from_vec(name)),
                                    val,
                                );
                            }
                            if !args_arr.is_empty() {
                                call.args
                                    .push(Value::Array(Rc::new(RefCell::new(args_arr))));
                            } else {
                                // No extra args - push empty array
                                call.args.push(Value::Array(Rc::new(RefCell::new(
                                    crate::array::PhpArray::new(),
                                ))));
                            }
                        }

                        // Check if this is a generator function
                        if user_fn.is_generator {
                            // Set up parameters as CVs
                            let mut func_cvs = vec![Value::Undef; user_fn.cv_names.len()];
                            if let Some(variadic_idx) = user_fn.variadic_param {
                                let vi = variadic_idx as usize;
                                for (i, arg) in call.args.iter().enumerate() {
                                    if i < vi && i < func_cvs.len() {
                                        func_cvs[i] = arg.clone();
                                    }
                                }
                                let mut variadic_arr = crate::array::PhpArray::new();
                                for arg in call.args.iter().skip(vi) {
                                    variadic_arr.push(arg.clone());
                                }
                                if vi < func_cvs.len() {
                                    func_cvs[vi] =
                                        Value::Array(Rc::new(RefCell::new(variadic_arr)));
                                }
                            } else {
                                for (i, arg) in call.args.iter().enumerate() {
                                    if i < func_cvs.len() {
                                        func_cvs[i] = arg.clone();
                                    }
                                }
                            }

                            // Create a generator instead of executing
                            let generator = crate::generator::PhpGenerator::new(user_fn, func_cvs);
                            let gen_rc = Rc::new(RefCell::new(generator));
                            self.write_operand(
                                &op.result,
                                Value::Generator(gen_rc),
                                &mut cvs,
                                &mut tmps,
                                &static_cv_keys,
                            );

                            // Reload globals
                            if self.is_global_scope {
                                for (i, name) in op_array.cv_names.iter().enumerate() {
                                    if let Some(val) = self.globals.get(name)
                                        && i < cvs.len()
                                    {
                                        cvs[i] = val.clone();
                                    }
                                }
                            }
                            continue;
                        }

                        // User-defined function - execute its op_array
                        let was_global = self.is_global_scope;
                        self.is_global_scope = false;

                        // Push called class for late static binding
                        let pushed_called_class =
                            if let Some(pos) = func_name_lower.iter().position(|&b| b == b':') {
                                if func_name_lower.get(pos + 1) == Some(&b':') {
                                    // Extract original-case class name from call.name
                                    let orig_bytes = call.name.as_bytes();
                                    let class_part = orig_bytes[..pos].to_vec();
                                    self.called_class_stack.push(class_part);

                                    // Push the defining class scope for visibility checks
                                    let class_part_lower = &func_name_lower[..pos];
                                    let method_part_lower = &func_name_lower[pos + 2..];
                                    let defining_class = self.classes.get(class_part_lower)
                                        .and_then(|c| c.get_method(method_part_lower))
                                        .map(|m| m.declaring_class.clone())
                                        .unwrap_or_else(|| class_part_lower.to_vec());
                                    self.class_scope_stack.push(defining_class);

                                    true
                                } else {
                                    false
                                }
                            } else {
                                false
                            };

                        // For closures/functions with a scope_class, push the scope for visibility
                        let pushed_scope_from_fn = if !pushed_called_class {
                            if let Some(ref scope) = user_fn.scope_class {
                                self.class_scope_stack.push(scope.clone());
                                true
                            } else {
                                false
                            }
                        } else {
                            false
                        };

                        // Save caller's globals before the call
                        if was_global {
                            for (i, cv) in cvs.iter().enumerate() {
                                if !matches!(cv, Value::Undef)
                                    && let Some(name) = op_array.cv_names.get(i)
                                {
                                    self.globals.insert(name.clone(), cv.clone());
                                }
                            }
                        }

                        // Set up parameters as CVs (handle variadic)
                        let mut func_cvs = vec![Value::Undef; user_fn.cv_names.len()];
                        if let Some(variadic_idx) = user_fn.variadic_param {
                            let vi = variadic_idx as usize;
                            // Regular params first
                            for (i, arg) in call.args.iter().enumerate() {
                                if i < vi && i < func_cvs.len() {
                                    func_cvs[i] = arg.clone();
                                }
                            }
                            // Pack remaining args into an array for the variadic param
                            let mut variadic_arr = crate::array::PhpArray::new();
                            for arg in call.args.iter().skip(vi) {
                                variadic_arr.push(arg.clone());
                            }
                            // Add extra named args with string keys
                            for (name, val) in call.named_args.drain(..) {
                                variadic_arr.set(
                                    ArrayKey::String(PhpString::from_vec(name)),
                                    val,
                                );
                            }
                            if vi < func_cvs.len() {
                                func_cvs[vi] = Value::Array(Rc::new(RefCell::new(variadic_arr)));
                            }
                        } else {
                            for (i, arg) in call.args.iter().enumerate() {
                                if i < func_cvs.len() {
                                    func_cvs[i] = arg.clone();
                                }
                            }
                        }

                        // Execute the function's op_array
                        // (call stack frame was already pushed before param checks)
                        let call_result = self.execute_op_array(&user_fn, func_cvs);

                        // Only pop call stack frame on success - keep it on error for stack trace
                        let call_failed = call_result.is_err();
                        if !call_failed {
                            self.call_stack.pop();
                        }

                        // Pop the called class stack and scope stack
                        if pushed_called_class {
                            self.called_class_stack.pop();
                            self.class_scope_stack.pop();
                        }
                        if pushed_scope_from_fn {
                            self.class_scope_stack.pop();
                        }

                        self.is_global_scope = was_global;

                        let result = match call_result {
                            Ok(v) => v,
                            Err(e) => {
                                // Check if we have an exception handler for uncaught exceptions
                                if let Some(exc) = self.current_exception.take() {
                                    if let Some((catch_target, _, _)) = exception_handlers.pop() {
                                        // Exception caught - pop call stack frame
                                        if call_failed { self.call_stack.pop(); }
                                        self.current_exception = Some(exc);
                                        ip = catch_target as usize;
                                        // Reload globals
                                        if was_global {
                                            for (i, name) in op_array.cv_names.iter().enumerate() {
                                                if let Some(val) = self.globals.get(name)
                                                    && i < cvs.len()
                                                {
                                                    cvs[i] = val.clone();
                                                }
                                            }
                                        }
                                        continue;
                                    } else {
                                        self.current_exception = Some(exc);
                                        return Err(e);
                                    }
                                }
                                // Check if there's a stored exception from the called function
                                if let Some(exc) = self.current_exception.take() {
                                    if !exception_handlers.is_empty() {
                                        // Exception caught - pop call stack frame
                                        if call_failed { self.call_stack.pop(); }
                                        self.current_exception = Some(exc);
                                        let (catch_target, _, _) =
                                            exception_handlers.pop().unwrap();
                                        ip = catch_target as usize;
                                        continue;
                                    } else {
                                        self.current_exception = Some(exc);
                                        return Err(e);
                                    }
                                }
                                return Err(e);
                            }
                        };

                        // Reload globals into caller's CVs after the function returns
                        if was_global {
                            for (i, name) in op_array.cv_names.iter().enumerate() {
                                if let Some(val) = self.globals.get(name)
                                    && i < cvs.len()
                                {
                                    cvs[i] = val.clone();
                                }
                            }
                        } else {
                            // In a non-global calling scope, reload any global-bound CVs
                            for (cv_idx, name) in &global_cv_keys {
                                if let Some(val) = self.globals.get(name)
                                    && (*cv_idx as usize) < cvs.len()
                                {
                                    cvs[*cv_idx as usize] = val.clone();
                                }
                            }
                        }

                        self.write_operand(
                            &op.result,
                            result,
                            &mut cvs,
                            &mut tmps,
                            &static_cv_keys,
                        );
                    } else {
                        // If it's a constructor call and the class has no __construct, silently succeed
                        let name_bytes = call.name.as_bytes();
                        if name_bytes.ends_with(b"::__construct") || name_bytes == b"__construct" {
                            // Check for unknown named parameters on classes without __construct
                            if !call.named_args.is_empty() {
                                let first_name = String::from_utf8_lossy(&call.named_args[0].0).into_owned();
                                let err_msg = format!("Unknown named parameter ${}", first_name);
                                let exc_val = self.create_exception(b"Error", &err_msg, op.line);
                                self.current_exception = Some(exc_val);
                                if let Some((catch_target, _, _)) = exception_handlers.pop() {
                                    ip = catch_target as usize;
                                    continue;
                                } else {
                                    return Err(VmError {
                                        message: format!("Uncaught Error: {}", err_msg),
                                        line: op.line,
                                    });
                                }
                            }
                            // For Exception-like classes, set message/code from args
                            if !call.args.is_empty() {
                                let this_idx = if call.args.len() > 1 { 0 } else { usize::MAX };
                                if this_idx == 0
                                    && let Value::Object(obj) = &call.args[0]
                                {
                                    let mut obj_mut = obj.borrow_mut();
                                    let class_lower: Vec<u8> = obj_mut.class_name.iter().map(|b| b.to_ascii_lowercase()).collect();

                                    // SPL class constructors
                                    match class_lower.as_slice() {
                                        b"arrayobject" | b"arrayiterator" | b"recursivearrayiterator" => {
                                            // __construct($array = [], $flags = 0, $iteratorClass = "ArrayIterator")
                                            if call.args.len() > 1 {
                                                if let Value::Array(_) = &call.args[1] {
                                                    obj_mut.set_property(b"__spl_array".to_vec(), call.args[1].clone());
                                                } else if let Value::Object(src) = &call.args[1] {
                                                    // Copy properties as array
                                                    let src = src.borrow();
                                                    let mut arr = PhpArray::new();
                                                    for (name, val) in &src.properties {
                                                        arr.set(ArrayKey::String(PhpString::from_vec(name.clone())), val.clone());
                                                    }
                                                    obj_mut.set_property(b"__spl_array".to_vec(), Value::Array(Rc::new(RefCell::new(arr))));
                                                }
                                            }
                                            if call.args.len() > 2 {
                                                obj_mut.set_property(b"__spl_flags".to_vec(), call.args[2].clone());
                                            }
                                        }
                                        b"splfixedarray" => {
                                            // __construct($size = 0)
                                            let size = if call.args.len() > 1 { call.args[1].to_long() } else { 0 };
                                            let mut arr = PhpArray::new();
                                            for i in 0..size {
                                                arr.push(Value::Null);
                                            }
                                            obj_mut.set_property(b"__spl_array".to_vec(), Value::Array(Rc::new(RefCell::new(arr))));
                                            obj_mut.set_property(b"__spl_size".to_vec(), Value::Long(size));
                                        }
                                        _ => {
                                            // Check if this is ErrorException
                                            // ErrorException($message, $code, $severity, $filename, $line, $previous)
                                            if class_lower.as_slice() == b"errorexception" {
                                                // ErrorException($message, $code, $severity, $filename, $line, $previous)
                                                if call.args.len() > 1 {
                                                    obj_mut.set_property(b"message".to_vec(), call.args[1].clone());
                                                }
                                                if call.args.len() > 2 {
                                                    obj_mut.set_property(b"code".to_vec(), call.args[2].clone());
                                                }
                                                if call.args.len() > 3 {
                                                    obj_mut.set_property(b"severity".to_vec(), call.args[3].clone());
                                                }
                                                // Only override file if non-null
                                                if call.args.len() > 4 && !matches!(call.args[4], Value::Null | Value::Undef) {
                                                    obj_mut.set_property(b"file".to_vec(), call.args[4].clone());
                                                    // When file is explicitly set, default line to 0
                                                    obj_mut.set_property(b"line".to_vec(), Value::Long(0));
                                                }
                                                // Override line if explicitly provided and non-null
                                                if call.args.len() > 5 && !matches!(call.args[5], Value::Null | Value::Undef) {
                                                    obj_mut.set_property(b"line".to_vec(), call.args[5].clone());
                                                }
                                                if call.args.len() > 6 {
                                                    obj_mut.set_property(b"previous".to_vec(), call.args[6].clone());
                                                }
                                            } else {
                                                // Default exception/error constructor: ($message, $code, $previous)
                                                if call.args.len() > 1 {
                                                    obj_mut.set_property(b"message".to_vec(), call.args[1].clone());
                                                }
                                                if call.args.len() > 2 {
                                                    obj_mut.set_property(b"code".to_vec(), call.args[2].clone());
                                                }
                                                if call.args.len() > 3 {
                                                    obj_mut.set_property(b"previous".to_vec(), call.args[3].clone());
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            self.write_operand(
                                &op.result,
                                Value::Null,
                                &mut cvs,
                                &mut tmps,
                                &static_cv_keys,
                            );
                        } else {
                            // Check for __callStatic/__call on ClassName::method calls
                            let name_bytes = call.name.as_bytes();
                            let mut handled = false;
                            if let Some(pos) = name_bytes.iter().position(|&b| b == b':') {
                                if pos + 1 < name_bytes.len() && name_bytes[pos + 1] == b':' {
                                    let class_part = &name_bytes[..pos];
                                    let method_part = &name_bytes[pos + 2..];
                                    let class_lower: Vec<u8> =
                                        class_part.iter().map(|b| b.to_ascii_lowercase()).collect();
                                    // Extract magic method op_arrays upfront to avoid borrow conflicts
                                    let magic_call = self.classes.get(&class_lower).and_then(|cd| {
                                        cd.get_method(b"__call").map(|m| (m.op_array.clone(), m.declaring_class.clone()))
                                    });
                                    let magic_callstatic = self.classes.get(&class_lower).and_then(|cd| {
                                        cd.get_method(b"__callstatic").map(|m| (m.op_array.clone(), m.declaring_class.clone()))
                                    });
                                    let class_found = self.classes.contains_key(&class_lower);
                                    if class_found {
                                        // Check if we're in an instance context (cvs[0] is $this object)
                                        // If so, try __call first (parent::method() from instance method)
                                        let in_instance_context = matches!(cvs.first(), Some(Value::Object(_)));
                                        if in_instance_context {
                                            if let Some((call_op, declaring_class)) = magic_call {
                                                let mut fn_cvs =
                                                    vec![Value::Undef; call_op.cv_names.len()];
                                                // __call($name, $arguments): CV[0]=$this, CV[1]=$name, CV[2]=$args
                                                if fn_cvs.len() > 0 {
                                                    fn_cvs[0] = cvs[0].clone(); // $this
                                                }
                                                if fn_cvs.len() > 1 {
                                                    fn_cvs[1] = Value::String(PhpString::from_vec(
                                                        method_part.to_vec(),
                                                    ));
                                                }
                                                if fn_cvs.len() > 2 {
                                                    let mut args_arr = crate::array::PhpArray::new();
                                                    for arg in &call.args {
                                                        args_arr.push(arg.clone());
                                                    }
                                                    fn_cvs[2] =
                                                        Value::Array(Rc::new(RefCell::new(args_arr)));
                                                }
                                                self.called_class_stack.push(class_part.to_vec());
                                                self.class_scope_stack.push(declaring_class);
                                                let result =
                                                    self.execute_op_array(&call_op, fn_cvs)?;
                                                self.called_class_stack.pop();
                                                self.class_scope_stack.pop();
                                                self.write_operand(
                                                    &op.result,
                                                    result,
                                                    &mut cvs,
                                                    &mut tmps,
                                                    &static_cv_keys,
                                                );
                                                handled = true;
                                            }
                                        }
                                        if !handled {
                                            if let Some((call_static_op, declaring_class)) = magic_callstatic {
                                                let mut fn_cvs =
                                                    vec![Value::Undef; call_static_op.cv_names.len()];
                                                // __callStatic($name, $arguments)
                                                if fn_cvs.len() > 0 {
                                                    fn_cvs[0] = Value::String(PhpString::from_vec(
                                                        method_part.to_vec(),
                                                    ));
                                                }
                                                if fn_cvs.len() > 1 {
                                                    let mut args_arr = crate::array::PhpArray::new();
                                                    for arg in &call.args {
                                                        args_arr.push(arg.clone());
                                                    }
                                                    fn_cvs[1] =
                                                        Value::Array(Rc::new(RefCell::new(args_arr)));
                                                }
                                                self.called_class_stack.push(class_part.to_vec());
                                                self.class_scope_stack.push(declaring_class);
                                                let result =
                                                    self.execute_op_array(&call_static_op, fn_cvs)?;
                                                self.called_class_stack.pop();
                                                self.class_scope_stack.pop();
                                                self.write_operand(
                                                    &op.result,
                                                    result,
                                                    &mut cvs,
                                                    &mut tmps,
                                                    &static_cv_keys,
                                                );
                                                handled = true;
                                            }
                                        }
                                    }
                                }
                            }
                            if !handled {
                                let func_display = call.name.to_string_lossy();
                                let err_msg = if func_display.contains("::") {
                                    format!("Call to undefined method {}()", func_display)
                                } else {
                                    format!("Call to undefined function {}()", func_display)
                                };
                                // Throw as Error exception for method calls
                                if func_display.contains("::") {
                                    let err_id = self.next_object_id;
                                    self.next_object_id += 1;
                                    let mut err_obj = PhpObject::new(b"Error".to_vec(), err_id);
                                    err_obj.set_property(
                                        b"message".to_vec(),
                                        Value::String(PhpString::from_string(err_msg.clone())),
                                    );
                                    err_obj.set_property(b"code".to_vec(), Value::Long(0));
                                    err_obj.set_property(b"file".to_vec(), Value::String(PhpString::from_bytes(b"")));
                                    err_obj.set_property(b"line".to_vec(), Value::Long(op.line as i64));
                                    self.current_exception = Some(Value::Object(Rc::new(RefCell::new(err_obj))));
                                    if let Some((catch_target, _, _)) = exception_handlers.last() {
                                        ip = *catch_target as usize;
                                        continue;
                                    }
                                }
                                return Err(VmError {
                                    message: err_msg,
                                    line: op.line,
                                });
                            }
                        }
                    }
                }

                OpCode::Return => {
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    self.last_return_line = op.line;
                    // Save global-bound CVs back to globals
                    for (cv_idx, name) in &global_cv_keys {
                        if let Some(cv_val) = cvs.get(*cv_idx as usize) {
                            self.globals.insert(name.clone(), cv_val.clone());
                        }
                    }
                    // In global scope, save all CVs as globals
                    if self.is_global_scope {
                        for (i, cv) in cvs.iter().enumerate() {
                            if !matches!(cv, Value::Undef)
                                && let Some(name) = op_array.cv_names.get(i)
                            {
                                self.globals.insert(name.clone(), cv.clone());
                            }
                        }
                    }
                    return Ok(val);
                }

                // Casts
                OpCode::CastInt => {
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    if let Value::Object(obj) = &val {
                        let class_name = {
                            let borrowed = obj.borrow();
                            String::from_utf8_lossy(&borrowed.class_name).into_owned()
                        };
                        self.emit_warning_at(
                            &format!(
                                "Object of class {} could not be converted to int",
                                class_name
                            ),
                            op.line,
                        );
                    }
                    self.write_operand(
                        &op.result,
                        Value::Long(val.to_long()),
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::CastFloat => {
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    if let Value::Object(obj) = &val {
                        let class_name = {
                            let borrowed = obj.borrow();
                            String::from_utf8_lossy(&borrowed.class_name).into_owned()
                        };
                        self.emit_warning_at(
                            &format!(
                                "Object of class {} could not be converted to float",
                                class_name
                            ),
                            op.line,
                        );
                    }
                    self.write_operand(
                        &op.result,
                        Value::Double(val.to_double()),
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::CastString => {
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let str_val = self.value_to_string(&val);
                    self.write_operand(
                        &op.result,
                        Value::String(str_val),
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::CastBool => {
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        if val.is_truthy() {
                            Value::True
                        } else {
                            Value::False
                        },
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::CastArray => {
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let arr = match val {
                        Value::Array(a) => a,
                        Value::Object(obj) => {
                            let ob = obj.borrow();
                            // Check for SPL array classes
                            let spl_arr = ob.get_property(b"__spl_array");
                            if let Value::Array(a) = spl_arr {
                                Rc::new(RefCell::new(a.borrow().clone()))
                            } else {
                                // Regular object: convert properties to array
                                let mut arr = PhpArray::new();
                                for (name, value) in &ob.properties {
                                    if name.starts_with(b"__spl_") {
                                        continue;
                                    }
                                    arr.set(
                                        ArrayKey::String(PhpString::from_vec(name.clone())),
                                        value.clone(),
                                    );
                                }
                                Rc::new(RefCell::new(arr))
                            }
                        }
                        Value::Null | Value::Undef => {
                            Rc::new(RefCell::new(PhpArray::new()))
                        }
                        other => {
                            let mut arr = PhpArray::new();
                            arr.push(other);
                            Rc::new(RefCell::new(arr))
                        }
                    };
                    self.write_operand(
                        &op.result,
                        Value::Array(arr),
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }

                OpCode::CastObject => {
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let obj = match val {
                        Value::Object(o) => Value::Object(o),
                        Value::Array(arr) => {
                            let arr_borrow = arr.borrow();
                            let obj_id = self.next_object_id;
                            self.next_object_id += 1;
                            let mut obj = PhpObject::new(b"stdClass".to_vec(), obj_id);
                            for (key, value) in arr_borrow.iter() {
                                let prop_name = match key {
                                    ArrayKey::String(s) => s.as_bytes().to_vec(),
                                    ArrayKey::Int(n) => n.to_string().into_bytes(),
                                };
                                obj.set_property(prop_name, value.clone());
                            }
                            Value::Object(Rc::new(RefCell::new(obj)))
                        }
                        Value::Null | Value::Undef => {
                            let obj_id = self.next_object_id;
                            self.next_object_id += 1;
                            Value::Object(Rc::new(RefCell::new(PhpObject::new(
                                b"stdClass".to_vec(),
                                obj_id,
                            ))))
                        }
                        other => {
                            let obj_id = self.next_object_id;
                            self.next_object_id += 1;
                            let mut obj = PhpObject::new(b"stdClass".to_vec(), obj_id);
                            obj.set_property(b"scalar".to_vec(), other);
                            Value::Object(Rc::new(RefCell::new(obj)))
                        }
                    };
                    self.write_operand(&op.result, obj, &mut cvs, &mut tmps, &static_cv_keys);
                }

                // Arrays
                OpCode::ArrayNew => {
                    let arr = Rc::new(RefCell::new(PhpArray::new()));
                    self.write_operand(
                        &op.result,
                        Value::Array(arr),
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::ArrayAppend => {
                    let arr_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let val = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    if let Value::Array(arr) = &arr_val {
                        arr.borrow_mut().push(val);
                    } else if let Value::Object(obj) = &arr_val {
                        // ArrayAccess: $obj[] = $val -> offsetSet(null, $val)
                        let class_lower: Vec<u8> = obj.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                        let spl_args = vec![arr_val.clone(), Value::Null, val.clone()];
                        if self.handle_spl_docall(&class_lower, b"offsetset", &spl_args).is_none() {
                            self.call_object_method(&arr_val, b"offsetset", &[Value::Null, val]);
                        }
                    }
                }
                OpCode::ArraySet => {
                    let arr_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let val = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    let key_val = self.read_operand(&op.result, &cvs, &tmps, &op_array.literals);
                    if let Value::Array(arr) = &arr_val {
                        let key = Self::value_to_array_key(key_val);
                        arr.borrow_mut().set(key, val);
                    } else if let Value::Object(obj) = &arr_val {
                        // ArrayAccess: $obj[$key] = $val -> offsetSet($key, $val)
                        let class_lower: Vec<u8> = obj.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                        let spl_args = vec![arr_val.clone(), key_val.clone(), val.clone()];
                        if self.handle_spl_docall(&class_lower, b"offsetset", &spl_args).is_none() {
                            self.call_object_method(&arr_val, b"offsetset", &[key_val, val]);
                        }
                    } else if matches!(arr_val, Value::String(_)) {
                        // String offset write: $str[n] = 'x'
                        let idx = key_val.to_long();
                        let replacement = val.to_php_string();
                        let replace_byte = replacement.as_bytes().first().copied().unwrap_or(b'\0');

                        // Read the CV directly to modify the string
                        if let OperandType::Cv(cv_idx) = &op.op1 {
                            let i = *cv_idx as usize;
                            if let Some(cv_val) = cvs.get_mut(i) {
                                let actual_val = match cv_val {
                                    Value::Reference(r) => {
                                        let mut inner = r.borrow_mut();
                                        if let Value::String(s) = &*inner {
                                            let mut bytes = s.as_bytes().to_vec();
                                            let actual_idx = if idx < 0 {
                                                let p = (-idx) as usize;
                                                if p <= bytes.len() { bytes.len() - p } else { continue; }
                                            } else {
                                                idx as usize
                                            };
                                            if actual_idx < bytes.len() {
                                                bytes[actual_idx] = replace_byte;
                                            } else {
                                                // Extend with spaces
                                                while bytes.len() < actual_idx {
                                                    bytes.push(b' ');
                                                }
                                                bytes.push(replace_byte);
                                            }
                                            *inner = Value::String(PhpString::from_vec(bytes));
                                        }
                                        continue;
                                    }
                                    Value::String(s) => {
                                        let mut bytes = s.as_bytes().to_vec();
                                        let actual_idx = if idx < 0 {
                                            let p = (-idx) as usize;
                                            if p <= bytes.len() { bytes.len() - p } else { continue; }
                                        } else {
                                            idx as usize
                                        };
                                        if actual_idx < bytes.len() {
                                            bytes[actual_idx] = replace_byte;
                                        } else {
                                            while bytes.len() < actual_idx {
                                                bytes.push(b' ');
                                            }
                                            bytes.push(replace_byte);
                                        }
                                        Some(Value::String(PhpString::from_vec(bytes)))
                                    }
                                    _ => None,
                                };
                                if let Some(new_val) = actual_val {
                                    *cv_val = new_val;
                                }
                            }
                        }
                    }
                }
                OpCode::ArrayGet => {
                    let arr_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let key_val = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    let result = if let Value::Array(arr) = &arr_val {
                        let key = Self::value_to_array_key(key_val.clone());
                        match arr.borrow().get(&key).cloned() {
                            Some(v) => v,
                            None => {
                                // Emit "Undefined array key" warning
                                match &key {
                                    crate::array::ArrayKey::Int(i) => {
                                        self.emit_warning_at(&format!("Undefined array key {}", i), op.line);
                                    }
                                    crate::array::ArrayKey::String(s) => {
                                        self.emit_warning_at(&format!("Undefined array key \"{}\"", s.to_string_lossy()), op.line);
                                    }
                                }
                                Value::Null
                            }
                        }
                    } else if let Value::String(s) = &arr_val {
                        // String offset access (supports negative indices)
                        let idx = key_val.to_long();
                        let bytes = s.as_bytes();
                        let actual_idx = if idx < 0 {
                            let positive = (-idx) as usize;
                            if positive <= bytes.len() {
                                bytes.len() - positive
                            } else {
                                usize::MAX // will fail bounds check
                            }
                        } else {
                            idx as usize
                        };
                        if actual_idx < bytes.len() {
                            Value::String(PhpString::from_bytes(&[bytes[actual_idx]]))
                        } else {
                            Value::String(PhpString::empty())
                        }
                    } else if let Value::Object(obj) = &arr_val {
                        // ArrayAccess: $obj[$key] -> offsetGet($key)
                        let class_lower: Vec<u8> = obj.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                        let args = vec![arr_val.clone(), key_val.clone()];
                        self.handle_spl_docall(&class_lower, b"offsetget", &args)
                            .unwrap_or_else(|| {
                                // Try user-defined offsetGet method
                                self.call_object_method(&arr_val, b"offsetget", &[key_val.clone()])
                                    .unwrap_or(Value::Null)
                            })
                    } else {
                        // Emit warning for non-array types
                        let type_name = match &arr_val {
                            Value::True | Value::False => "bool",
                            Value::Long(_) => "int",
                            Value::Double(_) => "float",
                            Value::Null | Value::Undef => "", // null silently returns null
                            _ => "",
                        };
                        if !type_name.is_empty() {
                            self.emit_warning_at(&format!("Cannot use {} as array", type_name), op.line);
                        }
                        Value::Null
                    };
                    self.write_operand(&op.result, result, &mut cvs, &mut tmps, &static_cv_keys);
                }

                OpCode::ForeachInit => {
                    let arr_val = self.read_operand_warn(&op.op1, &cvs, &tmps, &op_array.literals, &op_array, op.line);

                    if let Value::Generator(gen_rc) = &arr_val {
                        // For generators, advance to the first yield on init
                        let mut gen_borrow = gen_rc.borrow_mut();
                        let _ = gen_borrow.resume(self);
                        drop(gen_borrow);
                        // Store the generator in the iterator tmp slot
                        self.write_operand(
                            &op.result,
                            arr_val,
                            &mut cvs,
                            &mut tmps,
                            &static_cv_keys,
                        );
                    } else if let Value::Object(obj) = &arr_val {
                        let class_lower: Vec<u8> = obj.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();

                        // Check for SPL __spl_array first (built-in SPL classes)
                        let has_spl_array = {
                            let ob = obj.borrow();
                            matches!(ob.get_property(b"__spl_array"), Value::Array(_))
                        };

                        if has_spl_array && !self.class_implements_interface(&class_lower, b"iterator") {
                            // SPL class with internal array - iterate the array directly
                            let obj_borrow = obj.borrow();
                            let spl_arr = obj_borrow.get_property(b"__spl_array");
                            let arr_val = if let Value::Array(a) = spl_arr {
                                Value::Array(Rc::new(RefCell::new(a.borrow().clone())))
                            } else {
                                Value::Array(Rc::new(RefCell::new(PhpArray::new())))
                            };
                            drop(obj_borrow);
                            self.write_operand(
                                &op.result,
                                arr_val,
                                &mut cvs,
                                &mut tmps,
                                &static_cv_keys,
                            );
                        } else if self.class_implements_interface(&class_lower, b"iteratoraggregate") {
                            // IteratorAggregate: call getIterator() and iterate the result
                            let iter_obj = self.call_object_method(&arr_val, b"getIterator", &[]);
                            if let Some(iter_val) = iter_obj {
                                if let Value::Object(_) = &iter_val {
                                    // Call rewind() on the returned Iterator
                                    self.call_object_method(&iter_val, b"rewind", &[]);
                                    // Store the iterator object for ForeachNext
                                    self.write_operand(
                                        &op.result,
                                        iter_val,
                                        &mut cvs,
                                        &mut tmps,
                                        &static_cv_keys,
                                    );
                                } else {
                                    // getIterator() didn't return an object, store as-is
                                    self.write_operand(
                                        &op.result,
                                        iter_val,
                                        &mut cvs,
                                        &mut tmps,
                                        &static_cv_keys,
                                    );
                                }
                            } else {
                                self.write_operand(
                                    &op.result,
                                    Value::Null,
                                    &mut cvs,
                                    &mut tmps,
                                    &static_cv_keys,
                                );
                            }
                        } else if self.class_implements_interface(&class_lower, b"iterator") {
                            // Iterator: call rewind() and store the object
                            self.call_object_method(&arr_val, b"rewind", &[]);
                            self.write_operand(
                                &op.result,
                                arr_val,
                                &mut cvs,
                                &mut tmps,
                                &static_cv_keys,
                            );
                        } else {
                            // Plain object: convert properties to an array for iteration
                            let obj_borrow = obj.borrow();
                            let mut arr = PhpArray::new();
                            for (name, value) in &obj_borrow.properties {
                                arr.set(
                                    ArrayKey::String(PhpString::from_vec(name.clone())),
                                    value.clone(),
                                );
                            }
                            let arr_val = Value::Array(Rc::new(RefCell::new(arr)));
                            drop(obj_borrow);
                            self.write_operand(
                                &op.result,
                                arr_val,
                                &mut cvs,
                                &mut tmps,
                                &static_cv_keys,
                            );
                        }
                    } else {
                        // Emit warning for non-iterable values (null, bool, int, etc.)
                        // But NOT for closure strings (which represent objects in goro)
                        let is_closure_string = if let Value::String(s) = &arr_val {
                            let bytes = s.as_bytes();
                            bytes.starts_with(b"__closure_") || bytes.starts_with(b"__arrow_") || bytes.starts_with(b"__bound_closure_")
                        } else {
                            false
                        };
                        if !is_closure_string {
                            match &arr_val {
                                Value::Null | Value::False | Value::True | Value::Long(_) | Value::Double(_) | Value::String(_) => {
                                    let type_name = Vm::value_type_name(&arr_val);
                                    self.emit_warning_at(&format!(
                                        "foreach() argument must be of type array|object, {} given",
                                        type_name
                                    ), op.line);
                                }
                                _ => {}
                            }
                        }
                        // Store value in the iterator tmp slot
                        self.write_operand(
                            &op.result,
                            arr_val,
                            &mut cvs,
                            &mut tmps,
                            &static_cv_keys,
                        );
                    }
                    // Reset iteration position and snapshot keys
                    let iter_idx = match op.result {
                        OperandType::Tmp(idx) => idx,
                        _ => 0,
                    };
                    foreach_positions.insert(iter_idx, 0usize);
                    // Snapshot array keys for stable iteration
                    let stored = match &op.result {
                        OperandType::Tmp(idx) => tmps.get(*idx as usize).cloned(),
                        OperandType::Cv(idx) => cvs.get(*idx as usize).cloned(),
                        _ => None,
                    };
                    if let Some(Value::Array(arr)) = stored {
                        let keys: Vec<ArrayKey> = arr.borrow().keys().cloned().collect();
                        foreach_keys.insert(iter_idx, keys);
                    }
                }

                OpCode::ForeachNext => {
                    let iter_idx = match op.op1 {
                        OperandType::Tmp(idx) => idx,
                        _ => 0,
                    };
                    let pos = foreach_positions.get(&iter_idx).copied().unwrap_or(0);
                    let arr_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);

                    if let Value::Generator(gen_rc) = &arr_val {
                        // Generator iteration
                        // On first call (pos==0), ForeachInit already advanced to first yield.
                        // On subsequent calls (pos>0), advance to the next yield first.
                        if pos > 0 {
                            let mut gen_borrow = gen_rc.borrow_mut();
                            gen_borrow.write_send_value();
                            let _ = gen_borrow.resume(self);
                        }

                        let gen_borrow = gen_rc.borrow();
                        if gen_borrow.state == crate::generator::GeneratorState::Completed {
                            drop(gen_borrow);
                            // Done - jump to end
                            if let OperandType::JmpTarget(target) = op.op2 {
                                ip = target as usize;
                            }
                        } else {
                            // Get current value and key
                            let value = gen_borrow.current_value.clone();
                            let key = gen_borrow.current_key.clone();
                            drop(gen_borrow);

                            // Save the key for ForeachKey to read
                            foreach_gen_keys.insert(iter_idx, key);

                            self.write_operand(
                                &op.result,
                                value,
                                &mut cvs,
                                &mut tmps,
                                &static_cv_keys,
                            );
                            foreach_positions.insert(iter_idx, pos + 1);
                        }
                    } else if let Value::Object(_) = &arr_val {
                        // Iterator object: call valid(), current(), key(), next()
                        // On pos > 0, advance first via next()
                        if pos > 0 {
                            self.call_object_method(&arr_val, b"next", &[]);
                        }

                        // Check valid()
                        let valid = self
                            .call_object_method(&arr_val, b"valid", &[])
                            .unwrap_or(Value::False);
                        let is_valid = valid.is_truthy();

                        if !is_valid {
                            // Done - jump to end
                            if let OperandType::JmpTarget(target) = op.op2 {
                                ip = target as usize;
                            }
                        } else {
                            // Get current value
                            let value = self
                                .call_object_method(&arr_val, b"current", &[])
                                .unwrap_or(Value::Null);
                            // Get key
                            let key = self
                                .call_object_method(&arr_val, b"key", &[])
                                .unwrap_or(Value::Long(pos as i64));

                            // Save the key for ForeachKey to read
                            foreach_gen_keys.insert(iter_idx, key);

                            self.write_operand(
                                &op.result,
                                value,
                                &mut cvs,
                                &mut tmps,
                                &static_cv_keys,
                            );
                            foreach_positions.insert(iter_idx, pos + 1);
                        }
                    } else if let Value::Array(arr) = &arr_val {
                        // Use snapshotted keys for stable iteration
                        let keys = foreach_keys.get(&iter_idx);
                        let done = if let Some(keys) = keys {
                            // Find next valid key
                            let arr_borrow = arr.borrow();
                            let mut found = false;
                            let mut next_pos = pos;
                            while next_pos < keys.len() {
                                if let Some(value) = arr_borrow.get(&keys[next_pos]) {
                                    self.write_operand(
                                        &op.result,
                                        value.clone(),
                                        &mut cvs,
                                        &mut tmps,
                                        &static_cv_keys,
                                    );
                                    foreach_positions.insert(iter_idx, next_pos + 1);
                                    found = true;
                                    break;
                                }
                                next_pos += 1;
                            }
                            !found
                        } else {
                            // Fallback: direct position-based iteration
                            let arr_borrow = arr.borrow();
                            let entries: Vec<_> = arr_borrow.iter().collect();
                            if pos >= entries.len() {
                                true
                            } else {
                                let (_, value) = entries[pos];
                                self.write_operand(
                                    &op.result,
                                    value.clone(),
                                    &mut cvs,
                                    &mut tmps,
                                    &static_cv_keys,
                                );
                                foreach_positions.insert(iter_idx, pos + 1);
                                false
                            }
                        };
                        if done {
                            if let OperandType::JmpTarget(target) = op.op2 {
                                ip = target as usize;
                            }
                        }
                    } else {
                        // Not an array or generator - jump to end
                        if let OperandType::JmpTarget(target) = op.op2 {
                            ip = target as usize;
                        }
                    }
                }

                OpCode::ForeachKey => {
                    let iter_idx = match op.op1 {
                        OperandType::Tmp(idx) => idx,
                        _ => 0,
                    };
                    let pos = foreach_positions.get(&iter_idx).copied().unwrap_or(1);
                    let arr_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);

                    if let Value::Generator(_) = &arr_val {
                        // Use the key saved by ForeachNext before it advanced
                        let key_val = foreach_gen_keys
                            .get(&iter_idx)
                            .cloned()
                            .unwrap_or(Value::Long(0));
                        self.write_operand(
                            &op.result,
                            key_val,
                            &mut cvs,
                            &mut tmps,
                            &static_cv_keys,
                        );
                    } else if let Value::Object(_) = &arr_val {
                        // Iterator object: use the key saved by ForeachNext
                        let key_val = foreach_gen_keys
                            .get(&iter_idx)
                            .cloned()
                            .unwrap_or(Value::Long(0));
                        self.write_operand(
                            &op.result,
                            key_val,
                            &mut cvs,
                            &mut tmps,
                            &static_cv_keys,
                        );
                    } else if let Value::Array(_) = &arr_val {
                        // pos was already incremented by ForeachNext, so use pos - 1
                        let actual_pos = pos.saturating_sub(1);
                        let key_val = if let Some(keys) = foreach_keys.get(&iter_idx) {
                            if actual_pos < keys.len() {
                                match &keys[actual_pos] {
                                    ArrayKey::Int(n) => Value::Long(*n),
                                    ArrayKey::String(s) => Value::String(s.clone()),
                                }
                            } else {
                                Value::Null
                            }
                        } else {
                            Value::Long(actual_pos as i64)
                        };
                        self.write_operand(
                            &op.result,
                            key_val,
                            &mut cvs,
                            &mut tmps,
                            &static_cv_keys,
                        );
                    }
                }

                OpCode::BindGlobal => {
                    let name_val = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    let name = name_val.to_php_string().as_bytes().to_vec();
                    // Load the current global value into the CV
                    if let Some(val) = self.globals.get(&name) {
                        self.write_operand(
                            &op.op1,
                            val.clone(),
                            &mut cvs,
                            &mut tmps,
                            &static_cv_keys,
                        );
                    }
                    // Register this CV as global so writes are synced
                    if let OperandType::Cv(cv_idx) = op.op1 {
                        global_cv_keys.insert(cv_idx, name);
                    }
                }

                OpCode::StaticVarInit => {
                    let key_val = self.read_operand(&op.result, &cvs, &tmps, &op_array.literals);
                    let key = key_val.to_php_string().as_bytes().to_vec();

                    if let Some(existing) = self.static_vars.get(&key) {
                        self.write_operand(
                            &op.op1,
                            existing.clone(),
                            &mut cvs,
                            &mut tmps,
                            &static_cv_keys,
                        );
                    } else {
                        let default = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                        self.write_operand(
                            &op.op1,
                            default.clone(),
                            &mut cvs,
                            &mut tmps,
                            &static_cv_keys,
                        );
                        self.static_vars.insert(key.clone(), default);
                    }

                    // Register this CV as static so writes are persisted
                    if let OperandType::Cv(cv_idx) = op.op1 {
                        static_cv_keys.insert(cv_idx, key);
                    }
                }

                OpCode::DeclareFunction => {
                    let name_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let func_idx_val = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    let func_idx = func_idx_val.to_long() as usize;

                    if let Some(func_op_array) = op_array.child_functions.get(func_idx) {
                        let name = name_val.to_php_string();
                        self.register_user_function(name.as_bytes(), func_op_array.clone());
                    }
                }

                OpCode::TypeCheck => {
                    // instanceof check: op1 = value, op2 = class name
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let class_name = self
                        .read_operand(&op.op2, &cvs, &tmps, &op_array.literals)
                        .to_php_string();
                    let class_lower: Vec<u8> = class_name
                        .as_bytes()
                        .iter()
                        .map(|b| b.to_ascii_lowercase())
                        .collect();

                    let result = if class_lower == b"closure" {
                        // Special case: closures in goro-rs may be strings/arrays, not objects
                        match &val {
                            Value::String(s) => {
                                let bytes = s.as_bytes();
                                if bytes.starts_with(b"__closure_") || bytes.starts_with(b"__arrow_") || bytes.starts_with(b"__bound_closure_") {
                                    Value::True
                                } else {
                                    Value::False
                                }
                            }
                            Value::Array(arr) => {
                                let arr_borrow = arr.borrow();
                                if let Some(first) = arr_borrow.values().next() {
                                    if let Value::String(s) = first {
                                        let bytes = s.as_bytes();
                                        if bytes.starts_with(b"__closure_") || bytes.starts_with(b"__arrow_") || bytes.starts_with(b"__bound_closure_") {
                                            Value::True
                                        } else {
                                            Value::False
                                        }
                                    } else {
                                        Value::False
                                    }
                                } else {
                                    Value::False
                                }
                            }
                            Value::Object(obj) => {
                                let obj_borrow = obj.borrow();
                                if obj_borrow.class_name.eq_ignore_ascii_case(b"Closure") {
                                    Value::True
                                } else {
                                    Value::False
                                }
                            }
                            _ => Value::False,
                        }
                    } else if let Value::Generator(_) = &val {
                        // Generator instanceof check
                        if class_lower == b"generator"
                            || class_lower == b"iterator"
                            || class_lower == b"traversable"
                        {
                            Value::True
                        } else {
                            Value::False
                        }
                    } else if let Value::Object(obj) = &val {
                        let obj_borrow = obj.borrow();
                        let obj_class_lower: Vec<u8> = obj_borrow
                            .class_name
                            .iter()
                            .map(|b| b.to_ascii_lowercase())
                            .collect();

                        if obj_class_lower == class_lower {
                            Value::True
                        } else {
                            // Walk the class hierarchy (parents + interfaces)
                            let mut current = obj_class_lower.clone();
                            let mut found = false;
                            let mut visited = Vec::new();
                            loop {
                                if visited.contains(&current) {
                                    break;
                                }
                                visited.push(current.clone());
                                if let Some(class_def) = self.classes.get(&current) {
                                    // Check interfaces
                                    for iface in &class_def.interfaces {
                                        let iface_lower: Vec<u8> =
                                            iface.iter().map(|b| b.to_ascii_lowercase()).collect();
                                        if iface_lower == class_lower {
                                            found = true;
                                            break;
                                        }
                                    }
                                    if found {
                                        break;
                                    }
                                    // Check parent
                                    if let Some(ref parent) = class_def.parent {
                                        let parent_lower: Vec<u8> =
                                            parent.iter().map(|b| b.to_ascii_lowercase()).collect();
                                        if parent_lower == class_lower {
                                            found = true;
                                            break;
                                        }
                                        current = parent_lower;
                                    } else {
                                        break;
                                    }
                                } else {
                                    // Class not in table - check built-in hierarchy
                                    found = is_builtin_subclass(&current, &class_lower);
                                    break;
                                }
                            }
                            if found { Value::True } else { Value::False }
                        }
                    } else {
                        Value::False
                    };
                    self.write_operand(&op.result, result, &mut cvs, &mut tmps, &static_cv_keys);
                }

                OpCode::TryBegin => {
                    let catch_target = match op.op1 {
                        OperandType::JmpTarget(t) => t,
                        _ => 0,
                    };
                    let finally_target = match op.op2 {
                        OperandType::JmpTarget(t) => t,
                        _ => 0,
                    };
                    // Allocate a tmp to hold the caught exception
                    let exc_tmp = if temp_count > 0 {
                        (temp_count - 1) as u32
                    } else {
                        0
                    };
                    exception_handlers.push((catch_target, finally_target, exc_tmp));
                }

                OpCode::TryEnd => {
                    exception_handlers.pop();
                }

                OpCode::CatchException => {
                    // Store current exception into the CV
                    if let Some(exc) = self.current_exception.take() {
                        self.write_operand(&op.op1, exc, &mut cvs, &mut tmps, &static_cv_keys);
                    }
                }

                OpCode::Throw => {
                    let exc_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);

                    // Check that we're throwing an object
                    if !matches!(&exc_val, Value::Object(_)) {
                        let err_msg = "Can only throw objects".to_string();
                        let err_exc = self.create_exception(b"Error", &err_msg, op.line);
                        self.current_exception = Some(err_exc);
                        if let Some((catch_target, finally_target, _)) = exception_handlers.pop() {
                            if catch_target > 0 {
                                ip = catch_target as usize;
                            } else if finally_target > 0 {
                                ip = finally_target as usize;
                            }
                            continue;
                        }
                        return Err(VmError {
                            message: format!("Uncaught Error: {}", err_msg),
                            line: op.line,
                        });
                    }

                    // Check that the object implements Throwable
                    if let Value::Object(obj) = &exc_val {
                        let class_name = {
                            let obj_ref = obj.borrow();
                            obj_ref.class_name.clone()
                        };
                        let class_lower: Vec<u8> = class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                        let is_throwable = class_lower == b"exception"
                            || class_lower == b"error"
                            || is_builtin_subclass(&class_lower, b"exception")
                            || is_builtin_subclass(&class_lower, b"error")
                            || self.class_extends(&class_lower, b"exception")
                            || self.class_extends(&class_lower, b"error");
                        if !is_throwable {
                            let err_msg = "Cannot throw objects that do not implement Throwable".to_string();
                            let err_exc = self.create_exception(b"Error", &err_msg, op.line);
                            self.current_exception = Some(err_exc);
                            if let Some((catch_target, finally_target, _)) = exception_handlers.pop() {
                                if catch_target > 0 {
                                    ip = catch_target as usize;
                                } else if finally_target > 0 {
                                    ip = finally_target as usize;
                                }
                                continue;
                            }
                            return Err(VmError {
                                message: format!("Uncaught Error: {}", err_msg),
                                line: op.line,
                            });
                        }
                    }

                    if let Some((catch_target, finally_target, _exc_tmp)) = exception_handlers.pop()
                    {
                        // Store exception for the catch block to access
                        self.current_exception = Some(exc_val);

                        if catch_target > 0 {
                            // Has catch block - jump to it
                            // If there's also a finally, it will be reached via Jmp after catch
                            ip = catch_target as usize;
                        } else if finally_target > 0 {
                            // No catch, only finally - jump to finally
                            // Exception stays in current_exception for ReturnDeferred to re-throw
                            ip = finally_target as usize;
                        }
                    } else {
                        // No handler - check if there's a pending finally from an outer scope
                        // Store exception and return error
                        let msg = if let Value::Object(obj) = &exc_val {
                            let obj = obj.borrow();
                            let class = String::from_utf8_lossy(&obj.class_name).to_string();
                            let message = obj.get_property(b"message");
                            format!(
                                "Uncaught {}: {}",
                                class,
                                message.to_php_string().to_string_lossy()
                            )
                        } else {
                            format!(
                                "Uncaught exception: {}",
                                exc_val.to_php_string().to_string_lossy()
                            )
                        };
                        self.current_exception = Some(exc_val);
                        return Err(VmError {
                            message: msg,
                            line: op.line,
                        });
                    }
                }

                OpCode::StaticPropGet => {
                    let class_name_raw = self
                        .read_operand(&op.op1, &cvs, &tmps, &op_array.literals)
                        .to_php_string();
                    let prop_name = self
                        .read_operand(&op.op2, &cvs, &tmps, &op_array.literals)
                        .to_php_string();

                    // Resolve "static" for late static binding
                    let resolved_class = self
                        .resolve_static_class(class_name_raw.as_bytes())
                        .to_vec();

                    // Handle static::class - return the resolved class name
                    if prop_name.as_bytes() == b"class"
                        && class_name_raw.as_bytes().eq_ignore_ascii_case(b"static")
                    {
                        let val = Value::String(PhpString::from_vec(resolved_class));
                        self.write_operand(&op.result, val, &mut cvs, &mut tmps, &static_cv_keys);
                    } else {
                        let class_lower: Vec<u8> = resolved_class
                            .iter()
                            .map(|b| b.to_ascii_lowercase())
                            .collect();

                        let val = if let Some(class) = self.classes.get(&class_lower) {
                            // Check static properties first, then constants
                            class
                                .static_properties
                                .get(prop_name.as_bytes())
                                .cloned()
                                .or_else(|| class.constants.get(prop_name.as_bytes()).cloned())
                                .unwrap_or(Value::Null)
                        } else {
                            Value::Null
                        };
                        self.write_operand(&op.result, val, &mut cvs, &mut tmps, &static_cv_keys);
                    }
                }

                OpCode::StaticPropSet => {
                    let class_name_raw = self
                        .read_operand(&op.op1, &cvs, &tmps, &op_array.literals)
                        .to_php_string();
                    let value = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    let prop_name = self
                        .read_operand(&op.result, &cvs, &tmps, &op_array.literals)
                        .to_php_string();

                    // Resolve "static" for late static binding
                    let resolved_class = self
                        .resolve_static_class(class_name_raw.as_bytes())
                        .to_vec();
                    let class_lower: Vec<u8> = resolved_class
                        .iter()
                        .map(|b| b.to_ascii_lowercase())
                        .collect();

                    if let Some(class) = self.classes.get_mut(&class_lower) {
                        class
                            .static_properties
                            .insert(prop_name.as_bytes().to_vec(), value);
                    }
                }

                OpCode::ConstLookup => {
                    let name = self
                        .read_operand(&op.op1, &cvs, &tmps, &op_array.literals)
                        .to_php_string();
                    let name_bytes = name.as_bytes();
                    // Look up in constants table
                    let val = if let Some(v) = self.constants.get(name_bytes) {
                        Some(v.clone())
                    } else if name_bytes.contains(&b'\\') {
                        // Namespace fallback: try the unqualified (global) name
                        if let Some(last_sep) = name_bytes.iter().rposition(|&b| b == b'\\') {
                            let global_name = &name_bytes[last_sep + 1..];
                            self.constants.get(global_name).cloned()
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    if let Some(val) = val {
                        self.write_operand(&op.result, val, &mut cvs, &mut tmps, &static_cv_keys);
                    } else {
                        // PHP 8: undefined constants are fatal errors
                        let name_str = name.to_string_lossy();
                        let msg = format!("Undefined constant \"{}\"", name_str);
                        let exc = self.create_exception(b"Error", &msg, op.line);
                        self.current_exception = Some(exc);
                        if let Some((catch_target, finally_target, _)) = exception_handlers.last().copied() {
                            if catch_target > 0 {
                                exception_handlers.pop();
                                ip = catch_target as usize;
                            } else if finally_target > 0 {
                                ip = finally_target as usize;
                            }
                            continue;
                        }
                        return Err(VmError {
                            message: msg,
                            line: op.line,
                        });
                    }
                }

                OpCode::IncludeFile => {
                    let path_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let path_str = path_val.to_php_string().to_string_lossy();

                    // Try to read and execute the file
                    let path: &str = &path_str;
                    let result = match std::fs::read(path) {
                        Ok(source) => {
                            // Compile and execute
                            let mut lexer = goro_parser::Lexer::new(&source);
                            let tokens = lexer.tokenize();
                            let mut parser = goro_parser::Parser::new(tokens);
                            match parser.parse() {
                                Ok(program) => {
                                    let mut compiler = crate::compiler::Compiler::new();
                                    compiler.source_file = path.as_bytes().to_vec();
                                    match compiler.compile(&program) {
                                        Ok((inc_op_array, inc_classes)) => {
                                            // Register classes from included file
                                            for class in inc_classes {
                                                self.pending_classes.push(class);
                                            }
                                            // Execute included file's op_array
                                            let inc_cvs =
                                                vec![Value::Undef; inc_op_array.cv_names.len()];
                                            match self.execute_op_array(&inc_op_array, inc_cvs) {
                                                Ok(v) => v,
                                                Err(_) => Value::False,
                                            }
                                        }
                                        Err(_) => Value::False,
                                    }
                                }
                                Err(_) => Value::False,
                            }
                        }
                        Err(_) => Value::False,
                    };
                    self.write_operand(&op.result, result, &mut cvs, &mut tmps, &static_cv_keys);
                }

                OpCode::Eval => {
                    let code_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let code_str = code_val.to_php_string().to_string_lossy();

                    // PHP eval expects code without <?php opening tag typically,
                    // but if it has one, the lexer handles it.
                    // eval() wraps the code as if it were a file: <?php is optional
                    let source = if code_str.trim_start().starts_with("<?") {
                        code_str.as_bytes().to_vec()
                    } else {
                        format!("<?php {}", code_str).into_bytes()
                    };

                    let result = {
                        let mut lexer = goro_parser::Lexer::new(&source);
                        let tokens = lexer.tokenize();
                        let mut parser = goro_parser::Parser::new(tokens);
                        match parser.parse() {
                            Ok(program) => {
                                let mut compiler = crate::compiler::Compiler::new();
                                let eval_file = format!("{} : eval()'d code", self.current_file);
                                compiler.source_file = eval_file.into_bytes();
                                match compiler.compile(&program) {
                                    Ok((eval_op_array, eval_classes)) => {
                                        for class in eval_classes {
                                            self.pending_classes.push(class);
                                        }
                                        // Eval shares the calling scope's variables
                                        // Create new CVs but copy global scope if we're in global scope
                                        let eval_cvs = vec![Value::Undef; eval_op_array.cv_names.len()];
                                        match self.execute_op_array(&eval_op_array, eval_cvs) {
                                            Ok(v) => v,
                                            Err(e) => {
                                                // Propagate errors from eval
                                                if self.current_exception.is_some() {
                                                    if let Some((catch_target, _, _)) = exception_handlers.last() {
                                                        ip = *catch_target as usize;
                                                        continue;
                                                    }
                                                    return Err(e);
                                                }
                                                return Err(e);
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        // Compile error in eval - output as parse error
                                        let msg = format!("\nParse error: {} in {} : eval()'d code on line {}\n",
                                            e.message, self.current_file, e.line);
                                        self.output.extend_from_slice(msg.as_bytes());
                                        Value::False
                                    }
                                }
                            }
                            Err(e) => {
                                // PHP 7+: parse errors in eval throw ParseError exception
                                let msg = format!("syntax error, unexpected token \"{}\"",
                                    if e.message.contains("unexpected") {
                                        e.message.clone()
                                    } else {
                                        format!("syntax error in eval()'d code on line {}", e.span.line)
                                    });
                                let eval_file = format!("{} : eval()'d code", self.current_file);
                                let exc = self.create_exception(b"ParseError", &msg, e.span.line);
                                // Set file and line on the exception
                                if let Value::Object(ref obj) = exc {
                                    let mut ob = obj.borrow_mut();
                                    ob.set_property(b"file".to_vec(), Value::String(PhpString::from_string(eval_file)));
                                    ob.set_property(b"line".to_vec(), Value::Long(e.span.line as i64));
                                }
                                self.current_exception = Some(exc);
                                if let Some((catch_target, _, _)) = exception_handlers.last() {
                                    ip = *catch_target as usize;
                                    continue;
                                }
                                // No exception handler - output as parse error
                                let msg = format!("\nParse error: syntax error in {} : eval()'d code on line {}\n",
                                    self.current_file, e.span.line);
                                self.output.extend_from_slice(msg.as_bytes());
                                Value::False
                            }
                        }
                    };
                    self.write_operand(&op.result, result, &mut cvs, &mut tmps, &static_cv_keys);
                }

                OpCode::CloneObj => {
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let cloned = match &val {
                        Value::Object(obj) => {
                            let obj_borrow = obj.borrow();
                            let clone_id = self.next_object_id;
                            self.next_object_id += 1;
                            let mut new_obj =
                                PhpObject::new(obj_borrow.class_name.clone(), clone_id);
                            // Copy all properties
                            for (name, value) in &obj_borrow.properties {
                                new_obj.set_property(name.clone(), value.clone());
                            }
                            Value::Object(Rc::new(RefCell::new(new_obj)))
                        }
                        Value::Array(arr) => {
                            // Clone array
                            let cloned_arr = arr.borrow().clone();
                            Value::Array(Rc::new(RefCell::new(cloned_arr)))
                        }
                        _other => {
                            let type_name = Self::value_type_name(_other);
                            let msg = format!("clone(): Argument #1 ($object) must be of type object, {} given", type_name);
                            let exc_val = self.throw_type_error(msg.clone());
                            self.current_exception = Some(exc_val);
                            if let Some((catch_target, _, _)) = exception_handlers.pop() {
                                ip = catch_target as usize;
                                continue;
                            }
                            return Err(VmError {
                                message: msg,
                                line: op.line,
                            });
                        }
                    };
                    self.write_operand(&op.result, cloned, &mut cvs, &mut tmps, &static_cv_keys);
                }

                OpCode::GetClassName => {
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let class_name = match &val {
                        Value::Object(obj) => {
                            Value::String(PhpString::from_vec(obj.borrow().class_name.clone()))
                        }
                        _ => Value::String(PhpString::empty()),
                    };
                    self.write_operand(&op.result, class_name, &mut cvs, &mut tmps, &static_cv_keys);
                }

                OpCode::PropertyIsset => {
                    // Check if object property is set (with __isset support)
                    let obj_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let prop_name = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals).to_php_string();
                    let result = if let Value::Object(obj) = &obj_val {
                        let ob = obj.borrow();
                        if ob.has_property(prop_name.as_bytes()) {
                            let val = ob.get_property(prop_name.as_bytes());
                            if matches!(val, Value::Null) { Value::False } else { Value::True }
                        } else {
                            let class_lower: Vec<u8> = ob.class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                            drop(ob);
                            // Try __isset magic
                            let has_isset = self.classes.get(&class_lower)
                                .map(|c| c.methods.contains_key(&b"__isset".to_vec()))
                                .unwrap_or(false);
                            if has_isset && self.magic_depth < 5 {
                                self.magic_depth += 1;
                                let method = self.classes.get(&class_lower).unwrap().get_method(b"__isset").unwrap().op_array.clone();
                                let mut fn_cvs = vec![Value::Undef; method.cv_names.len()];
                                if !fn_cvs.is_empty() { fn_cvs[0] = obj_val.clone(); }
                                if fn_cvs.len() > 1 { fn_cvs[1] = Value::String(prop_name.clone()); }
                                let isset_result = self.execute_op_array(&method, fn_cvs).unwrap_or(Value::False);
                                self.magic_depth -= 1;
                                if isset_result.is_truthy() { Value::True } else { Value::False }
                            } else {
                                Value::False
                            }
                        }
                    } else {
                        Value::False
                    };
                    self.write_operand(&op.result, result, &mut cvs, &mut tmps, &static_cv_keys);
                }

                OpCode::ArrayUnset => {
                    // Remove element from array: op1 = array CV, op2 = key
                    let key_val = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    // Read the array reference directly
                    let arr_val = match &op.op1 {
                        OperandType::Cv(idx) => cvs.get(*idx as usize).cloned(),
                        OperandType::Tmp(idx) => tmps.get(*idx as usize).cloned(),
                        _ => None,
                    };
                    if let Some(Value::Array(arr)) = arr_val {
                        let key = Self::value_to_array_key(key_val.clone());
                        arr.borrow_mut().remove(&key);
                    } else if let Some(Value::Reference(r)) = arr_val {
                        let inner = r.borrow().clone();
                        if let Value::Array(arr) = inner {
                            let key = Self::value_to_array_key(key_val.clone());
                            arr.borrow_mut().remove(&key);
                        }
                    } else if let Some(Value::Object(obj)) = arr_val {
                        // ArrayAccess: unset($obj[$key]) -> offsetUnset($key)
                        let class_lower: Vec<u8> = obj.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                        let obj_val = Value::Object(obj);
                        let spl_args = vec![obj_val.clone(), key_val.clone()];
                        if self.handle_spl_docall(&class_lower, b"offsetunset", &spl_args).is_none() {
                            self.call_object_method(&obj_val, b"offsetunset", &[key_val]);
                        }
                    }
                }

                OpCode::PropertyUnset => {
                    // Remove property from object: op1 = object, op2 = property name
                    let obj_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let prop_name = self
                        .read_operand(&op.op2, &cvs, &tmps, &op_array.literals)
                        .to_php_string();
                    if let Value::Object(obj) = &obj_val {
                        let has_prop = obj.borrow().has_property(prop_name.as_bytes());
                        if has_prop {
                            obj.borrow_mut().properties
                                .retain(|(name, _)| name != prop_name.as_bytes());
                        } else {
                            // Try __unset magic method
                            let class_lower: Vec<u8> = obj.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                            let has_unset = self.classes.get(&class_lower)
                                .map(|c| c.methods.contains_key(&b"__unset".to_vec()))
                                .unwrap_or(false);
                            if has_unset && self.magic_depth < 5 {
                                self.magic_depth += 1;
                                let method = self.classes.get(&class_lower).unwrap().get_method(b"__unset").unwrap().op_array.clone();
                                let mut fn_cvs = vec![Value::Undef; method.cv_names.len()];
                                if !fn_cvs.is_empty() { fn_cvs[0] = obj_val.clone(); }
                                if fn_cvs.len() > 1 { fn_cvs[1] = Value::String(prop_name.clone()); }
                                let _ = self.execute_op_array(&method, fn_cvs);
                                self.magic_depth -= 1;
                            }
                        }
                    }
                }

                OpCode::VarVarGet => {
                    let name_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let name = name_val.to_php_string();
                    let name_bytes = name.as_bytes();
                    let mut found = Value::Null;
                    for (i, cv_name) in op_array.cv_names.iter().enumerate() {
                        if cv_name == name_bytes {
                            if let Some(val) = cvs.get(i) {
                                found = val.deref();
                            }
                            break;
                        }
                    }
                    if matches!(found, Value::Null | Value::Undef) {
                        if let Some(val) = self.globals.get(name_bytes) {
                            found = val.clone();
                        }
                    }
                    self.write_operand(&op.result, found, &mut cvs, &mut tmps, &static_cv_keys);
                }

                OpCode::VarVarSet => {
                    let name_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let value = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    let name = name_val.to_php_string();
                    let name_bytes = name.as_bytes().to_vec();
                    let mut wrote = false;
                    for (i, cv_name) in op_array.cv_names.iter().enumerate() {
                        if *cv_name == name_bytes {
                            if let Some(slot) = cvs.get_mut(i) {
                                *slot = value.clone();
                                wrote = true;
                            }
                            break;
                        }
                    }
                    if !wrote {
                        self.globals.insert(name_bytes, value);
                    }
                }

                OpCode::SaveReturn => {
                    // Save return value for deferred return (finally blocks)
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    self.pending_return = Some(val);
                }

                OpCode::ReturnDeferred => {
                    // If there's a pending return, execute it now
                    if let Some(val) = self.pending_return.take() {
                        return Ok(val);
                    }
                    // If there's a pending exception, re-throw it
                    if self.current_exception.is_some() {
                        // Try to find an outer exception handler
                        if let Some((catch_target, finally_target, _exc_tmp)) = exception_handlers.pop() {
                            if catch_target > 0 {
                                ip = catch_target as usize;
                                continue;
                            } else if finally_target > 0 {
                                ip = finally_target as usize;
                                continue;
                            }
                        }
                        // No outer handler - uncaught exception
                        let exc = self.current_exception.as_ref().unwrap();
                        let msg = if let Value::Object(obj) = exc {
                            let obj = obj.borrow();
                            let class = String::from_utf8_lossy(&obj.class_name).to_string();
                            let message = obj.get_property(b"message");
                            format!(
                                "Uncaught {}: {}",
                                class,
                                message.to_php_string().to_string_lossy()
                            )
                        } else {
                            "Uncaught exception".to_string()
                        };
                        return Err(VmError {
                            message: msg,
                            line: op.line,
                        });
                    }
                    // Otherwise continue (no pending return or exception)
                }

                OpCode::ArrayAppendRef => {
                    // Append to array keeping Reference wrapper (for closure by-ref capture)
                    let arr_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let raw_val = Self::read_operand_raw(&cvs, &op.op2);
                    if let Value::Array(arr) = arr_val {
                        arr.borrow_mut().push(raw_val);
                    }
                }

                OpCode::MakeRef => {
                    // Convert a CV's value to a Reference if not already one
                    if let OperandType::Cv(idx) = &op.op1 {
                        if let Some(slot) = cvs.get_mut(*idx as usize) {
                            if !matches!(slot, Value::Reference(_)) {
                                let current = std::mem::replace(slot, Value::Null);
                                *slot = Value::Reference(Rc::new(RefCell::new(current)));
                            }
                        }
                    }
                }

                OpCode::IssetCheck => {
                    // isset() check: True if value is not Null and not Undef
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let result = match val {
                        Value::Null | Value::Undef => Value::False,
                        _ => Value::True,
                    };
                    self.write_operand(&op.result, result, &mut cvs, &mut tmps, &static_cv_keys);
                }

                OpCode::ErrorSuppress => {
                    self.error_reporting_stack.push(self.error_reporting);
                    self.error_reporting = 0;
                }

                OpCode::ErrorRestore => {
                    if let Some(saved) = self.error_reporting_stack.pop() {
                        self.error_reporting = saved;
                    }
                }

                OpCode::ArraySpread => {
                    // Spread source array elements into target array
                    let target_val = match &op.op1 {
                        OperandType::Tmp(idx) => tmps.get(*idx as usize).cloned(),
                        OperandType::Cv(idx) => cvs.get(*idx as usize).cloned(),
                        _ => None,
                    };
                    let source = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    if let (Some(Value::Array(target)), Value::Array(source_arr)) =
                        (target_val, source)
                    {
                        let source_borrow = source_arr.borrow();
                        let mut target_borrow = target.borrow_mut();
                        for (key, val) in source_borrow.iter() {
                            match key {
                                ArrayKey::Int(_) => {
                                    target_borrow.push(val.clone());
                                }
                                ArrayKey::String(s) => {
                                    target_borrow.set(ArrayKey::String(s.clone()), val.clone());
                                }
                            }
                        }
                    }
                }

                OpCode::MatchError => {
                    // Throw UnhandledMatchError with the unmatched value
                    let subject = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let subject_repr = match &subject {
                        Value::True => "true".to_string(),
                        Value::False => "false".to_string(),
                        Value::Null | Value::Undef => "NULL".to_string(),
                        Value::Long(n) => n.to_string(),
                        Value::Double(f) => {
                            // PHP shows 5.0 not 5 for floats
                            let s = format!("{}", f);
                            if !s.contains('.') && !s.contains('E') && !s.contains('e') {
                                format!("{}.0", s)
                            } else {
                                s
                            }
                        }
                        Value::String(s) => {
                            let lossy = s.to_string_lossy();
                            let escape_str = |input: &str| -> String {
                                input
                                    .replace('\\', "\\\\")
                                    .replace('\n', "\\n")
                                    .replace('\r', "\\r")
                                    .replace('\t', "\\t")
                                    .replace('\0', "\\0")
                            };
                            if lossy.len() > 15 {
                                // Truncate the raw string to 15 chars, then escape
                                let truncated: String = lossy.chars().take(15).collect();
                                format!("'{}...'", escape_str(&truncated))
                            } else {
                                format!("'{}'", escape_str(&lossy))
                            }
                        }
                        Value::Array(_) => "of type array".to_string(),
                        Value::Object(obj) => {
                            let obj_borrow = obj.borrow();
                            let name = String::from_utf8_lossy(&obj_borrow.class_name);
                            format!("of type {}", name)
                        }
                        _ => subject.to_php_string().to_string_lossy(),
                    };
                    let msg = format!("Unhandled match case {}", subject_repr);

                    let err_id = self.next_object_id;
                    self.next_object_id += 1;
                    let mut err_obj = PhpObject::new(b"UnhandledMatchError".to_vec(), err_id);
                    err_obj.set_property(
                        b"message".to_vec(),
                        Value::String(PhpString::from_string(msg)),
                    );

                    let exc_val = Value::Object(Rc::new(RefCell::new(err_obj)));

                    if let Some((catch_target, _finally_target, _exc_tmp)) =
                        exception_handlers.pop()
                    {
                        self.current_exception = Some(exc_val);
                        ip = catch_target as usize;
                    } else {
                        self.current_exception = Some(exc_val.clone());
                        let msg = if let Value::Object(obj) = &exc_val {
                            let obj = obj.borrow();
                            let class = String::from_utf8_lossy(&obj.class_name).to_string();
                            let message = obj.get_property(b"message");
                            format!(
                                "Uncaught {}: {}",
                                class,
                                message.to_php_string().to_string_lossy()
                            )
                        } else {
                            "Uncaught UnhandledMatchError".to_string()
                        };
                        return Err(VmError {
                            message: msg,
                            line: op.line,
                        });
                    }
                }

                OpCode::LoadConst | OpCode::FastConcat => {
                    // TODO: implement
                }

                OpCode::YieldFrom => {
                    // YieldFrom in non-generator context - shouldn't happen normally
                    // Just evaluate the inner expression
                    let _val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        Value::Null,
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }

                OpCode::Yield => {
                    // Yield should only be executed inside generators, not in the main VM loop.
                    // If we reach here, it means yield was used outside a generator context.
                    // Just treat it as returning null.
                    let idx = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    self.write_operand(&op.result, idx, &mut cvs, &mut tmps, &static_cv_keys);
                }

                OpCode::GeneratorReturn => {
                    // Generator return in main VM - treat as regular return
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    return Ok(val);
                }

                OpCode::DeclareClass => {
                    let name_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let class_idx = self
                        .read_operand(&op.op2, &cvs, &tmps, &op_array.literals)
                        .to_long() as usize;
                    if let Some(mut class) = self.pending_classes.get(class_idx).cloned() {
                        // Resolve inheritance: copy parent methods/properties
                        if let Some(parent_name) = &class.parent.clone() {
                            let parent_lower: Vec<u8> =
                                parent_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                            if let Some(parent) = self.classes.get(&parent_lower).cloned() {
                                // Check if parent is final
                                if parent.is_final {
                                    let parent_display = String::from_utf8_lossy(parent_name).to_string();
                                    let child_display = String::from_utf8_lossy(&name_val.to_php_string().as_bytes()).to_string();
                                    return Err(VmError {
                                        message: format!("Class {} cannot extend final class {}", child_display, parent_display),
                                        line: op.line,
                                    });
                                }
                                // Inherit methods (child overrides take precedence)
                                for (method_name, method) in &parent.methods {
                                    // Check if parent method is final and child overrides it
                                    if let Some(child_method) = class.methods.get(method_name) {
                                        if method.is_final {
                                            let parent_display = String::from_utf8_lossy(parent_name).to_string();
                                            let method_display = String::from_utf8_lossy(&method.name).to_string();
                                            return Err(VmError {
                                                message: format!("Cannot override final method {}::{}()", parent_display, method_display),
                                                line: op.line,
                                            });
                                        }
                                        // Skip __construct compatibility checks
                                        let mn_lower = method_name.as_slice();
                                        if mn_lower != b"__construct" && !method.is_abstract {
                                            // Check method signature compatibility
                                            if let Some(err_msg) = Self::check_method_compatibility(
                                                &class.name, child_method,
                                                &parent.name, method,
                                            ) {
                                                return Err(VmError {
                                                    message: err_msg,
                                                    line: op.line,
                                                });
                                            }
                                        }
                                    } else {
                                        class.methods.insert(method_name.clone(), method.clone());
                                    }
                                }
                                // Inherit properties (parent properties come first, child overrides take precedence)
                                let child_prop_names: Vec<Vec<u8>> =
                                    class.properties.iter().map(|p| p.name.clone()).collect();
                                let mut new_props = Vec::new();
                                for prop in &parent.properties {
                                    if !child_prop_names.contains(&prop.name) {
                                        new_props.push(prop.clone());
                                    }
                                }
                                new_props.append(&mut class.properties);
                                class.properties = new_props;
                                // Inherit constants
                                for (const_name, const_val) in &parent.constants {
                                    if !class.constants.contains_key(const_name) {
                                        class
                                            .constants
                                            .insert(const_name.clone(), const_val.clone());
                                    }
                                }
                                // Inherit static properties
                                for (prop_name, prop_val) in &parent.static_properties {
                                    if !class.static_properties.contains_key(prop_name) {
                                        class
                                            .static_properties
                                            .insert(prop_name.clone(), prop_val.clone());
                                    }
                                }
                            }
                        }
                        // Resolve interfaces: copy interface methods into the class
                        let iface_names = class.interfaces.clone();
                        for iface_name in &iface_names {
                            let iface_lower: Vec<u8> =
                                iface_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                            if let Some(iface) = self.classes.get(&iface_lower).cloned() {
                                for (method_name, method) in &iface.methods {
                                    if !class.methods.contains_key(method_name) {
                                        class.methods.insert(method_name.clone(), method.clone());
                                    }
                                }
                                // Inherit interface constants
                                for (const_name, const_val) in &iface.constants {
                                    if !class.constants.contains_key(const_name) {
                                        class
                                            .constants
                                            .insert(const_name.clone(), const_val.clone());
                                    }
                                }
                            }
                        }

                        // Resolve traits: copy trait methods/properties/constants into the class
                        let trait_names = class.traits.clone();
                        let trait_adaptations = class.trait_adaptations.clone();
                        let class_name_lower: Vec<u8> = class.name.iter().map(|b| b.to_ascii_lowercase()).collect();

                        for trait_name in &trait_names {
                            let trait_lower: Vec<u8> =
                                trait_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                            if let Some(trait_def) = self.classes.get(&trait_lower).cloned() {
                                // Copy trait methods (class's own methods take precedence)
                                for (method_name, method) in &trait_def.methods {
                                    // Check if this method is excluded by an insteadof rule
                                    let mut excluded = false;
                                    for adapt in &trait_adaptations {
                                        if let crate::object::TraitAdaptation::Precedence {
                                            trait_name: prec_trait,
                                            method: prec_method,
                                            instead_of,
                                        } = adapt
                                        {
                                            let prec_method_lower: Vec<u8> = prec_method.iter().map(|b| b.to_ascii_lowercase()).collect();
                                            let prec_trait_lower: Vec<u8> = prec_trait.iter().map(|b| b.to_ascii_lowercase()).collect();
                                            if *method_name == prec_method_lower && prec_trait_lower != trait_lower {
                                                // This trait's method is being overridden
                                                let io_lower: Vec<Vec<u8>> = instead_of.iter().map(|n| n.iter().map(|b| b.to_ascii_lowercase()).collect()).collect();
                                                if io_lower.contains(&trait_lower) {
                                                    excluded = true;
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                    if excluded {
                                        continue;
                                    }
                                    if !class.methods.contains_key(method_name) {
                                        let mut m = method.clone();
                                        // Trait methods should have scope of the using class
                                        m.declaring_class = class_name_lower.clone();
                                        m.op_array.scope_class = Some(class_name_lower.clone());
                                        class.methods.insert(method_name.clone(), m);
                                    }
                                }
                                // Copy trait properties (class's own properties take precedence)
                                let child_prop_names: Vec<Vec<u8>> =
                                    class.properties.iter().map(|p| p.name.clone()).collect();
                                for prop in &trait_def.properties {
                                    if !child_prop_names.contains(&prop.name) {
                                        let mut p = prop.clone();
                                        p.declaring_class = class_name_lower.clone();
                                        class.properties.push(p);
                                    }
                                }
                                // Copy trait constants (class's own constants take precedence)
                                for (const_name, const_val) in &trait_def.constants {
                                    if !class.constants.contains_key(const_name) {
                                        class
                                            .constants
                                            .insert(const_name.clone(), const_val.clone());
                                    }
                                }
                                // Copy trait static properties (class's own take precedence)
                                for (prop_name, prop_val) in &trait_def.static_properties {
                                    if !class.static_properties.contains_key(prop_name) {
                                        class
                                            .static_properties
                                            .insert(prop_name.clone(), prop_val.clone());
                                    }
                                }
                            }
                        }

                        // Apply trait aliases
                        for adapt in &trait_adaptations {
                            if let crate::object::TraitAdaptation::Alias {
                                trait_name: alias_trait,
                                method: alias_method,
                                new_name,
                                new_visibility,
                            } = adapt
                            {
                                let method_lower: Vec<u8> = alias_method.iter().map(|b| b.to_ascii_lowercase()).collect();

                                // Find the source method (from specified trait or any trait)
                                let source_method = if let Some(tn) = alias_trait {
                                    let tn_lower: Vec<u8> = tn.iter().map(|b| b.to_ascii_lowercase()).collect();
                                    self.classes.get(&tn_lower).and_then(|t| t.methods.get(&method_lower).cloned())
                                } else {
                                    // Look in the class's already-imported methods
                                    class.methods.get(&method_lower).cloned()
                                };

                                if let Some(mut m) = source_method {
                                    m.declaring_class = class_name_lower.clone();
                                    m.op_array.scope_class = Some(class_name_lower.clone());
                                    if let Some(vis) = new_visibility {
                                        m.visibility = *vis;
                                    }
                                    if let Some(nn) = new_name {
                                        let nn_lower: Vec<u8> = nn.iter().map(|b| b.to_ascii_lowercase()).collect();
                                        m.name = nn.clone();
                                        class.methods.insert(nn_lower, m);
                                    } else {
                                        // Just change visibility on existing method
                                        if let Some(existing) = class.methods.get_mut(&method_lower) {
                                            if let Some(vis) = new_visibility {
                                                existing.visibility = *vis;
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // Check for unimplemented abstract methods (interface enforcement)
                        if !class.is_abstract && !class.is_interface && !class.is_trait {
                            let mut abstract_methods: Vec<String> = Vec::new();
                            for (_, method) in &class.methods {
                                if method.is_abstract {
                                    // Find which interface this method belongs to
                                    let mut iface_origin = String::new();
                                    for iface_name in &iface_names {
                                        let iface_lower: Vec<u8> = iface_name
                                            .iter()
                                            .map(|b| b.to_ascii_lowercase())
                                            .collect();
                                        if let Some(iface) = self.classes.get(&iface_lower) {
                                            let method_lower: Vec<u8> = method
                                                .name
                                                .iter()
                                                .map(|b| b.to_ascii_lowercase())
                                                .collect();
                                            if iface.methods.contains_key(&method_lower) {
                                                iface_origin = String::from_utf8_lossy(&iface.name)
                                                    .to_string();
                                                break;
                                            }
                                        }
                                    }
                                    if iface_origin.is_empty() {
                                        iface_origin = String::from_utf8_lossy(
                                            &name_val.to_php_string().as_bytes(),
                                        )
                                        .to_string();
                                    }
                                    let method_name_str =
                                        String::from_utf8_lossy(&method.name).to_string();
                                    abstract_methods
                                        .push(format!("{}::{}", iface_origin, method_name_str));
                                }
                            }
                            if !abstract_methods.is_empty() {
                                let class_name_str =
                                    String::from_utf8_lossy(&name_val.to_php_string().as_bytes())
                                        .to_string();
                                let class_name_lower_str: Vec<u8> = class_name_str.bytes().map(|b| b.to_ascii_lowercase()).collect();
                                let count = abstract_methods.len();
                                abstract_methods.sort();
                                let methods_list = abstract_methods.join(", ");
                                let method_word = if count == 1 { "method" } else { "methods" };
                                // Check if the abstract method was declared by this class itself
                                // (not inherited from parent/interface)
                                let mut self_declared_abstract = Vec::new();
                                for (_, method) in &class.methods {
                                    if method.is_abstract && method.declaring_class == class_name_lower_str {
                                        self_declared_abstract.push(
                                            String::from_utf8_lossy(&method.name).to_string()
                                        );
                                    }
                                }
                                let msg = if !self_declared_abstract.is_empty() && self_declared_abstract.len() == count {
                                    // All abstract methods are self-declared
                                    format!(
                                        "Class {} declares abstract method {}() and must therefore be declared abstract",
                                        class_name_str, self_declared_abstract[0]
                                    )
                                } else {
                                    format!(
                                        "Class {} contains {} abstract {} and must therefore be declared abstract or implement the remaining {} ({})",
                                        class_name_str, count, method_word, method_word, methods_list
                                    )
                                };
                                return Err(VmError {
                                    message: msg,
                                    line: op.line,
                                });
                            }
                        }

                        let name_lower: Vec<u8> = name_val
                            .to_php_string()
                            .as_bytes()
                            .iter()
                            .map(|b| b.to_ascii_lowercase())
                            .collect();
                        let class_name_orig = name_val.to_php_string().as_bytes().to_vec();

                        // Register all methods as callable functions: ClassName::methodName
                        for (method_name, method) in &class.methods {
                            let mut func_name = class_name_orig.clone();
                            func_name.extend_from_slice(b"::");
                            func_name.extend_from_slice(&method.name);
                            self.user_functions
                                .insert(func_name.to_ascii_lowercase(), method.op_array.clone());
                        }

                        // Resolve deferred constant references (self::CONST, ClassName::CONST)
                        // in class constants
                        let const_keys: Vec<Vec<u8>> = class.constants.keys().cloned().collect();
                        for const_name in const_keys {
                            if let Some(Value::String(s)) = class.constants.get(&const_name) {
                                let s_bytes = s.as_bytes();
                                if s_bytes.starts_with(b"__deferred_const__::") {
                                    let rest = &s_bytes[b"__deferred_const__::".len()..];
                                    // Parse ClassName::CONSTANT_NAME
                                    if let Some(sep_pos) = rest.windows(2).position(|w| w == b"::") {
                                        let ref_class_name = &rest[..sep_pos];
                                        let ref_const_name = &rest[sep_pos + 2..];
                                        let ref_class_lower: Vec<u8> = ref_class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                                        // Look up the constant in the referenced class or current class
                                        let resolved = if ref_class_lower == name_lower.as_slice() {
                                            // self:: reference to same class
                                            class.constants.get(ref_const_name).cloned()
                                        } else if let Some(ref_class) = self.classes.get(&ref_class_lower) {
                                            ref_class.constants.get(ref_const_name).cloned()
                                        } else {
                                            None
                                        };
                                        if let Some(val) = resolved {
                                            class.constants.insert(const_name, val);
                                        }
                                    }
                                }
                            }
                        }
                        // Resolve deferred constant references in property defaults
                        for prop in &mut class.properties {
                            if let Value::String(s) = &prop.default {
                                let s_bytes = s.as_bytes();
                                if s_bytes.starts_with(b"__deferred_const__::") {
                                    let rest = &s_bytes[b"__deferred_const__::".len()..];
                                    if let Some(sep_pos) = rest.windows(2).position(|w| w == b"::") {
                                        let ref_class_name = &rest[..sep_pos];
                                        let ref_const_name = &rest[sep_pos + 2..];
                                        let ref_class_lower: Vec<u8> = ref_class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                                        let resolved = if ref_class_lower == name_lower.as_slice() {
                                            class.constants.get(ref_const_name).cloned()
                                        } else if let Some(ref_class) = self.classes.get(&ref_class_lower) {
                                            ref_class.constants.get(ref_const_name).cloned()
                                        } else {
                                            None
                                        };
                                        if let Some(val) = resolved {
                                            prop.default = val;
                                        }
                                    }
                                }
                            }
                        }
                        // Resolve deferred constant references in static properties
                        let static_keys: Vec<Vec<u8>> = class.static_properties.keys().cloned().collect();
                        for prop_name in static_keys {
                            if let Some(Value::String(s)) = class.static_properties.get(&prop_name) {
                                let s_bytes = s.as_bytes();
                                if s_bytes.starts_with(b"__deferred_const__::") {
                                    let rest = &s_bytes[b"__deferred_const__::".len()..];
                                    if let Some(sep_pos) = rest.windows(2).position(|w| w == b"::") {
                                        let ref_class_name = &rest[..sep_pos];
                                        let ref_const_name = &rest[sep_pos + 2..];
                                        let ref_class_lower: Vec<u8> = ref_class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                                        let resolved = if ref_class_lower == name_lower.as_slice() {
                                            class.constants.get(ref_const_name).cloned()
                                        } else if let Some(ref_class) = self.classes.get(&ref_class_lower) {
                                            ref_class.constants.get(ref_const_name).cloned()
                                        } else {
                                            None
                                        };
                                        if let Some(val) = resolved {
                                            class.static_properties.insert(prop_name, val);
                                        }
                                    }
                                }
                            }
                        }

                        self.classes.insert(name_lower, class);
                    }
                }

                OpCode::NewObject => {
                    let class_name_raw = self
                        .read_operand(&op.op1, &cvs, &tmps, &op_array.literals)
                        .to_php_string();

                    // Resolve "static" and "self" for late static binding / new self()
                    let resolved_bytes =
                        if class_name_raw.as_bytes().eq_ignore_ascii_case(b"static") {
                            self.resolve_static_class(class_name_raw.as_bytes())
                                .to_vec()
                        } else if class_name_raw.as_bytes().eq_ignore_ascii_case(b"self") {
                            // self:: in new context - use called class stack
                            if let Some(called) = self.called_class_stack.last() {
                                called.clone()
                            } else {
                                class_name_raw.as_bytes().to_vec()
                            }
                        } else {
                            class_name_raw.as_bytes().to_vec()
                        };
                    let class_name = PhpString::from_vec(resolved_bytes);
                    let name_lower: Vec<u8> = class_name
                        .as_bytes()
                        .iter()
                        .map(|b| b.to_ascii_lowercase())
                        .collect();

                    // Check for abstract class or interface
                    if let Some(class) = self.classes.get(&name_lower) {
                        if class.is_abstract || class.is_interface {
                            // Create an Error object and throw it
                            let err_msg = if class.is_interface {
                                format!(
                                    "Cannot instantiate interface {}",
                                    class_name.to_string_lossy()
                                )
                            } else {
                                format!(
                                    "Cannot instantiate abstract class {}",
                                    class_name.to_string_lossy()
                                )
                            };
                            let err_id = self.next_object_id;
                            self.next_object_id += 1;
                            let mut err_obj = PhpObject::new(b"Error".to_vec(), err_id);
                            err_obj.set_property(
                                b"message".to_vec(),
                                Value::String(PhpString::from_string(err_msg.clone())),
                            );
                            self.current_exception =
                                Some(Value::Object(Rc::new(RefCell::new(err_obj))));

                            if let Some((catch_target, _, _)) = exception_handlers.pop() {
                                ip = catch_target as usize;
                                continue;
                            } else {
                                return Err(VmError {
                                    message: err_msg,
                                    line: op.line,
                                });
                            }
                        }
                    }

                    let obj_id = self.next_object_id;
                    self.next_object_id += 1;

                    // Use canonical class name from class table if available,
                    // or normalize well-known class names
                    let canonical_name = if let Some(class) = self.classes.get(&name_lower) {
                        class.name.clone()
                    } else {
                        // Normalize well-known class names
                        match name_lower.as_slice() {
                            b"stdclass" => b"stdClass".to_vec(),
                            b"exception" => b"Exception".to_vec(),
                            b"error" => b"Error".to_vec(),
                            b"typeerror" => b"TypeError".to_vec(),
                            b"valueerror" => b"ValueError".to_vec(),
                            b"runtimeexception" => b"RuntimeException".to_vec(),
                            b"logicexception" => b"LogicException".to_vec(),
                            b"invalidargumentexception" => b"InvalidArgumentException".to_vec(),
                            b"badmethodcallexception" => b"BadMethodCallException".to_vec(),
                            b"badfunctioncallexception" => b"BadFunctionCallException".to_vec(),
                            b"overflowexception" => b"OverflowException".to_vec(),
                            b"underflowexception" => b"UnderflowException".to_vec(),
                            b"rangeerror" => b"RangeError".to_vec(),
                            b"arithmeticerror" => b"ArithmeticError".to_vec(),
                            b"divisionbyzeroerror" => b"DivisionByZeroError".to_vec(),
                            b"argumentcounterror" => b"ArgumentCountError".to_vec(),
                            b"errorexception" => b"ErrorException".to_vec(),
                            b"closedgeneratorexception" => b"ClosedGeneratorException".to_vec(),
                            b"unexpectedvalueexception" => b"UnexpectedValueException".to_vec(),
                            b"domainexception" => b"DomainException".to_vec(),
                            b"assertionerror" => b"AssertionError".to_vec(),
                            b"unhandledmatcherror" => b"UnhandledMatchError".to_vec(),
                            // SPL classes
                            b"arrayobject" => b"ArrayObject".to_vec(),
                            b"arrayiterator" => b"ArrayIterator".to_vec(),
                            b"splfixedarray" => b"SplFixedArray".to_vec(),
                            b"spldoublylinkedlist" => b"SplDoublyLinkedList".to_vec(),
                            b"splstack" => b"SplStack".to_vec(),
                            b"splqueue" => b"SplQueue".to_vec(),
                            b"splpriorityqueue" => b"SplPriorityQueue".to_vec(),
                            b"splmaxheap" => b"SplMaxHeap".to_vec(),
                            b"splminheap" => b"SplMinHeap".to_vec(),
                            b"splobjectstorage" => b"SplObjectStorage".to_vec(),
                            b"recursivearrayiterator" => b"RecursiveArrayIterator".to_vec(),
                            b"lengthexception" => b"LengthException".to_vec(),
                            b"outofrangeexception" => b"OutOfRangeException".to_vec(),
                            b"outofboundsexception" => b"OutOfBoundsException".to_vec(),
                            b"invalidargumentexception" => b"InvalidArgumentException".to_vec(),
                            _ => class_name.as_bytes().to_vec(),
                        }
                    };
                    let mut obj = PhpObject::new(canonical_name, obj_id);

                    // Built-in Exception/Error classes get default properties
                    // Check if this is a Throwable subclass (built-in or user-defined)
                    let is_throwable = name_lower == b"exception"
                        || name_lower == b"error"
                        || is_builtin_subclass(&name_lower, b"exception")
                        || is_builtin_subclass(&name_lower, b"error")
                        || self.class_extends(
                            &name_lower,
                            b"exception",
                        )
                        || self.class_extends(
                            &name_lower,
                            b"error",
                        );
                    if is_throwable {
                        obj.set_property(b"message".to_vec(), Value::String(PhpString::empty()));
                        obj.set_property(b"code".to_vec(), Value::Long(0));
                        obj.set_property(
                            b"file".to_vec(),
                            Value::String(PhpString::from_string(self.current_file.clone())),
                        );
                        obj.set_property(b"line".to_vec(), Value::Long(op.line as i64));
                        obj.set_property(b"previous".to_vec(), Value::Null);
                        // Build trace from call stack
                        let mut trace_arr = PhpArray::new();
                        for (_i, (func_name, file, line, args, is_instance)) in self.call_stack.iter().rev().enumerate() {
                            let mut frame = PhpArray::new();
                            frame.set(ArrayKey::String(PhpString::from_bytes(b"file")), Value::String(PhpString::from_string(file.clone())));
                            frame.set(ArrayKey::String(PhpString::from_bytes(b"line")), Value::Long(*line as i64));
                            frame.set(ArrayKey::String(PhpString::from_bytes(b"function")), Value::String(PhpString::from_string(func_name.clone())));
                            // Parse class::method for class/type
                            if let Some(pos) = func_name.find("::") {
                                frame.set(ArrayKey::String(PhpString::from_bytes(b"class")), Value::String(PhpString::from_string(func_name[..pos].to_string())));
                                let type_str = if *is_instance { b"->" as &[u8] } else { b"::" };
                                frame.set(ArrayKey::String(PhpString::from_bytes(b"type")), Value::String(PhpString::from_bytes(type_str)));
                                frame.set(ArrayKey::String(PhpString::from_bytes(b"function")), Value::String(PhpString::from_string(func_name[pos+2..].to_string())));
                            }
                            // Include actual arguments in trace
                            let mut args_arr = PhpArray::new();
                            for arg in args {
                                args_arr.push(arg.clone());
                            }
                            frame.set(ArrayKey::String(PhpString::from_bytes(b"args")), Value::Array(Rc::new(RefCell::new(args_arr))));
                            trace_arr.push(Value::Array(Rc::new(RefCell::new(frame))));
                        }
                        obj.set_property(
                            b"trace".to_vec(),
                            Value::Array(Rc::new(RefCell::new(trace_arr))),
                        );
                    }

                    // Initialize SPL class internal storage
                    match name_lower.as_slice() {
                        b"arrayobject" | b"arrayiterator" | b"recursivearrayiterator" => {
                            obj.set_property(
                                b"__spl_array".to_vec(),
                                Value::Array(Rc::new(RefCell::new(PhpArray::new()))),
                            );
                            obj.set_property(b"__spl_flags".to_vec(), Value::Long(0));
                        }
                        b"splfixedarray" => {
                            obj.set_property(
                                b"__spl_array".to_vec(),
                                Value::Array(Rc::new(RefCell::new(PhpArray::new()))),
                            );
                            obj.set_property(b"__spl_size".to_vec(), Value::Long(0));
                        }
                        b"spldoublylinkedlist" | b"splstack" | b"splqueue" | b"splpriorityqueue"
                        | b"splmaxheap" | b"splminheap" => {
                            obj.set_property(
                                b"__spl_array".to_vec(),
                                Value::Array(Rc::new(RefCell::new(PhpArray::new()))),
                            );
                        }
                        b"splobjectstorage" => {
                            obj.set_property(
                                b"__spl_array".to_vec(),
                                Value::Array(Rc::new(RefCell::new(PhpArray::new()))),
                            );
                        }
                        _ => {}
                    }

                    // Initialize properties from class definition
                    if let Some(class) = self.classes.get(&name_lower) {
                        for prop in &class.properties {
                            if !prop.is_static {
                                if prop.is_readonly {
                                    // Readonly properties start as Undef to allow first assignment
                                    obj.set_property(prop.name.clone(), Value::Undef);
                                } else {
                                    obj.set_property(prop.name.clone(), prop.default.clone());
                                }
                            }
                        }
                    }

                    let obj_value = Value::Object(Rc::new(RefCell::new(obj)));

                    // Track objects with __destruct for shutdown-time destruction
                    let has_destruct = self
                        .classes
                        .get(&name_lower)
                        .map(|c| c.methods.contains_key(&b"__destruct".to_vec()))
                        .unwrap_or(false);
                    if has_destruct {
                        self.destructible_objects.push(obj_value.clone());
                    }

                    self.write_operand(&op.result, obj_value, &mut cvs, &mut tmps, &static_cv_keys);
                }

                OpCode::PropertyGet => {
                    let obj_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let prop_name = self
                        .read_operand(&op.op2, &cvs, &tmps, &op_array.literals)
                        .to_php_string();

                    let result = if let Value::Object(obj) = &obj_val {
                        let class_name_orig = obj.borrow().class_name.clone();
                        let class_lower: Vec<u8> = class_name_orig
                            .iter()
                            .map(|b| b.to_ascii_lowercase())
                            .collect();

                        // Check visibility before accessing the property
                        let mut visibility_error: Option<String> = None;
                        if let Some((vis, declaring_class, _is_readonly)) = self.find_property_def(&class_lower, prop_name.as_bytes()) {
                            if vis != Visibility::Public {
                                let caller_scope = self.current_class_scope();
                                let prop_name_str = String::from_utf8_lossy(prop_name.as_bytes()).to_string();
                                visibility_error = self.check_visibility(
                                    vis,
                                    &declaring_class,
                                    &class_name_orig,
                                    &prop_name_str,
                                    true,
                                    caller_scope.as_deref(),
                                );
                            }
                        }

                        if let Some(err_msg) = visibility_error {
                            // Property is inaccessible - try __get magic method first
                            let has_get = self
                                .classes
                                .get(&class_lower)
                                .map(|c| c.methods.contains_key(&b"__get".to_vec()))
                                .unwrap_or(false);
                            if has_get && self.magic_depth < 5 {
                                self.magic_depth += 1;
                                let magic_method_def = self
                                    .classes
                                    .get(&class_lower)
                                    .unwrap()
                                    .get_method(b"__get")
                                    .unwrap();
                                let method = magic_method_def.op_array.clone();
                                let magic_declaring = magic_method_def.declaring_class.clone();
                                let mut fn_cvs = vec![Value::Undef; method.cv_names.len()];
                                if !fn_cvs.is_empty() {
                                    fn_cvs[0] = obj_val.clone();
                                }
                                if fn_cvs.len() > 1 {
                                    fn_cvs[1] = Value::String(prop_name.clone());
                                }
                                self.class_scope_stack.push(magic_declaring.clone());
                                self.called_class_stack.push(class_name_orig.clone());
                                let result = self
                                    .execute_op_array(&method, fn_cvs)
                                    .unwrap_or(Value::Null);
                                self.called_class_stack.pop();
                                self.class_scope_stack.pop();
                                self.magic_depth -= 1;
                                result
                            } else {
                                // No __get - throw the error
                                let err_id = self.next_object_id;
                                self.next_object_id += 1;
                                let mut err_obj = PhpObject::new(b"Error".to_vec(), err_id);
                                err_obj.set_property(
                                    b"message".to_vec(),
                                    Value::String(PhpString::from_string(err_msg.clone())),
                                );
                                err_obj.set_property(b"code".to_vec(), Value::Long(0));
                                self.current_exception =
                                    Some(Value::Object(Rc::new(RefCell::new(err_obj))));
                                if let Some((catch_target, _, _)) = exception_handlers.pop() {
                                    ip = catch_target as usize;
                                    continue;
                                } else {
                                    return Err(VmError {
                                        message: format!("Uncaught Error: {}", err_msg),
                                        line: op.line,
                                    });
                                }
                            }
                        } else {
                            let prop = obj.borrow().get_property(prop_name.as_bytes());
                            if matches!(prop, Value::Null)
                                && !obj.borrow().has_property(prop_name.as_bytes())
                                && self.magic_depth < 5
                            {
                                // Try __get magic method (with recursion guard)
                                let has_get = self
                                    .classes
                                    .get(&class_lower)
                                    .map(|c| c.methods.contains_key(&b"__get".to_vec()))
                                    .unwrap_or(false);
                                if has_get {
                                    self.magic_depth += 1;
                                    let magic_method_def = self
                                        .classes
                                        .get(&class_lower)
                                        .unwrap()
                                        .get_method(b"__get")
                                        .unwrap();
                                    let method = magic_method_def.op_array.clone();
                                    let magic_declaring = magic_method_def.declaring_class.clone();
                                    let mut fn_cvs = vec![Value::Undef; method.cv_names.len()];
                                    if !fn_cvs.is_empty() {
                                        fn_cvs[0] = obj_val.clone();
                                    } // $this
                                    if fn_cvs.len() > 1 {
                                        fn_cvs[1] = Value::String(prop_name.clone());
                                    } // $name
                                    self.class_scope_stack.push(magic_declaring);
                                    self.called_class_stack.push(class_name_orig.clone());
                                    let result = self
                                        .execute_op_array(&method, fn_cvs)
                                        .unwrap_or(Value::Null);
                                    self.called_class_stack.pop();
                                    self.class_scope_stack.pop();
                                    self.magic_depth -= 1;
                                    result
                                } else {
                                    Value::Null
                                }
                            } else {
                                prop
                            }
                        }
                    } else {
                        Value::Null
                    };
                    self.write_operand(&op.result, result, &mut cvs, &mut tmps, &static_cv_keys);
                }

                OpCode::PropertySet => {
                    let obj_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let value = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    let prop_name = self
                        .read_operand(&op.result, &cvs, &tmps, &op_array.literals)
                        .to_php_string();

                    if let Value::Object(obj) = &obj_val {
                        let class_name_orig = obj.borrow().class_name.clone();
                        let class_lower: Vec<u8> = class_name_orig
                            .iter()
                            .map(|b| b.to_ascii_lowercase())
                            .collect();

                        // Check visibility and readonly before setting the property
                        let mut visibility_error: Option<String> = None;
                        let mut readonly_error: Option<String> = None;
                        if let Some((vis, declaring_class, prop_is_readonly)) = self.find_property_def(&class_lower, prop_name.as_bytes()) {
                            if vis != Visibility::Public {
                                let caller_scope = self.current_class_scope();
                                let prop_name_str = String::from_utf8_lossy(prop_name.as_bytes()).to_string();
                                visibility_error = self.check_visibility(
                                    vis,
                                    &declaring_class,
                                    &class_name_orig,
                                    &prop_name_str,
                                    true,
                                    caller_scope.as_deref(),
                                );
                            }
                            // Enforce readonly: if property is readonly and already initialized (not Undef), reject
                            if prop_is_readonly {
                                let current_val = obj.borrow().get_property(prop_name.as_bytes());
                                // A readonly property can be set once (from Undef/Null initial state)
                                // but cannot be modified after initialization
                                if obj.borrow().has_property(prop_name.as_bytes()) && !matches!(current_val, Value::Undef) {
                                    let class_display = String::from_utf8_lossy(&class_name_orig).to_string();
                                    let prop_display = String::from_utf8_lossy(prop_name.as_bytes()).to_string();
                                    readonly_error = Some(format!("Cannot modify readonly property {}::${}", class_display, prop_display));
                                }
                            }
                        }

                        if let Some(err_msg) = readonly_error {
                            // Readonly violation - throw Error
                            let err_id = self.next_object_id;
                            self.next_object_id += 1;
                            let mut err_obj = PhpObject::new(b"Error".to_vec(), err_id);
                            err_obj.set_property(
                                b"message".to_vec(),
                                Value::String(PhpString::from_string(err_msg.clone())),
                            );
                            err_obj.set_property(b"code".to_vec(), Value::Long(0));
                            self.current_exception =
                                Some(Value::Object(Rc::new(RefCell::new(err_obj))));
                            if let Some((catch_target, _, _)) = exception_handlers.pop() {
                                ip = catch_target as usize;
                                continue;
                            } else {
                                return Err(VmError {
                                    message: format!("Uncaught Error: {}", err_msg),
                                    line: op.line,
                                });
                            }
                        } else if let Some(err_msg) = visibility_error {
                            // Property is inaccessible - try __set magic method first
                            let has_set = self
                                .classes
                                .get(&class_lower)
                                .map(|c| c.methods.contains_key(&b"__set".to_vec()))
                                .unwrap_or(false);
                            if has_set && self.magic_depth < 5 {
                                self.magic_depth += 1;
                                let magic_method_def = self
                                    .classes
                                    .get(&class_lower)
                                    .unwrap()
                                    .get_method(b"__set")
                                    .unwrap();
                                let method = magic_method_def.op_array.clone();
                                let magic_declaring = magic_method_def.declaring_class.clone();
                                let mut fn_cvs = vec![Value::Undef; method.cv_names.len()];
                                if !fn_cvs.is_empty() {
                                    fn_cvs[0] = obj_val.clone();
                                }
                                if fn_cvs.len() > 1 {
                                    fn_cvs[1] = Value::String(prop_name.clone());
                                }
                                if fn_cvs.len() > 2 {
                                    fn_cvs[2] = value;
                                }
                                self.class_scope_stack.push(magic_declaring);
                                self.called_class_stack.push(class_name_orig.clone());
                                let _ = self.execute_op_array(&method, fn_cvs);
                                self.called_class_stack.pop();
                                self.class_scope_stack.pop();
                                self.magic_depth -= 1;
                            } else {
                                // No __set - throw the error
                                let err_id = self.next_object_id;
                                self.next_object_id += 1;
                                let mut err_obj = PhpObject::new(b"Error".to_vec(), err_id);
                                err_obj.set_property(
                                    b"message".to_vec(),
                                    Value::String(PhpString::from_string(err_msg.clone())),
                                );
                                err_obj.set_property(b"code".to_vec(), Value::Long(0));
                                self.current_exception =
                                    Some(Value::Object(Rc::new(RefCell::new(err_obj))));
                                if let Some((catch_target, _, _)) = exception_handlers.pop() {
                                    ip = catch_target as usize;
                                    continue;
                                } else {
                                    return Err(VmError {
                                        message: format!("Uncaught Error: {}", err_msg),
                                        line: op.line,
                                    });
                                }
                            }
                        } else {
                            let has_set = self
                                .classes
                                .get(&class_lower)
                                .map(|c| c.methods.contains_key(&b"__set".to_vec()))
                                .unwrap_or(false);
                            if has_set
                                && !obj.borrow().has_property(prop_name.as_bytes())
                                && self.magic_depth < 5
                            {
                                let magic_method_def = self
                                    .classes
                                    .get(&class_lower)
                                    .unwrap()
                                    .get_method(b"__set")
                                    .unwrap();
                                let method = magic_method_def.op_array.clone();
                                let magic_declaring = magic_method_def.declaring_class.clone();
                                let mut fn_cvs = vec![Value::Undef; method.cv_names.len()];
                                if !fn_cvs.is_empty() {
                                    fn_cvs[0] = obj_val.clone();
                                }
                                if fn_cvs.len() > 1 {
                                    fn_cvs[1] = Value::String(prop_name.clone());
                                }
                                if fn_cvs.len() > 2 {
                                    fn_cvs[2] = value;
                                }
                                self.class_scope_stack.push(magic_declaring);
                                self.called_class_stack.push(class_name_orig.clone());
                                let _ = self.execute_op_array(&method, fn_cvs);
                                self.called_class_stack.pop();
                                self.class_scope_stack.pop();
                            } else {
                                obj.borrow_mut()
                                    .set_property(prop_name.as_bytes().to_vec(), value);
                            }
                        }
                    }
                }

                OpCode::InitMethodCall => {
                    let obj_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let method_name = self
                        .read_operand(&op.op2, &cvs, &tmps, &op_array.literals)
                        .to_php_string();

                    if let Value::Object(obj) = &obj_val {
                        let class_name_orig;
                        let class_name_lower: Vec<u8>;
                        let method_name_lower: Vec<u8> = method_name
                            .as_bytes()
                            .iter()
                            .map(|b| b.to_ascii_lowercase())
                            .collect();
                        {
                            let obj_borrow = obj.borrow();
                            class_name_orig = obj_borrow.class_name.clone();
                            class_name_lower = obj_borrow
                                .class_name
                                .iter()
                                .map(|b| b.to_ascii_lowercase())
                                .collect();
                        } // obj_borrow dropped here

                        // Only apply builtin exception methods to Throwable subclasses
                        let is_throwable = class_name_lower == b"exception"
                            || class_name_lower == b"error"
                            || is_builtin_subclass(&class_name_lower, b"exception")
                            || is_builtin_subclass(&class_name_lower, b"error")
                            || self.class_extends(&class_name_lower, b"exception")
                            || self.class_extends(&class_name_lower, b"error");

                        // Check if the class has a user-defined method for this call
                        let has_user_method = self
                            .classes
                            .get(&class_name_lower)
                            .map(|c| c.methods.contains_key(&method_name_lower))
                            .unwrap_or(false);

                        let builtin_result = if is_throwable && !has_user_method {
                            let obj_borrow = obj.borrow();
                            match method_name_lower.as_slice() {
                                b"getmessage" => Some(obj_borrow.get_property(b"message")),
                                b"getcode" => Some(obj_borrow.get_property(b"code")),
                                b"getfile" => Some(obj_borrow.get_property(b"file")),
                                b"getline" => Some(obj_borrow.get_property(b"line")),
                                b"gettrace" => Some(obj_borrow.get_property(b"trace")),
                                b"gettraceasstring" => {
                                    let trace = obj_borrow.get_property(b"trace");
                                    let trace_str = if let Value::Array(arr) = &trace {
                                        let arr = arr.borrow();
                                        let mut lines = Vec::new();
                                        let mut idx = 0;
                                        for (_key, frame_val) in arr.iter() {
                                            if let Value::Array(frame) = frame_val {
                                                let frame = frame.borrow();
                                                let file = frame.get(&crate::array::ArrayKey::String(PhpString::from_bytes(b"file")))
                                                    .map(|v| v.to_php_string().to_string_lossy())
                                                    .unwrap_or_default();
                                                let line = frame.get(&crate::array::ArrayKey::String(PhpString::from_bytes(b"line")))
                                                    .map(|v| v.to_long())
                                                    .unwrap_or(0);
                                                let function = frame.get(&crate::array::ArrayKey::String(PhpString::from_bytes(b"function")))
                                                    .map(|v| v.to_php_string().to_string_lossy())
                                                    .unwrap_or_default();
                                                let class = frame.get(&crate::array::ArrayKey::String(PhpString::from_bytes(b"class")))
                                                    .map(|v| v.to_php_string().to_string_lossy())
                                                    .unwrap_or_default();
                                                let type_str = frame.get(&crate::array::ArrayKey::String(PhpString::from_bytes(b"type")))
                                                    .map(|v| v.to_php_string().to_string_lossy())
                                                    .unwrap_or_default();
                                                let loc = if file.is_empty() {
                                                    "[internal function]".to_string()
                                                } else {
                                                    format!("{}({})", file, line)
                                                };
                                                lines.push(format!("#{} {}: {}{}{}()", idx, loc, class, type_str, function));
                                            }
                                            idx += 1;
                                        }
                                        lines.push(format!("#{} {{main}}", idx));
                                        lines.join("\n")
                                    } else {
                                        "#0 {main}".to_string()
                                    };
                                    Some(Value::String(PhpString::from_string(trace_str)))
                                }
                                b"getprevious" => Some(obj_borrow.get_property(b"previous")),
                                b"getseverity" => {
                                    let severity = obj_borrow.get_property(b"severity");
                                    Some(if matches!(severity, Value::Undef | Value::Null) {
                                        Value::Long(1) // E_ERROR default
                                    } else {
                                        severity
                                    })
                                }
                                b"__tostring" => {
                                    let class_display = String::from_utf8_lossy(&obj_borrow.class_name).to_string();
                                    let message = obj_borrow.get_property(b"message").to_php_string().to_string_lossy();
                                    let file = obj_borrow.get_property(b"file").to_php_string().to_string_lossy();
                                    let line = obj_borrow.get_property(b"line").to_long();
                                    let file_str = if file.is_empty() { self.current_file.clone() } else { file };
                                    // Build trace string from stored trace
                                    let trace = obj_borrow.get_property(b"trace");
                                    let trace_str = if let Value::Array(arr) = &trace {
                                        let arr = arr.borrow();
                                        let mut lines = Vec::new();
                                        let mut idx = 0;
                                        for (_key, frame_val) in arr.iter() {
                                            if let Value::Array(frame) = frame_val {
                                                let frame = frame.borrow();
                                                let ff = frame.get(&crate::array::ArrayKey::String(PhpString::from_bytes(b"file")))
                                                    .map(|v| v.to_php_string().to_string_lossy())
                                                    .unwrap_or_default();
                                                let fl = frame.get(&crate::array::ArrayKey::String(PhpString::from_bytes(b"line")))
                                                    .map(|v| v.to_long())
                                                    .unwrap_or(0);
                                                let func = frame.get(&crate::array::ArrayKey::String(PhpString::from_bytes(b"function")))
                                                    .map(|v| v.to_php_string().to_string_lossy())
                                                    .unwrap_or_default();
                                                let cls = frame.get(&crate::array::ArrayKey::String(PhpString::from_bytes(b"class")))
                                                    .map(|v| v.to_php_string().to_string_lossy())
                                                    .unwrap_or_default();
                                                let typ = frame.get(&crate::array::ArrayKey::String(PhpString::from_bytes(b"type")))
                                                    .map(|v| v.to_php_string().to_string_lossy())
                                                    .unwrap_or_default();
                                                // Format args
                                                let args_str = if let Some(args_val) = frame.get(&crate::array::ArrayKey::String(PhpString::from_bytes(b"args"))) {
                                                    if let Value::Array(args_arr) = args_val {
                                                        let args_arr = args_arr.borrow();
                                                        let formatted: Vec<String> = args_arr.iter().map(|(_k, v)| {
                                                            Self::format_trace_arg(v)
                                                        }).collect();
                                                        formatted.join(", ")
                                                    } else {
                                                        String::new()
                                                    }
                                                } else {
                                                    String::new()
                                                };
                                                let loc = if ff.is_empty() {
                                                    "[internal function]".to_string()
                                                } else {
                                                    format!("{}({})", ff, fl)
                                                };
                                                lines.push(format!("#{} {}: {}{}{}({})", idx, loc, cls, typ, func, args_str));
                                            }
                                            idx += 1;
                                        }
                                        lines.push(format!("#{} {{main}}", idx));
                                        lines.join("\n")
                                    } else {
                                        "#0 {main}".to_string()
                                    };
                                    let result = if message.is_empty() {
                                        format!("{} in {}:{}\nStack trace:\n{}", class_display, file_str, line, trace_str)
                                    } else {
                                        format!("{}: {} in {}:{}\nStack trace:\n{}", class_display, message, file_str, line, trace_str)
                                    };
                                    Some(Value::String(PhpString::from_string(result)))
                                }
                                _ => None,
                            }
                        } else if !has_user_method {
                            // SPL class method dispatch
                            self.dispatch_spl_method(
                                &class_name_lower,
                                &method_name_lower,
                                obj,
                            )
                        } else {
                            None
                        };

                        if let Some(result) = builtin_result {
                            self.pending_calls.push(PendingCall {
                                name: PhpString::from_bytes(b"__builtin_return"),
                                args: vec![result],
                                named_args: Vec::new(),
                            });
                        } else if !has_user_method && self.is_spl_args_method(&class_name_lower, &method_name_lower) {
                            // SPL method that needs args - defer to DoCall with __spl:: prefix
                            let mut spl_name = b"__spl::".to_vec();
                            spl_name.extend_from_slice(&class_name_lower);
                            spl_name.extend_from_slice(b"::");
                            spl_name.extend_from_slice(&method_name_lower);
                            self.pending_calls.push(PendingCall {
                                name: PhpString::from_vec(spl_name),
                                args: vec![obj_val.clone()],
                                named_args: Vec::new(),
                            });
                        } else
                        // Find the method in the class
                        if let Some(class) = self.classes.get(&class_name_lower) {
                            if let Some(method) = class.get_method(&method_name_lower) {
                                // Check method visibility
                                let visibility_err = if method.visibility != Visibility::Public {
                                    let method_vis = method.visibility;
                                    let method_declaring = method.declaring_class.clone();
                                    let method_display_name = String::from_utf8_lossy(&method.name).to_string();
                                    let caller_scope = self.current_class_scope();
                                    self.check_visibility(
                                        method_vis,
                                        &method_declaring,
                                        &class_name_orig,
                                        &method_display_name,
                                        false,
                                        caller_scope.as_deref(),
                                    )
                                } else {
                                    None
                                };

                                if visibility_err.is_none() {
                                    // Method is accessible - set up the call
                                    // Create a synthetic function name for the pending call
                                    let mut func_name = class_name_orig.clone();
                                    func_name.extend_from_slice(b"::");
                                    func_name.extend_from_slice(&method.name);

                                    // Register the method as a temporary user function
                                    let call_name = PhpString::from_vec(func_name.clone());
                                    self.user_functions.insert(
                                        func_name.to_ascii_lowercase(),
                                        method.op_array.clone(),
                                    );

                                    // Push the pending call with $this as the first implicit arg
                                    // For static methods, don't pass $this
                                    let args = if method.is_static {
                                        vec![]
                                    } else {
                                        vec![obj_val.clone()] // $this is first arg, mapped to CV 0
                                    };
                                    self.pending_calls.push(PendingCall {
                                        name: call_name,
                                        args,
                                        named_args: Vec::new(),
                                    });
                                } else if let Some(call_method) = class.get_method(b"__call") {
                                    // Method exists but is not accessible - fall through to __call
                                    let mut func_name = class_name_orig.clone();
                                    func_name.extend_from_slice(b"::__call");
                                    let call_name = PhpString::from_vec(func_name.clone());
                                    self.user_functions.insert(
                                        func_name.to_ascii_lowercase(),
                                        call_method.op_array.clone(),
                                    );

                                    let method_name_val = Value::String(method_name.clone());
                                    self.pending_calls.push(PendingCall {
                                        name: call_name,
                                        args: vec![obj_val.clone(), method_name_val],
                                        named_args: Vec::new(),
                                    });
                                } else {
                                    // Method not accessible and no __call - throw error
                                    let err_msg = visibility_err.unwrap();
                                    let err_id = self.next_object_id;
                                    self.next_object_id += 1;
                                    let mut err_obj = PhpObject::new(b"Error".to_vec(), err_id);
                                    err_obj.set_property(
                                        b"message".to_vec(),
                                        Value::String(PhpString::from_string(err_msg.clone())),
                                    );
                                    err_obj.set_property(b"code".to_vec(), Value::Long(0));
                                    self.current_exception =
                                        Some(Value::Object(Rc::new(RefCell::new(err_obj))));
                                    if let Some((catch_target, _, _)) = exception_handlers.pop() {
                                        ip = catch_target as usize;
                                        continue;
                                    } else {
                                        return Err(VmError {
                                            message: format!("Uncaught Error: {}", err_msg),
                                            line: op.line,
                                        });
                                    }
                                }
                            } else if method_name_lower == b"__construct" {
                                // No explicit constructor - silently succeed
                                // (handled in DoFCall as Class::__construct fallback)
                                let mut func_name = class_name_orig.clone();
                                func_name.extend_from_slice(b"::__construct");
                                self.pending_calls.push(PendingCall {
                                    name: PhpString::from_vec(func_name),
                                    args: vec![obj_val.clone()],
                                    named_args: Vec::new(),
                                });
                            } else if let Some(call_method) = class.get_method(b"__call") {
                                // __call magic method fallback
                                let mut func_name = class_name_orig.clone();
                                func_name.extend_from_slice(b"::__call");
                                let call_name = PhpString::from_vec(func_name.clone());
                                self.user_functions.insert(
                                    func_name.to_ascii_lowercase(),
                                    call_method.op_array.clone(),
                                );

                                // Build args array for __call($name, $args)
                                // $this is CV[0], method name is CV[1], args array is CV[2]
                                let method_name_val = Value::String(method_name.clone());
                                // Args will be added by SendVal opcodes, collected in DoFCall
                                self.pending_calls.push(PendingCall {
                                    name: call_name,
                                    args: vec![obj_val.clone(), method_name_val],
                                    named_args: Vec::new(),
                                });
                            } else {
                                // Method not found - push call with class-qualified name
                                let mut func_name = class_name_orig.clone();
                                func_name.extend_from_slice(b"::");
                                func_name.extend_from_slice(method_name.as_bytes());
                                self.pending_calls.push(PendingCall {
                                    name: PhpString::from_vec(func_name),
                                    args: vec![obj_val.clone()],
                                    named_args: Vec::new(),
                                });
                            }
                        } else {
                            // Class not found in class table - push call with $this
                            self.pending_calls.push(PendingCall {
                                name: method_name,
                                args: vec![obj_val.clone()],
                                named_args: Vec::new(),
                            });
                        }
                    } else if let Value::Generator(gen_rc) = &obj_val {
                        // Generator method calls: current(), next(), valid(), key(), rewind(), send()
                        let method_lower: Vec<u8> = method_name
                            .as_bytes()
                            .iter()
                            .map(|b| b.to_ascii_lowercase())
                            .collect();

                        let result = match method_lower.as_slice() {
                            b"current" => {
                                let gen_borrow = gen_rc.borrow();
                                if gen_borrow.state == crate::generator::GeneratorState::Created {
                                    drop(gen_borrow);
                                    // Need to advance to first yield
                                    let mut gen_borrow = gen_rc.borrow_mut();
                                    let _ = gen_borrow.resume(self);
                                    gen_borrow.current_value.clone()
                                } else {
                                    gen_borrow.current_value.clone()
                                }
                            }
                            b"key" => {
                                let gen_borrow = gen_rc.borrow();
                                if gen_borrow.state == crate::generator::GeneratorState::Created {
                                    drop(gen_borrow);
                                    let mut gen_borrow = gen_rc.borrow_mut();
                                    let _ = gen_borrow.resume(self);
                                    gen_borrow.current_key.clone()
                                } else {
                                    gen_borrow.current_key.clone()
                                }
                            }
                            b"next" => {
                                let mut gen_borrow = gen_rc.borrow_mut();
                                if gen_borrow.state == crate::generator::GeneratorState::Created {
                                    // First call to next(): advance to first yield
                                    let _ = gen_borrow.resume(self);
                                }
                                // Then advance past current yield
                                gen_borrow.write_send_value();
                                let _ = gen_borrow.resume(self);
                                Value::Null
                            }
                            b"valid" => {
                                let gen_borrow = gen_rc.borrow();
                                if gen_borrow.state == crate::generator::GeneratorState::Created {
                                    drop(gen_borrow);
                                    let mut gen_borrow = gen_rc.borrow_mut();
                                    let _ = gen_borrow.resume(self);
                                    if gen_borrow.state
                                        == crate::generator::GeneratorState::Completed
                                    {
                                        Value::False
                                    } else {
                                        Value::True
                                    }
                                } else if gen_borrow.state
                                    == crate::generator::GeneratorState::Completed
                                {
                                    Value::False
                                } else {
                                    Value::True
                                }
                            }
                            b"rewind" => {
                                // In PHP, rewind on a started generator is a no-op / warning
                                Value::Null
                            }
                            b"send" => {
                                // send($value): resume the generator with a value
                                // The sent value becomes the result of the yield expression
                                Value::Null // Will be handled in DoFCall with args
                            }
                            b"getreturn" => {
                                let gen_borrow = gen_rc.borrow();
                                gen_borrow.return_value.clone()
                            }
                            _ => Value::Null,
                        };

                        // For send(), we need to pass through to DoFCall so args can be collected
                        if method_lower == b"send" {
                            self.pending_calls.push(PendingCall {
                                name: PhpString::from_bytes(b"__generator_send"),
                                args: vec![obj_val.clone()],
                                named_args: Vec::new(),
                            });
                        } else {
                            self.pending_calls.push(PendingCall {
                                name: PhpString::from_bytes(b"__builtin_return"),
                                args: vec![result],
                                named_args: Vec::new(),
                            });
                        }
                    } else {
                        // Check for closure methods on string/array values
                        let method_lower: Vec<u8> = method_name
                            .as_bytes()
                            .iter()
                            .map(|b| b.to_ascii_lowercase())
                            .collect();
                        match method_lower.as_slice() {
                            b"bindto" | b"bind" => {
                                // Closure::bindTo($newThis, $scope) - defer to DoFCall
                                self.pending_calls.push(PendingCall {
                                    name: PhpString::from_bytes(b"__closure_bindto"),
                                    args: vec![obj_val.clone()],
                                    named_args: Vec::new(),
                                });
                            }
                            b"call" => {
                                // Closure::call($newThis, ...$args) - bind and call
                                self.pending_calls.push(PendingCall {
                                    name: PhpString::from_bytes(b"__closure_call"),
                                    args: vec![obj_val.clone()],
                                    named_args: Vec::new(),
                                });
                            }
                            _ => {
                                // Not an object - throw "Call to a member function on <type>"
                                let type_name = Vm::value_type_name(&obj_val);
                                let method_str = method_name.to_string_lossy();
                                let err_msg = format!("Call to a member function {}() on {}", method_str, type_name);
                                let err_id = self.next_object_id;
                                self.next_object_id += 1;
                                let mut err_obj = PhpObject::new(b"Error".to_vec(), err_id);
                                err_obj.set_property(
                                    b"message".to_vec(),
                                    Value::String(PhpString::from_string(err_msg.clone())),
                                );
                                err_obj.set_property(b"code".to_vec(), Value::Long(0));
                                err_obj.set_property(b"file".to_vec(), Value::String(PhpString::from_bytes(b"")));
                                err_obj.set_property(b"line".to_vec(), Value::Long(op.line as i64));
                                self.current_exception = Some(Value::Object(Rc::new(RefCell::new(err_obj))));
                                if let Some((catch_target, _, _)) = exception_handlers.last() {
                                    ip = *catch_target as usize;
                                    continue;
                                } else {
                                    return Err(VmError {
                                        message: err_msg,
                                        line: op.line,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Convert a value to an array key (PHP coerces numeric strings to int keys)
    pub fn value_to_array_key(val: Value) -> ArrayKey {
        match val {
            Value::Long(n) => ArrayKey::Int(n),
            Value::String(s) => {
                // PHP converts numeric strings to integer keys
                let bytes = s.as_bytes();
                let s_str = s.to_string_lossy();
                let trimmed = s_str.trim();
                if !trimmed.is_empty()
                    && let Ok(n) = trimmed.parse::<i64>()
                {
                    // Only convert if the string is exactly the integer representation
                    if n.to_string() == trimmed {
                        return ArrayKey::Int(n);
                    }
                }
                ArrayKey::String(s)
            }
            Value::Double(f) => ArrayKey::Int(f as i64),
            Value::True => ArrayKey::Int(1),
            Value::False | Value::Null | Value::Undef => ArrayKey::Int(0),
            Value::Object(_) | Value::Array(_) | Value::Generator(_) => ArrayKey::Int(0),
            Value::Reference(r) => Self::value_to_array_key(r.borrow().clone()),
        }
    }

    /// Convert a value to string, calling __toString for objects if available
    pub fn value_to_string(&mut self, val: &Value) -> PhpString {
        if let Value::Object(obj) = val {
            let class_lower: Vec<u8> = obj
                .borrow()
                .class_name
                .iter()
                .map(|b| b.to_ascii_lowercase())
                .collect();
            let has_tostring = self
                .classes
                .get(&class_lower)
                .map(|c| c.methods.contains_key(&b"__tostring".to_vec()))
                .unwrap_or(false);
            if has_tostring {
                let method = self
                    .classes
                    .get(&class_lower)
                    .unwrap()
                    .get_method(b"__tostring")
                    .unwrap()
                    .op_array
                    .clone();
                let mut method_cvs = vec![Value::Undef; method.cv_names.len()];
                if !method_cvs.is_empty() {
                    method_cvs[0] = val.clone();
                }
                if let Ok(result) = self.execute_op_array(&method, method_cvs) {
                    return result.to_php_string();
                }
            }
            // Built-in __toString for Throwable classes (Exception, Error, etc.)
            let is_throwable = class_lower == b"exception"
                || class_lower == b"error"
                || is_builtin_subclass(&class_lower, b"exception")
                || is_builtin_subclass(&class_lower, b"error")
                || self.class_extends(&class_lower, b"exception")
                || self.class_extends(&class_lower, b"error");
            if is_throwable {
                let obj_borrow = obj.borrow();
                let class_display = String::from_utf8_lossy(&obj_borrow.class_name).to_string();
                let message = obj_borrow.get_property(b"message").to_php_string().to_string_lossy();
                let file = obj_borrow.get_property(b"file").to_php_string().to_string_lossy();
                let line = obj_borrow.get_property(b"line").to_long();
                let file_str = if file.is_empty() { self.current_file.clone() } else { file };
                let result = if message.is_empty() {
                    format!("{} in {}:{}\nStack trace:\n#0 {{main}}", class_display, file_str, line)
                } else {
                    format!("{}: {} in {}:{}\nStack trace:\n#0 {{main}}", class_display, message, file_str, line)
                };
                return PhpString::from_string(result);
            }
        }
        val.to_php_string()
    }

    fn read_operand(
        &self,
        operand: &OperandType,
        cvs: &[Value],
        tmps: &[Value],
        literals: &[Value],
    ) -> Value {
        match operand {
            OperandType::Cv(idx) => {
                let val = cvs.get(*idx as usize).cloned().unwrap_or(Value::Null);
                // Auto-deref references when reading
                val.deref()
            }
            OperandType::Const(idx) => literals.get(*idx as usize).cloned().unwrap_or(Value::Null),
            OperandType::Tmp(idx) => tmps.get(*idx as usize).cloned().unwrap_or(Value::Null),
            OperandType::Unused => Value::Null,
            OperandType::JmpTarget(_) => Value::Null,
        }
    }

    /// Read an operand, emitting "Undefined variable" warning for undef CVs
    fn read_operand_warn(
        &mut self,
        operand: &OperandType,
        cvs: &[Value],
        tmps: &[Value],
        literals: &[Value],
        op_array: &OpArray,
        line: u32,
    ) -> Value {
        if let OperandType::Cv(idx) = operand {
            let i = *idx as usize;
            if let Some(val) = cvs.get(i) {
                let is_undef = match val {
                    Value::Undef => true,
                    Value::Reference(r) => matches!(*r.borrow(), Value::Undef),
                    _ => false,
                };
                if is_undef {
                    if let Some(name) = op_array.cv_names.get(i) {
                        // Don't warn for superglobals
                        if name != b"GLOBALS" && name != b"_SERVER" && name != b"_GET"
                            && name != b"_POST" && name != b"_COOKIE" && name != b"_FILES"
                            && name != b"_REQUEST" && name != b"_SESSION" && name != b"_ENV"
                        {
                            let varname = String::from_utf8_lossy(name);
                            self.emit_warning_at(&format!("Undefined variable ${}", varname), line);
                        }
                    }
                    return Value::Null;
                }
            }
        }
        self.read_operand(operand, cvs, tmps, literals)
    }

    /// Read a CV value without dereferencing (keeps Reference wrapper)
    fn read_operand_raw(cvs: &[Value], operand: &OperandType) -> Value {
        match operand {
            OperandType::Cv(idx) => cvs.get(*idx as usize).cloned().unwrap_or(Value::Null),
            _ => Value::Null,
        }
    }

    /// Check if a CV operand refers to an undefined variable and emit a warning if so.
    /// This implements PHP's "Warning: Undefined variable $name" behavior.
    fn check_undefined_cv(
        &mut self,
        operand: &OperandType,
        cvs: &[Value],
        op_array: &OpArray,
        line: u32,
    ) {
        if let OperandType::Cv(idx) = operand {
            let i = *idx as usize;
            if let Some(val) = cvs.get(i) {
                let is_undef = match val {
                    Value::Undef => true,
                    Value::Reference(r) => matches!(*r.borrow(), Value::Undef),
                    _ => false,
                };
                if is_undef {
                    if let Some(name) = op_array.cv_names.get(i) {
                        let varname = String::from_utf8_lossy(name);
                        self.emit_warning_at(&format!("Undefined variable ${}", varname), line);
                    }
                }
            }
        }
    }

    fn write_operand(
        &mut self,
        operand: &OperandType,
        value: Value,
        cvs: &mut [Value],
        tmps: &mut [Value],
        static_cv_keys: &HashMap<u32, Vec<u8>>,
    ) {
        match operand {
            OperandType::Cv(idx) => {
                // PHP copy-on-write: when assigning an array to a CV,
                // clone the inner PhpArray so each variable has its own copy.
                let value = match &value {
                    Value::Array(arr) => Value::Array(Rc::new(RefCell::new(arr.borrow().clone()))),
                    _ => value,
                };
                if let Some(slot) = cvs.get_mut(*idx as usize) {
                    // If the CV holds a reference, write through the reference
                    if let Value::Reference(r) = slot {
                        *r.borrow_mut() = value.clone();
                    } else {
                        *slot = value.clone();
                    }
                }
                // If this CV is a static variable, persist the value
                if let Some(key) = static_cv_keys.get(idx) {
                    self.static_vars.insert(key.clone(), value);
                }
            }
            OperandType::Tmp(idx) => {
                if let Some(slot) = tmps.get_mut(*idx as usize) {
                    *slot = value;
                }
            }
            _ => {}
        }
    }

    // ---- Generator support methods ----
    // These methods are called by the generator executor to interact with the VM

    /// Increment call depth and check for overflow (used by generator resume to prevent stack overflow)
    pub fn enter_generator_resume(&mut self, line: u32) -> Result<(), VmError> {
        self.call_depth += 1;
        if self.call_depth > 100 {
            self.call_depth -= 1;
            return Err(VmError {
                message: "Maximum call depth exceeded (possible infinite recursion)".into(),
                line,
            });
        }
        Ok(())
    }

    /// Decrement call depth after generator resume completes
    pub fn leave_generator_resume(&mut self) {
        self.call_depth -= 1;
    }

    /// Initialize a function call from generator context
    pub fn generator_init_fcall(&mut self, name_val: Value) {
        if let Value::Array(arr) = &name_val {
            let arr = arr.borrow();
            let mut values: Vec<Value> = arr.values().cloned().collect();
            if !values.is_empty() {
                let name = values.remove(0).to_php_string();
                self.pending_calls.push(PendingCall {
                    name,
                    args: values,
                    named_args: Vec::new(),
                });
            } else {
                self.pending_calls.push(PendingCall {
                    name: PhpString::empty(),
                    args: Vec::new(),
                    named_args: Vec::new(),
                });
            }
        } else {
            let name = name_val.to_php_string();
            self.pending_calls.push(PendingCall {
                name,
                args: Vec::new(),
                named_args: Vec::new(),
            });
        }
    }

    /// Send a value to a pending function call from generator context
    pub fn generator_send_val(&mut self, val: Value) {
        if let Some(call) = self.pending_calls.last_mut() {
            call.args.push(val);
        }
    }

    /// Send a named value to a pending function call from generator context
    pub fn generator_send_named_val(&mut self, name: Vec<u8>, val: Value) {
        if let Some(call) = self.pending_calls.last_mut() {
            call.named_args.push((name, val));
        }
    }

    /// Execute a pending function call from generator context
    pub fn generator_do_fcall(&mut self, line: u32) -> Result<Value, VmError> {
        let mut call = self.pending_calls.pop().ok_or_else(|| VmError {
            message: "no pending function call".into(),
            line,
        })?;

        let func_name_lower: Vec<u8> = call
            .name
            .as_bytes()
            .iter()
            .map(|b| b.to_ascii_lowercase())
            .collect();

        if func_name_lower == b"__builtin_return" {
            Ok(call.args.first().cloned().unwrap_or(Value::Null))
        } else if let Some(func) = self.functions.get(&func_name_lower).copied() {
            func(self, &call.args).map_err(|e| VmError {
                message: e.message,
                line,
            })
        } else if let Some(user_fn) = self.user_functions.get(&func_name_lower).cloned() {
            // Resolve named arguments by reordering to match parameter positions
            if !call.named_args.is_empty() {
                let implicit_args_count =
                    if user_fn.cv_names.first().map(|n| n.as_slice()) == Some(b"this") {
                        1
                    } else {
                        0
                    };
                if let Err(err_msg) = call.resolve_named_args(&user_fn.cv_names, implicit_args_count, user_fn.variadic_param) {
                    let exc_val = self.create_exception(b"Error", &err_msg, line);
                    self.current_exception = Some(exc_val);
                    return Err(VmError {
                        message: format!("Uncaught Error: {}", err_msg),
                        line,
                    });
                }
            }

            // Check if this user function is a generator
            if user_fn.is_generator {
                let mut func_cvs = vec![Value::Undef; user_fn.cv_names.len()];
                for (i, arg) in call.args.iter().enumerate() {
                    if i < func_cvs.len() {
                        func_cvs[i] = arg.clone();
                    }
                }
                let generator = crate::generator::PhpGenerator::new(user_fn, func_cvs);
                let gen_rc = Rc::new(RefCell::new(generator));
                Ok(Value::Generator(gen_rc))
            } else {
                let was_global = self.is_global_scope;
                self.is_global_scope = false;
                let mut func_cvs = vec![Value::Undef; user_fn.cv_names.len()];
                if let Some(variadic_idx) = user_fn.variadic_param {
                    let vi = variadic_idx as usize;
                    for (i, arg) in call.args.iter().enumerate() {
                        if i < vi && i < func_cvs.len() {
                            func_cvs[i] = arg.clone();
                        }
                    }
                    let mut variadic_arr = crate::array::PhpArray::new();
                    for arg in call.args.iter().skip(vi) {
                        variadic_arr.push(arg.clone());
                    }
                    if vi < func_cvs.len() {
                        func_cvs[vi] = Value::Array(Rc::new(RefCell::new(variadic_arr)));
                    }
                } else {
                    for (i, arg) in call.args.iter().enumerate() {
                        if i < func_cvs.len() {
                            func_cvs[i] = arg.clone();
                        }
                    }
                }
                let result = self.execute_op_array(&user_fn, func_cvs);
                self.is_global_scope = was_global;
                result
            }
        } else {
            let name_bytes = call.name.as_bytes();
            if name_bytes.ends_with(b"::__construct") || name_bytes == b"__construct" {
                Ok(Value::Null)
            } else {
                Err(VmError {
                    message: format!(
                        "Call to undefined function {}()",
                        call.name.to_string_lossy()
                    ),
                    line,
                })
            }
        }
    }

    /// Look up a constant value
    pub fn lookup_constant(&self, name: &[u8]) -> Value {
        self.constants
            .get(name)
            .cloned()
            .unwrap_or_else(|| Value::String(PhpString::from_vec(name.to_vec())))
    }

    /// Get a static variable value
    pub fn get_static_var(&self, key: &[u8]) -> Option<Value> {
        self.static_vars.get(key).cloned()
    }

    /// Set a static variable value
    pub fn set_static_var(&mut self, key: Vec<u8>, value: Value) {
        self.static_vars.insert(key, value);
    }

    /// Get a global variable value
    pub fn get_global(&self, name: &[u8]) -> Option<Value> {
        self.globals.get(name).cloned()
    }
}

impl Default for Vm {
    fn default() -> Self {
        Self::new()
    }
}

/// Check if a built-in class is a subclass of another built-in class
/// PHP exception hierarchy:
///   Throwable
///   ├─ Error
///   │  ├─ TypeError
///   │  ├─ ValueError
///   │  ├─ ArithmeticError
///   │  │  └─ DivisionByZeroError
///   │  ├─ ArgumentCountError
///   │  ├─ RangeError
///   │  └─ UnhandledMatchError
///   └─ Exception
///      ├─ RuntimeException
///      │  ├─ OverflowException
///      │  └─ UnderflowException
///      ├─ LogicException
///      │  ├─ InvalidArgumentException
///      │  ├─ BadMethodCallException
///      │  │  └─ BadFunctionCallException
///      │  ├─ DomainException
///      │  └─ UnexpectedValueException
///      └─ ClosedGeneratorException
pub fn is_builtin_subclass(child: &[u8], parent: &[u8]) -> bool {
    // Check SPL interface implementation
    if is_builtin_implements(child, parent) {
        return true;
    }
    // Get the parent chain for the child class
    let parents = builtin_parent_chain(child);
    if parents.iter().any(|p| p == parent) {
        return true;
    }
    // Also check if any parent class implements the interface
    for p in &parents {
        if is_builtin_implements(p, parent) {
            return true;
        }
    }
    false
}

/// Get all interfaces that a built-in class implements
pub fn get_builtin_interfaces(class: &[u8]) -> Vec<Vec<u8>> {
    let interfaces: &[&[u8]] = match class {
        b"arrayobject" | b"arrayiterator" | b"recursivearrayiterator" => &[
            b"IteratorAggregate", b"Traversable", b"ArrayAccess", b"Serializable", b"Countable",
        ],
        b"splfixedarray" => &[b"Iterator", b"Traversable", b"ArrayAccess", b"Countable"],
        b"spldoublylinkedlist" => &[b"Iterator", b"Traversable", b"Countable", b"Serializable", b"ArrayAccess"],
        b"splstack" | b"splqueue" => &[b"Iterator", b"Traversable", b"Countable", b"Serializable", b"ArrayAccess"],
        b"splobjectstorage" => &[b"Countable", b"Iterator", b"Traversable", b"Serializable", b"ArrayAccess"],
        b"splpriorityqueue" => &[b"Iterator", b"Traversable", b"Countable"],
        b"splheap" | b"splminheap" | b"splmaxheap" => &[b"Iterator", b"Traversable", b"Countable"],
        b"exception" | b"error" | b"typeerror" | b"valueerror" | b"argumentcounterror"
        | b"rangeerror" | b"arithmeticerror" | b"divisionbyzeroerror" | b"assertionerror"
        | b"unhandledmatcherror" | b"runtimeexception" | b"logicexception"
        | b"invalidargumentexception" | b"badmethodcallexception" | b"badfunctioncallexception"
        | b"overflowexception" | b"underflowexception" | b"outofboundsexception"
        | b"domainexception" | b"unexpectedvalueexception" | b"lengthexception"
        | b"outofrangeexception" | b"closedgeneratorexception" | b"errorexception" => &[b"Throwable"],
        _ => &[],
    };
    interfaces.iter().map(|i| i.to_vec()).collect()
}

/// Get the built-in parent class name for a class (lowercase)
pub fn get_builtin_parent(class: &[u8]) -> Option<&'static [u8]> {
    match class {
        b"typeerror" | b"valueerror" | b"argumentcounterror" | b"rangeerror"
        | b"unhandledmatcherror" | b"assertionerror" => Some(b"Error"),
        b"arithmeticerror" => Some(b"Error"),
        b"divisionbyzeroerror" => Some(b"ArithmeticError"),
        b"runtimeexception" | b"logicexception" | b"closedgeneratorexception" | b"errorexception" => Some(b"Exception"),
        b"overflowexception" | b"underflowexception" | b"outofboundsexception" => Some(b"RuntimeException"),
        b"invalidargumentexception" | b"badmethodcallexception" | b"domainexception"
        | b"unexpectedvalueexception" | b"lengthexception" | b"outofrangeexception" => Some(b"LogicException"),
        b"badfunctioncallexception" => Some(b"BadMethodCallException"),
        b"splstack" | b"splqueue" => Some(b"SplDoublyLinkedList"),
        b"splminheap" | b"splmaxheap" => Some(b"SplHeap"),
        _ => None,
    }
}

/// Canonicalize a class name from lowercase to proper case
pub fn canonicalize_class_name(name_lower: &[u8]) -> Vec<u8> {
    match name_lower {
        b"stdclass" => b"stdClass".to_vec(),
        b"exception" => b"Exception".to_vec(),
        b"error" => b"Error".to_vec(),
        b"typeerror" => b"TypeError".to_vec(),
        b"valueerror" => b"ValueError".to_vec(),
        b"runtimeexception" => b"RuntimeException".to_vec(),
        b"logicexception" => b"LogicException".to_vec(),
        b"invalidargumentexception" => b"InvalidArgumentException".to_vec(),
        b"badmethodcallexception" => b"BadMethodCallException".to_vec(),
        b"badfunctioncallexception" => b"BadFunctionCallException".to_vec(),
        b"overflowexception" => b"OverflowException".to_vec(),
        b"underflowexception" => b"UnderflowException".to_vec(),
        b"rangeerror" => b"RangeError".to_vec(),
        b"arithmeticerror" => b"ArithmeticError".to_vec(),
        b"divisionbyzeroerror" => b"DivisionByZeroError".to_vec(),
        b"argumentcounterror" => b"ArgumentCountError".to_vec(),
        b"errorexception" => b"ErrorException".to_vec(),
        b"closedgeneratorexception" => b"ClosedGeneratorException".to_vec(),
        b"unexpectedvalueexception" => b"UnexpectedValueException".to_vec(),
        b"domainexception" => b"DomainException".to_vec(),
        b"assertionerror" => b"AssertionError".to_vec(),
        b"unhandledmatcherror" => b"UnhandledMatchError".to_vec(),
        b"lengthexception" => b"LengthException".to_vec(),
        b"outofrangeexception" => b"OutOfRangeException".to_vec(),
        b"outofboundsexception" => b"OutOfBoundsException".to_vec(),
        b"arrayobject" => b"ArrayObject".to_vec(),
        b"arrayiterator" => b"ArrayIterator".to_vec(),
        b"splfixedarray" => b"SplFixedArray".to_vec(),
        b"spldoublylinkedlist" => b"SplDoublyLinkedList".to_vec(),
        b"splstack" => b"SplStack".to_vec(),
        b"splqueue" => b"SplQueue".to_vec(),
        b"splpriorityqueue" => b"SplPriorityQueue".to_vec(),
        b"splmaxheap" => b"SplMaxHeap".to_vec(),
        b"splminheap" => b"SplMinHeap".to_vec(),
        b"splheap" => b"SplHeap".to_vec(),
        b"splobjectstorage" => b"SplObjectStorage".to_vec(),
        _ => name_lower.to_vec(),
    }
}

fn is_builtin_implements(class: &[u8], interface: &[u8]) -> bool {
    match class {
        b"arrayobject" | b"arrayiterator" | b"recursivearrayiterator" => matches!(
            interface,
            b"iteratoraggregate" | b"traversable" | b"arrayaccess" | b"serializable" | b"countable"
        ),
        b"splfixedarray" => matches!(
            interface,
            b"iterator" | b"traversable" | b"arrayaccess" | b"countable"
        ),
        b"spldoublylinkedlist" => matches!(
            interface,
            b"iterator" | b"traversable" | b"countable" | b"serializable" | b"arrayaccess"
        ),
        b"splstack" | b"splqueue" => matches!(
            interface,
            b"iterator" | b"traversable" | b"countable" | b"serializable" | b"arrayaccess"
        ),
        b"splobjectstorage" => matches!(
            interface,
            b"countable" | b"iterator" | b"traversable" | b"serializable" | b"arrayaccess"
        ),
        b"splpriorityqueue" => matches!(
            interface,
            b"iterator" | b"traversable" | b"countable"
        ),
        b"splheap" | b"splminheap" | b"splmaxheap" => matches!(
            interface,
            b"iterator" | b"traversable" | b"countable"
        ),
        _ => false,
    }
}

fn builtin_parent_chain(class: &[u8]) -> Vec<Vec<u8>> {
    let mut chain = Vec::new();
    let mut current = class.to_vec();
    loop {
        let parent = match current.as_slice() {
            // Error hierarchy
            b"typeerror"
            | b"valueerror"
            | b"argumentcounterror"
            | b"rangeerror"
            | b"unhandledmatcherror"
            | b"assertionerror" => Some(b"error".to_vec()),
            b"arithmeticerror" => Some(b"error".to_vec()),
            b"divisionbyzeroerror" => Some(b"arithmeticerror".to_vec()),
            b"error" => Some(b"throwable".to_vec()),
            // Exception hierarchy
            b"runtimeexception" | b"logicexception" | b"closedgeneratorexception" => {
                Some(b"exception".to_vec())
            }
            b"overflowexception" | b"underflowexception" | b"outofboundsexception" => {
                Some(b"runtimeexception".to_vec())
            }
            b"invalidargumentexception"
            | b"badmethodcallexception"
            | b"domainexception"
            | b"unexpectedvalueexception"
            | b"lengthexception"
            | b"outofrangeexception" => Some(b"logicexception".to_vec()),
            b"badfunctioncallexception" => Some(b"badmethodcallexception".to_vec()),
            b"errorexception" => Some(b"exception".to_vec()),
            b"exception" => Some(b"throwable".to_vec()),
            // SPL class hierarchy
            b"splstack" | b"splqueue" => Some(b"spldoublylinkedlist".to_vec()),
            b"splminheap" | b"splmaxheap" => Some(b"splheap".to_vec()),
            _ => None,
        };
        if let Some(p) = parent {
            chain.push(p.clone());
            current = p;
        } else {
            break;
        }
    }
    chain
}

/// Shift a CV operand index by +1 (used when inserting $this as CV[0] in bound closures)
fn shift_cv_operand(operand: &mut OperandType) {
    if let OperandType::Cv(idx) = operand {
        *idx += 1;
    }
}

/// Remap CV operand when moving CV[old_pos] to CV[0].
/// CV[old_pos] -> CV[0], CV[0..old_pos-1] -> CV[1..old_pos]
fn remap_cv_operand(operand: &mut OperandType, old_pos: u32) {
    if let OperandType::Cv(idx) = operand {
        if *idx == old_pos {
            *idx = 0;
        } else if *idx < old_pos {
            *idx += 1;
        }
        // CVs after old_pos stay the same
    }
}

/// PHP increment: for strings, follows alphabetic increment rules
fn php_increment(val: &Value) -> Value {
    match val {
        Value::Long(n) => match n.checked_add(1) {
            Some(r) => Value::Long(r),
            None => Value::Double(*n as f64 + 1.0),
        },
        Value::Double(f) => Value::Double(f + 1.0),
        Value::String(s) => {
            let bytes = s.as_bytes();
            // Check if it's a numeric string first (before alphanumeric check)
            if let Some(n) = crate::value::parse_numeric_string(bytes) {
                if n.fract() == 0.0 && n >= i64::MIN as f64 && n <= i64::MAX as f64 {
                    return match (n as i64).checked_add(1) {
                        Some(r) => Value::Long(r),
                        None => Value::Double(n + 1.0),
                    };
                }
                return Value::Double(n + 1.0);
            }
            // Empty string: becomes "1" (PHP 8.3+ emits Deprecated)
            if bytes.is_empty() {
                return Value::Long(1);
            }
            // Check if string has any alphanumeric characters
            let has_alnum = bytes.iter().any(|b| b.is_ascii_alphanumeric());
            if !has_alnum {
                // No alphanumeric characters (e.g. " ", "!", "🐘"): no change
                return val.clone();
            }
            // Alphabetic increment: "a" -> "b", "z" -> "aa", "Az" -> "Ba"
            // For mixed strings like "Hello world", only increment alphanumeric chars
            let mut result: Vec<u8> = bytes.to_vec();
            let mut carry = true;
            for i in (0..result.len()).rev() {
                if !carry {
                    break;
                }
                // Skip non-alphanumeric characters
                if !result[i].is_ascii_alphanumeric() {
                    continue;
                }
                carry = false;
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
                    b'a'..=b'y' | b'A'..=b'Y' | b'0'..=b'8' => {
                        result[i] += 1;
                    }
                    _ => {
                        result[i] += 1;
                    }
                }
            }
            if carry {
                // Need to prepend: "z" -> "aa", "Z" -> "AA", "9" -> "10"
                let prefix = match result[0] {
                    b'a'..=b'z' => b'a',
                    b'A'..=b'Z' => b'A',
                    _ => b'1',
                };
                result.insert(0, prefix);
            }
            Value::String(PhpString::from_vec(result))
        }
        Value::Null | Value::Undef => Value::Long(1),
        Value::False => Value::False, // false++ has no effect (stays false) in PHP 8.3+
        Value::True => Value::True,   // true++ stays true
        _ => val.add(&Value::Long(1)),
    }
}

/// PHP decrement: numeric strings are decremented, non-numeric stay the same
fn php_decrement(val: &Value) -> Value {
    match val {
        Value::Long(n) => match n.checked_sub(1) {
            Some(r) => Value::Long(r),
            None => Value::Double(*n as f64 - 1.0),
        },
        Value::Double(f) => Value::Double(f - 1.0),
        Value::String(s) => {
            let bytes = s.as_bytes();
            // Check if it's a numeric string
            if let Some(n) = crate::value::parse_numeric_string(bytes) {
                if n.fract() == 0.0 && n >= i64::MIN as f64 && n <= i64::MAX as f64 {
                    match (n as i64).checked_sub(1) {
                        Some(r) => return Value::Long(r),
                        None => return Value::Double(n - 1.0),
                    }
                }
                return Value::Double(n - 1.0);
            }
            // Non-numeric string: decrement has no effect
            val.clone()
        }
        Value::Null | Value::Undef => Value::Null, // null-- stays null
        Value::True => Value::True,   // true-- has no effect in PHP 8.3+
        Value::False => Value::False, // false-- has no effect in PHP 8.3+
        _ => val.sub(&Value::Long(1)),
    }
}

/// Format arguments for stack trace display
pub fn format_trace_args(args: &[Value]) -> String {
    args.iter()
        .map(|v| Vm::format_trace_arg(v))
        .collect::<Vec<_>>()
        .join(", ")
}
