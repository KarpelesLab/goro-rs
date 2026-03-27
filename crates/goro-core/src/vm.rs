use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use indexmap::IndexMap;

use crate::array::{ArrayKey, PhpArray};
use crate::object::{ClassEntry, MethodDef, PhpObject, PropertyDef, Visibility};
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
    /// Class table (preserves insertion order)
    pub classes: IndexMap<Vec<u8>, ClassEntry>,
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
    /// Cached enum case singleton objects: key = "classname_lower::CaseName"
    pub enum_case_cache: HashMap<Vec<u8>, Value>,
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
            classes: IndexMap::new(),
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
            enum_case_cache: HashMap::new(),
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
                // mt_rand constants
                c.insert(b"MT_RAND_MT19937".to_vec(), Value::Long(0));
                c.insert(b"MT_RAND_PHP".to_vec(), Value::Long(1));
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
                // Round constants
                c.insert(b"PHP_ROUND_HALF_UP".to_vec(), Value::Long(0));
                c.insert(b"PHP_ROUND_HALF_DOWN".to_vec(), Value::Long(1));
                c.insert(b"PHP_ROUND_HALF_EVEN".to_vec(), Value::Long(2));
                c.insert(b"PHP_ROUND_HALF_ODD".to_vec(), Value::Long(3));
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
                c.insert(b"JSON_ERROR_NON_BACKED_ENUM".to_vec(), Value::Long(11));
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
    /// Returns (visibility, declaring_class, is_readonly, property_type).
    fn find_property_def(&self, class_name_lower: &[u8], prop_name: &[u8]) -> Option<(Visibility, Vec<u8>, bool, Option<crate::opcode::ParamType>)> {
        self.find_property_def_for_scope(class_name_lower, prop_name, None)
    }

    /// Find a property definition, optionally considering the caller scope for private property resolution.
    /// When caller_scope is provided, private properties from other classes are skipped
    /// so that a parent can access its own private property on a child object.
    fn find_property_def_for_scope(&self, class_name_lower: &[u8], prop_name: &[u8], caller_scope: Option<&[u8]>) -> Option<(Visibility, Vec<u8>, bool, Option<crate::opcode::ParamType>)> {
        let mut current = class_name_lower.to_vec();
        let mut first_match = None;
        for _ in 0..50 {
            if let Some(class) = self.classes.get(&current) {
                for prop in &class.properties {
                    if prop.name == prop_name {
                        if prop.visibility == Visibility::Private {
                            if let Some(scope) = caller_scope {
                                let declaring_lower: Vec<u8> = prop.declaring_class.iter().map(|b| b.to_ascii_lowercase()).collect();
                                if declaring_lower == scope {
                                    // Exact match for private property in caller's scope
                                    return Some((prop.visibility, prop.declaring_class.clone(), prop.is_readonly, prop.property_type.clone()));
                                }
                                // Save first match but keep looking for a scope match
                                if first_match.is_none() {
                                    first_match = Some((prop.visibility, prop.declaring_class.clone(), prop.is_readonly, prop.property_type.clone()));
                                }
                            } else {
                                return Some((prop.visibility, prop.declaring_class.clone(), prop.is_readonly, prop.property_type.clone()));
                            }
                        } else {
                            return Some((prop.visibility, prop.declaring_class.clone(), prop.is_readonly, prop.property_type.clone()));
                        }
                    }
                }
                if let Some(parent) = &class.parent {
                    current = parent.iter().map(|b| b.to_ascii_lowercase()).collect();
                } else {
                    return first_match;
                }
            } else {
                return first_match;
            }
        }
        first_match
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

    /// Emit object-to-scalar comparison notice when comparing object to int/float/string
    /// PHP 8 emits: Notice: Object of class X could not be converted to int/float
    /// Emit "A non-numeric value encountered" warning for leading-numeric strings in arithmetic
    fn check_leading_numeric_warning(&mut self, v: &Value, line: u32) {
        let inner = match v {
            Value::Reference(r) => r.borrow().clone(),
            _ => v.clone(),
        };
        if let Value::String(s) = &inner {
            let bytes = s.as_bytes();
            if bytes.is_empty() {
                return;
            }
            let trimmed = std::str::from_utf8(bytes).unwrap_or("").trim();
            if trimmed.is_empty() {
                return;
            }
            let first = trimmed.as_bytes()[0];
            let starts_numeric = first.is_ascii_digit() || first == b'.'
                || ((first == b'+' || first == b'-') && trimmed.len() > 1
                    && (trimmed.as_bytes()[1].is_ascii_digit() || trimmed.as_bytes()[1] == b'.'));
            if starts_numeric {
                // Check if it's a leading-numeric string (has trailing non-numeric chars)
                if crate::value::parse_numeric_string(bytes).is_none() {
                    // It starts numeric but isn't fully numeric -> leading numeric
                    self.emit_warning_at("A non-numeric value encountered", line);
                }
            }
        }
    }

    fn emit_object_comparison_notice(&mut self, a: &Value, b: &Value, line: u32) {
        let a_inner = match a {
            Value::Reference(r) => r.borrow().clone(),
            _ => a.clone(),
        };
        let b_inner = match b {
            Value::Reference(r) => r.borrow().clone(),
            _ => b.clone(),
        };
        match (&a_inner, &b_inner) {
            (Value::Object(obj), Value::Long(_)) | (Value::Long(_), Value::Object(obj)) => {
                let class_name = String::from_utf8_lossy(&obj.borrow().class_name).to_string();
                self.emit_notice_at(
                    &format!("Object of class {} could not be converted to int", class_name),
                    line,
                );
            }
            (Value::Object(obj), Value::Double(_)) | (Value::Double(_), Value::Object(obj)) => {
                let class_name = String::from_utf8_lossy(&obj.borrow().class_name).to_string();
                self.emit_notice_at(
                    &format!("Object of class {} could not be converted to float", class_name),
                    line,
                );
            }
            _ => {}
        }
    }

    /// Emit a PHP deprecated warning
    pub fn emit_deprecated(&mut self, msg: &str) {
        let line = self.current_line;
        self.emit_deprecated_at(msg, line);
    }

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

        // PHP 8: arithmetic ops on fully non-numeric strings throw TypeError
        // Leading-numeric strings (like "45some") produce a Warning but NOT a TypeError
        {
            // Helper: check if string is fully non-numeric
            let is_fully_non_numeric_str = |v: &Value| -> bool {
                if let Value::String(s) = v {
                    let bytes = s.as_bytes();
                    if bytes.is_empty() {
                        return false; // empty string is treated as 0, not an error
                    }
                    let trimmed = std::str::from_utf8(bytes).unwrap_or("").trim();
                    if trimmed.is_empty() {
                        return false; // whitespace-only string is treated as 0
                    }
                    let first = trimmed.as_bytes()[0];
                    !(first.is_ascii_digit() || first == b'.' || ((first == b'+' || first == b'-') && trimmed.len() > 1 && (trimmed.as_bytes()[1].is_ascii_digit() || trimmed.as_bytes()[1] == b'.')))
                } else {
                    false
                }
            };
            let a_non_numeric = is_fully_non_numeric_str(&a_deref);
            let b_non_numeric = is_fully_non_numeric_str(&b_deref);
            // int/float + non-numeric string = TypeError
            if a_non_numeric && matches!(b_deref, Value::Long(_) | Value::Double(_) | Value::True | Value::False) {
                return Some(format!(
                    "Unsupported operand types: {} {} {}",
                    Self::value_type_name(&a_deref),
                    op_symbol,
                    Self::value_type_name(&b_deref)
                ));
            }
            if b_non_numeric && matches!(a_deref, Value::Long(_) | Value::Double(_) | Value::True | Value::False) {
                return Some(format!(
                    "Unsupported operand types: {} {} {}",
                    Self::value_type_name(&a_deref),
                    op_symbol,
                    Self::value_type_name(&b_deref)
                ));
            }
        }
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
        // Capture stack trace
        let trace = self.build_exception_trace();
        err_obj.set_property(b"trace".to_vec(), trace);
        err_obj.set_property(b"previous".to_vec(), Value::Null);
        Value::Object(Rc::new(RefCell::new(err_obj)))
    }

    /// Build a trace array for exceptions from the current call stack
    fn build_exception_trace(&self) -> Value {
        let mut trace_arr = PhpArray::new();
        // call_stack entries: (function_name, file, line_called_from, args, is_instance_method)
        for (idx, (func_name, file, line, args, is_method)) in self.call_stack.iter().rev().enumerate() {
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
            // Parse class::method from func_name
            if let Some(sep) = func_name.find("::") {
                frame.set(
                    ArrayKey::String(PhpString::from_bytes(b"class")),
                    Value::String(PhpString::from_string(func_name[..sep].to_string())),
                );
                // Use -> for instance methods, :: for static methods
                let type_str = if *is_method { b"->" as &[u8] } else { b"::" };
                frame.set(
                    ArrayKey::String(PhpString::from_bytes(b"type")),
                    Value::String(PhpString::from_bytes(type_str)),
                );
                frame.set(
                    ArrayKey::String(PhpString::from_bytes(b"function")),
                    Value::String(PhpString::from_string(func_name[sep+2..].to_string())),
                );
            } else if let Some(sep) = func_name.find("->") {
                frame.set(
                    ArrayKey::String(PhpString::from_bytes(b"class")),
                    Value::String(PhpString::from_string(func_name[..sep].to_string())),
                );
                frame.set(
                    ArrayKey::String(PhpString::from_bytes(b"type")),
                    Value::String(PhpString::from_bytes(b"->")),
                );
                frame.set(
                    ArrayKey::String(PhpString::from_bytes(b"function")),
                    Value::String(PhpString::from_string(func_name[sep+2..].to_string())),
                );
            } else {
                frame.set(
                    ArrayKey::String(PhpString::from_bytes(b"function")),
                    Value::String(PhpString::from_string(func_name.clone())),
                );
            }
            // Add args array
            let mut args_arr = PhpArray::new();
            for arg in args {
                args_arr.push(arg.clone());
            }
            frame.set(
                ArrayKey::String(PhpString::from_bytes(b"args")),
                Value::Array(Rc::new(RefCell::new(args_arr))),
            );
            trace_arr.set(ArrayKey::Int(idx as i64), Value::Array(Rc::new(RefCell::new(frame))));
        }
        Value::Array(Rc::new(RefCell::new(trace_arr)))
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
                    b"iterable" => match value {
                        Value::Array(_) | Value::Generator(_) => true,
                        Value::Object(obj) => {
                            let obj_class_lower: Vec<u8> = obj.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                            self.class_implements_interface(&obj_class_lower, b"traversable")
                                || self.class_implements_interface(&obj_class_lower, b"iterator")
                                || self.class_implements_interface(&obj_class_lower, b"iteratoraggregate")
                                || self.builtin_implements_interface(&obj_class_lower, b"traversable")
                                || self.builtin_implements_interface(&obj_class_lower, b"iterator")
                                || self.builtin_implements_interface(&obj_class_lower, b"iteratoraggregate")
                        }
                        _ => false,
                    },
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

        // Check return type compatibility (covariant return types)
        let return_type_incompatible = if let Some(parent_rt) = &parent_op.return_type {
            if let Some(child_rt) = &child_op.return_type {
                // Both have return types - check if they're compatible
                // Simple check: if the string representations differ and the child type
                // is not a subtype of the parent type, they're incompatible
                let parent_type_str = Self::format_param_type_for_sig(parent_rt, parent_class);
                let child_type_str = Self::format_param_type_for_sig(child_rt, child_class);
                if parent_type_str == child_type_str {
                    false
                } else {
                    // Check basic covariance: child return type must be subtype of parent
                    // For simple types: int is not compatible with string, etc.
                    !Self::is_return_type_compatible(child_rt, parent_rt)
                }
            } else {
                // Parent has return type but child doesn't - incompatible
                false
            }
        } else {
            // Parent has no return type - child can add one (covariant)
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

    /// Check if child return type is compatible (covariant) with parent return type.
    /// This is a simplified check - only flags definitely-incompatible cases.
    /// Returns true if types are compatible (or we can't determine).
    fn is_return_type_compatible(child_rt: &crate::opcode::ParamType, parent_rt: &crate::opcode::ParamType) -> bool {
        use crate::opcode::ParamType;
        match (child_rt, parent_rt) {
            // Same type is always compatible
            (ParamType::Simple(a), ParamType::Simple(b)) if a.eq_ignore_ascii_case(b) => true,
            // void is only compatible with void
            (ParamType::Simple(a), _) if a.eq_ignore_ascii_case(b"void") => {
                matches!(parent_rt, ParamType::Simple(b) if b.eq_ignore_ascii_case(b"void"))
            }
            (_, ParamType::Simple(b)) if b.eq_ignore_ascii_case(b"void") => false,
            // never is compatible with anything (bottom type)
            (ParamType::Simple(a), _) if a.eq_ignore_ascii_case(b"never") => true,
            // anything is compatible with mixed (top type)
            (_, ParamType::Simple(b)) if b.eq_ignore_ascii_case(b"mixed") => true,
            // Nullable types: ?T is compatible with ?T
            (ParamType::Nullable(a), ParamType::Nullable(b)) => Self::is_return_type_compatible(a, b),
            // T is compatible with ?T (covariant - narrowing)
            (_, ParamType::Nullable(inner)) => Self::is_return_type_compatible(child_rt, inner),
            // For intersection types, union types, and class types, we can't easily check
            // covariance without resolving the class hierarchy. Be permissive.
            (ParamType::Intersection(_), _) => true,
            (_, ParamType::Intersection(_)) => true,
            // Union in parent: child type should be a subset
            (ParamType::Union(_), ParamType::Union(_)) => true, // too complex, be permissive
            (ParamType::Simple(name), ParamType::Union(parent_types)) => {
                parent_types.iter().any(|pt| Self::is_return_type_compatible(child_rt, pt))
            }
            (ParamType::Union(child_types), ParamType::Simple(_)) => {
                // Union child -> simple parent: only compatible if all child types are compatible
                // But this is complex (class types), be permissive
                true
            }
            // Different simple built-in types are definitely incompatible
            (ParamType::Simple(a), ParamType::Simple(b)) => {
                // Only flag incompatibility for clearly different primitive types
                let primitives = [b"int".as_slice(), b"float", b"string", b"bool", b"array", b"null"];
                let a_lower: Vec<u8> = a.iter().map(|c| c.to_ascii_lowercase()).collect();
                let b_lower: Vec<u8> = b.iter().map(|c| c.to_ascii_lowercase()).collect();
                let a_is_prim = primitives.iter().any(|p| *p == a_lower.as_slice());
                let b_is_prim = primitives.iter().any(|p| *p == b_lower.as_slice());
                if a_is_prim && b_is_prim {
                    // Both are primitives and different -> incompatible
                    false
                } else {
                    // At least one is a class type - could be subclass, be permissive
                    true
                }
            }
            _ => true,
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
                if obj_ref.has_property(b"__enum_case") {
                    let class_name = String::from_utf8_lossy(&obj_ref.class_name);
                    let case_name = obj_ref.get_property(b"name");
                    let case_name_str = case_name.to_php_string().to_string_lossy();
                    format!("{}::{}", class_name, case_name_str)
                } else {
                    format!("Object({})", String::from_utf8_lossy(&obj_ref.class_name))
                }
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

    /// Get the current class scope name (lowercase), if any
    pub fn get_current_class_name(&self) -> Option<Vec<u8>> {
        self.class_scope_stack.last().cloned()
    }

    /// Get a class definition by lowercase name
    pub fn get_class_def(&self, class_name_lower: &[u8]) -> Option<&ClassEntry> {
        self.classes.get(class_name_lower)
    }

    /// Check if class_a is a subclass of class_b (both lowercase)
    pub fn is_subclass_of(&self, class_a: &[u8], class_b: &[u8]) -> bool {
        self.class_extends(class_a, class_b)
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
                b"arrayiterator" | b"recursivearrayiterator" | b"splfixedarray"
                    | b"spldoublylinkedlist" | b"splstack" | b"splqueue" | b"splpriorityqueue"
                    | b"splheap" | b"splminheap" | b"splmaxheap"
                    | b"emptyiterator" | b"iteratoriterator" | b"recursiveiteratoriterator"
                    | b"norewinditerator" | b"infiniteiterator" | b"limititerator"
                    | b"cachingiterator" | b"recursivecachingiterator"
                    | b"appenditerator" | b"filteriterator" | b"callbackfilteriterator"
                    | b"recursivefilteriterator" | b"recursivecallbackfilteriterator"
                    | b"regexiterator" | b"recursiveregexiterator"
                    | b"multipleiterator" | b"parentiterator"
                    | b"splobjectstorage"
                    | b"directoryiterator" | b"filesystemiterator" | b"recursivedirectoryiterator"
                    | b"globiterator" | b"splfileobject" | b"spltempfileobject"
            ),
            b"iteratoraggregate" => matches!(class_lower, b"arrayobject"),
            b"traversable" => matches!(
                class_lower,
                b"arrayiterator" | b"recursivearrayiterator" | b"splfixedarray"
                    | b"spldoublylinkedlist" | b"splstack" | b"splqueue" | b"splpriorityqueue"
                    | b"splheap" | b"splminheap" | b"splmaxheap"
                    | b"emptyiterator" | b"iteratoriterator" | b"recursiveiteratoriterator"
                    | b"norewinditerator" | b"infiniteiterator" | b"limititerator"
                    | b"cachingiterator" | b"recursivecachingiterator"
                    | b"appenditerator" | b"filteriterator" | b"callbackfilteriterator"
                    | b"recursivefilteriterator" | b"recursivecallbackfilteriterator"
                    | b"regexiterator" | b"recursiveregexiterator"
                    | b"multipleiterator" | b"parentiterator"
                    | b"splobjectstorage"
                    | b"arrayobject"
                    | b"directoryiterator" | b"filesystemiterator" | b"recursivedirectoryiterator"
                    | b"globiterator" | b"splfileobject" | b"spltempfileobject"
            ),
            b"countable" => matches!(
                class_lower,
                b"arrayobject" | b"arrayiterator" | b"recursivearrayiterator"
                    | b"splfixedarray" | b"spldoublylinkedlist" | b"splstack" | b"splqueue"
                    | b"splpriorityqueue" | b"splobjectstorage"
                    | b"splheap" | b"splminheap" | b"splmaxheap"
                    | b"cachingiterator" | b"recursivecachingiterator"
            ),
            b"arrayaccess" => matches!(
                class_lower,
                b"arrayobject" | b"arrayiterator" | b"recursivearrayiterator"
                    | b"splfixedarray" | b"spldoublylinkedlist" | b"splstack" | b"splqueue"
                    | b"splobjectstorage"
            ),
            b"outeriterator" => matches!(
                class_lower,
                b"iteratoriterator" | b"recursiveiteratoriterator"
                    | b"norewinditerator" | b"infiniteiterator" | b"limititerator"
                    | b"cachingiterator" | b"recursivecachingiterator"
                    | b"appenditerator" | b"filteriterator" | b"callbackfilteriterator"
                    | b"recursivefilteriterator" | b"recursivecallbackfilteriterator"
                    | b"regexiterator" | b"recursiveregexiterator"
                    | b"parentiterator"
            ),
            b"recursiveiterator" => matches!(
                class_lower,
                b"recursivearrayiterator" | b"recursivedirectoryiterator"
            ),
            b"seekableiterator" => matches!(
                class_lower,
                b"arrayiterator" | b"recursivearrayiterator" | b"splfixedarray"
                    | b"directoryiterator" | b"filesystemiterator" | b"recursivedirectoryiterator"
                    | b"globiterator" | b"splfileobject" | b"spltempfileobject"
            ),
            b"serializable" => matches!(
                class_lower,
                b"arrayobject" | b"arrayiterator" | b"recursivearrayiterator"
                    | b"splfixedarray" | b"spldoublylinkedlist" | b"splstack" | b"splqueue"
                    | b"splobjectstorage"
            ),
            b"stringable" => matches!(
                class_lower,
                b"splfileinfo" | b"splfileobject" | b"spltempfileobject"
                    | b"directoryiterator" | b"filesystemiterator" | b"recursivedirectoryiterator"
                    | b"globiterator"
            ),
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
            b"splheap" | b"splminheap" | b"splmaxheap" => {
                self.spl_heap_method(method_lower, obj)
            }
            b"emptyiterator" => {
                match method_lower {
                    b"rewind" | b"next" => Some(Value::Null),
                    b"valid" => Some(Value::False),
                    b"current" | b"key" => {
                        // These throw RuntimeException
                        let exc = self.create_exception(b"RuntimeException", "Accessing the value of an EmptyIterator", 0);
                        self.current_exception = Some(exc);
                        Some(Value::Null)
                    }
                    _ => None,
                }
            }
            b"iteratoriterator" | b"recursiveiteratoriterator" => {
                self.spl_outer_iterator_method(method_lower, obj)
            }
            b"norewinditerator" => {
                match method_lower {
                    b"rewind" => Some(Value::Null), // NoRewindIterator::rewind() does nothing
                    b"valid" | b"current" | b"key" | b"next" => {
                        self.spl_outer_iterator_method(method_lower, obj)
                    }
                    b"getinneriterator" => {
                        let ob = obj.borrow();
                        Some(ob.get_property(b"__spl_inner"))
                    }
                    _ => None,
                }
            }
            b"infiniteiterator" => {
                match method_lower {
                    b"next" => {
                        // Call next() on inner, if !valid(), call rewind()
                        let ob = obj.borrow();
                        let inner = ob.get_property(b"__spl_inner");
                        drop(ob);
                        self.call_object_method(&inner, b"next", &[]);
                        let valid = self.call_object_method(&inner, b"valid", &[]).unwrap_or(Value::False);
                        if !valid.is_truthy() {
                            self.call_object_method(&inner, b"rewind", &[]);
                        }
                        Some(Value::Null)
                    }
                    _ => self.spl_outer_iterator_method(method_lower, obj),
                }
            }
            b"limititerator" => {
                self.spl_limit_iterator_method(method_lower, obj)
            }
            b"cachingiterator" | b"recursivecachingiterator" => {
                self.spl_caching_iterator_method(method_lower, obj)
            }
            b"appenditerator" => {
                self.spl_append_iterator_method(method_lower, obj)
            }
            b"filteriterator" | b"callbackfilteriterator"
            | b"recursivefilteriterator" | b"recursivecallbackfilteriterator"
            | b"regexiterator" | b"recursiveregexiterator" | b"parentiterator" => {
                self.spl_filter_iterator_method(method_lower, obj)
            }
            b"multipleiterator" => {
                self.spl_multiple_iterator_method(method_lower, obj)
            }
            b"datetime" | b"datetimeimmutable" => {
                match method_lower {
                    b"gettimestamp" => {
                        let ob = obj.borrow();
                        Some(ob.get_property(b"__timestamp"))
                    }
                    b"getoffset" => Some(Value::Long(0)), // UTC
                    b"gettimezone" => {
                        let obj_id = self.next_object_id;
                        self.next_object_id += 1;
                        let mut tz_obj = PhpObject::new(b"DateTimeZone".to_vec(), obj_id);
                        tz_obj.set_property(b"timezone".to_vec(), Value::String(PhpString::from_bytes(b"UTC")));
                        Some(Value::Object(Rc::new(RefCell::new(tz_obj))))
                    }
                    _ => None, // format() and others handled via __spl:: dispatch
                }
            }
            b"datetimezone" => {
                match method_lower {
                    b"getname" => {
                        let ob = obj.borrow();
                        let tz = ob.get_property(b"timezone");
                        if matches!(tz, Value::Null) {
                            Some(Value::String(PhpString::from_bytes(b"UTC")))
                        } else {
                            Some(tz)
                        }
                    }
                    b"getoffset" => Some(Value::Long(0)),
                    _ => None,
                }
            }
            b"reflectionclass" | b"reflectionobject" | b"reflectionenum" => {
                self.reflection_class_method(method_lower, obj)
            }
            b"reflectionmethod" => {
                self.reflection_method_method(method_lower, obj)
            }
            b"reflectionfunction" | b"reflectionfunctionabstract" => {
                self.reflection_function_method(method_lower, obj)
            }
            b"reflectionproperty" => {
                self.reflection_property_method(method_lower, obj)
            }
            b"reflectionparameter" => {
                self.reflection_parameter_method(method_lower, obj)
            }
            b"reflectionextension" => {
                self.reflection_extension_method(method_lower, obj)
            }
            b"reflectionnamedtype" => {
                self.reflection_named_type_method(method_lower, obj)
            }
            b"reflectionuniontype" | b"reflectionintersectiontype" => {
                self.reflection_composite_type_method(method_lower, obj)
            }
            b"reflectionclassconstant" | b"reflectionenumunitcase" | b"reflectionenumbackedcase" => {
                self.reflection_class_constant_method(method_lower, obj)
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
                if matches!(flags, Value::Null) {
                    Some(Value::Long(0))
                } else {
                    Some(flags)
                }
            }
            b"setflags" => None,
            b"getiteratorclass" => {
                let ob = obj.borrow();
                let ic = ob.get_property(b"__spl_iterator_class");
                if matches!(ic, Value::Null) {
                    Some(Value::String(PhpString::from_bytes(b"ArrayIterator")))
                } else {
                    Some(ic)
                }
            }
            b"getiterator" => {
                // Create an ArrayIterator from the internal array
                let ob = obj.borrow();
                let spl_arr = ob.get_property(b"__spl_array");
                drop(ob);
                let obj_id = self.next_object_id;
                self.next_object_id += 1;
                let mut iter_obj = PhpObject::new(b"ArrayIterator".to_vec(), obj_id);
                iter_obj.set_property(b"__spl_array".to_vec(), spl_arr);
                Some(Value::Object(Rc::new(RefCell::new(iter_obj))))
            }
            b"setiteratorclass" => None,
            b"ksort" => None,     // handled in handle_spl_docall
            b"asort" => None,
            b"natsort" => None,
            b"natcasesort" => None,
            b"uasort" => None,
            b"uksort" => None,
            // Iterator methods for ArrayIterator
            b"rewind" => {
                let mut ob = obj.borrow_mut();
                ob.set_property(b"__spl_pos".to_vec(), Value::Long(0));
                Some(Value::Null)
            }
            b"valid" => {
                let ob = obj.borrow();
                let arr = ob.get_property(storage_prop);
                let pos = ob.get_property(b"__spl_pos");
                let pos_val = if let Value::Long(p) = pos { p as usize } else { 0 };
                if let Value::Array(a) = arr {
                    Some(if pos_val < a.borrow().len() { Value::True } else { Value::False })
                } else {
                    Some(Value::False)
                }
            }
            b"current" => {
                let ob = obj.borrow();
                let arr = ob.get_property(storage_prop);
                let pos = ob.get_property(b"__spl_pos");
                let pos_val = if let Value::Long(p) = pos { p as usize } else { 0 };
                if let Value::Array(a) = arr {
                    let a = a.borrow();
                    Some(a.values().nth(pos_val).cloned().unwrap_or(Value::Null))
                } else {
                    Some(Value::Null)
                }
            }
            b"key" => {
                let ob = obj.borrow();
                let arr = ob.get_property(storage_prop);
                let pos = ob.get_property(b"__spl_pos");
                let pos_val = if let Value::Long(p) = pos { p as usize } else { 0 };
                if let Value::Array(a) = arr {
                    let a = a.borrow();
                    if let Some(key) = a.keys().nth(pos_val) {
                        match key {
                            ArrayKey::Int(n) => Some(Value::Long(*n)),
                            ArrayKey::String(s) => Some(Value::String(s.clone())),
                        }
                    } else {
                        Some(Value::Null)
                    }
                } else {
                    Some(Value::Null)
                }
            }
            b"next" => {
                let mut ob = obj.borrow_mut();
                let pos = ob.get_property(b"__spl_pos");
                let pos_val = if let Value::Long(p) = pos { p } else { 0 };
                ob.set_property(b"__spl_pos".to_vec(), Value::Long(pos_val + 1));
                Some(Value::Null)
            }
            b"seek" => None,     // handled in handle_spl_docall
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
            // Iterator methods
            b"rewind" => {
                let mut ob = obj.borrow_mut();
                ob.set_property(b"__spl_pos".to_vec(), Value::Long(0));
                Some(Value::Null)
            }
            b"valid" => {
                let ob = obj.borrow();
                let size = ob.get_property(b"__spl_size");
                let pos = ob.get_property(b"__spl_pos");
                let size_val = if let Value::Long(s) = size { s } else { 0 };
                let pos_val = if let Value::Long(p) = pos { p } else { 0 };
                Some(if pos_val >= 0 && pos_val < size_val { Value::True } else { Value::False })
            }
            b"current" => {
                let ob = obj.borrow();
                let arr = ob.get_property(b"__spl_array");
                let pos = ob.get_property(b"__spl_pos");
                let pos_val = if let Value::Long(p) = pos { p } else { 0 };
                if let Value::Array(a) = arr {
                    let a = a.borrow();
                    Some(a.get(&ArrayKey::Int(pos_val)).cloned().unwrap_or(Value::Null))
                } else {
                    Some(Value::Null)
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
                        drop(a);
                        drop(ob);
                        let exc = self.create_exception(b"RuntimeException", "Can't peek at an empty datastructure", 0);
                        self.current_exception = Some(exc);
                        Some(Value::Null)
                    } else {
                        Some(a.values().last().cloned().unwrap_or(Value::Null))
                    }
                } else {
                    drop(ob);
                    let exc = self.create_exception(b"RuntimeException", "Can't peek at an empty datastructure", 0);
                    self.current_exception = Some(exc);
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
                        drop(a);
                        drop(ob);
                        let exc = self.create_exception(b"RuntimeException", "Can't peek at an empty datastructure", 0);
                        self.current_exception = Some(exc);
                        Some(Value::Null)
                    } else {
                        Some(a.values().next().cloned().unwrap_or(Value::Null))
                    }
                } else {
                    drop(ob);
                    let exc = self.create_exception(b"RuntimeException", "Can't peek at an empty datastructure", 0);
                    self.current_exception = Some(exc);
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
            b"rewind" => {
                let mut ob = obj.borrow_mut();
                ob.set_property(b"__spl_pos".to_vec(), Value::Long(0));
                Some(Value::Null)
            }
            b"valid" => {
                let ob = obj.borrow();
                let arr = ob.get_property(b"__spl_array");
                let pos = ob.get_property(b"__spl_pos");
                let pos_val = if let Value::Long(p) = pos { p as usize } else { 0 };
                if let Value::Array(a) = arr {
                    Some(if pos_val < a.borrow().len() { Value::True } else { Value::False })
                } else {
                    Some(Value::False)
                }
            }
            b"current" => {
                let ob = obj.borrow();
                let arr = ob.get_property(b"__spl_array");
                let pos = ob.get_property(b"__spl_pos");
                let pos_val = if let Value::Long(p) = pos { p as usize } else { 0 };
                if let Value::Array(a) = arr {
                    let a = a.borrow();
                    if pos_val >= a.len() {
                        drop(a);
                        drop(ob);
                        let exc = self.create_exception(b"RuntimeException", "Called current() on invalid iterator", 0);
                        self.current_exception = Some(exc);
                        Some(Value::Null)
                    } else {
                        // SplObjectStorage: current() returns the stored object
                        // The __spl_objects array stores the actual objects in order
                        let objs = ob.get_property(b"__spl_objects");
                        if let Value::Array(objects) = objs {
                            let objects = objects.borrow();
                            Some(objects.values().nth(pos_val).cloned().unwrap_or(Value::Null))
                        } else {
                            Some(a.values().nth(pos_val).cloned().unwrap_or(Value::Null))
                        }
                    }
                } else {
                    drop(ob);
                    let exc = self.create_exception(b"RuntimeException", "Called current() on invalid iterator", 0);
                    self.current_exception = Some(exc);
                    Some(Value::Null)
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
            b"getinfo" => {
                // Returns the data associated with the current object
                let ob = obj.borrow();
                let info_arr = ob.get_property(b"__spl_info");
                let pos = ob.get_property(b"__spl_pos");
                let pos_val = if let Value::Long(p) = pos { p as usize } else { 0 };
                if let Value::Array(a) = info_arr {
                    let a = a.borrow();
                    Some(a.values().nth(pos_val).cloned().unwrap_or(Value::Null))
                } else {
                    Some(Value::Null)
                }
            }
            b"gethash" => None, // handled in handle_spl_docall
            b"removeall" | b"removeallexcept" | b"addall" => None, // handled in handle_spl_docall
            b"seek" => None, // handled in handle_spl_docall
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

    fn spl_heap_method(
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
            b"top" | b"current" => {
                let ob = obj.borrow();
                let arr = ob.get_property(storage_prop);
                if let Value::Array(a) = arr {
                    let a = a.borrow();
                    if a.len() == 0 {
                        drop(a);
                        drop(ob);
                        let exc = self.create_exception(b"RuntimeException", "Can't peek at an empty heap", 0);
                        self.current_exception = Some(exc);
                        Some(Value::Null)
                    } else {
                        Some(a.values().next().cloned().unwrap_or(Value::Null))
                    }
                } else {
                    let exc = self.create_exception(b"RuntimeException", "Can't peek at an empty heap", 0);
                    self.current_exception = Some(exc);
                    Some(Value::Null)
                }
            }
            b"valid" => {
                let ob = obj.borrow();
                let arr = ob.get_property(storage_prop);
                if let Value::Array(a) = arr {
                    Some(if a.borrow().len() > 0 { Value::True } else { Value::False })
                } else {
                    Some(Value::False)
                }
            }
            b"key" => {
                // SplHeap key is count - 1 during first element, decrements as items are extracted
                let ob = obj.borrow();
                let arr = ob.get_property(b"__spl_array");
                if let Value::Array(a) = arr {
                    let count = a.borrow().len() as i64;
                    Some(Value::Long(count - 1))
                } else {
                    Some(Value::Long(-1))
                }
            }
            b"next" | b"extract" => None, // handled in handle_spl_docall
            b"insert" => None,
            b"rewind" => {
                // Rewind just resets the key counter
                let mut ob = obj.borrow_mut();
                ob.set_property(b"__spl_heap_key".to_vec(), Value::Long(0));
                Some(Value::Null)
            }
            b"recoverfromcorruption" => Some(Value::Null),
            b"isCorrupted" => Some(Value::False),
            _ => None,
        }
    }

    /// Method dispatch for outer iterator wrappers (IteratorIterator, etc.)
    fn spl_outer_iterator_method(
        &mut self,
        method: &[u8],
        obj: &Rc<RefCell<PhpObject>>,
    ) -> Option<Value> {
        match method {
            b"getinneriterator" => {
                let ob = obj.borrow();
                Some(ob.get_property(b"__spl_inner"))
            }
            b"rewind" => {
                let ob = obj.borrow();
                let inner = ob.get_property(b"__spl_inner");
                drop(ob);
                self.call_object_method(&inner, b"rewind", &[]);
                Some(Value::Null)
            }
            b"valid" => {
                let ob = obj.borrow();
                let inner = ob.get_property(b"__spl_inner");
                drop(ob);
                let r = self.call_object_method(&inner, b"valid", &[]).unwrap_or(Value::False);
                Some(r)
            }
            b"current" => {
                let ob = obj.borrow();
                let inner = ob.get_property(b"__spl_inner");
                drop(ob);
                let r = self.call_object_method(&inner, b"current", &[]).unwrap_or(Value::Null);
                Some(r)
            }
            b"key" => {
                let ob = obj.borrow();
                let inner = ob.get_property(b"__spl_inner");
                drop(ob);
                let r = self.call_object_method(&inner, b"key", &[]).unwrap_or(Value::Null);
                Some(r)
            }
            b"next" => {
                let ob = obj.borrow();
                let inner = ob.get_property(b"__spl_inner");
                drop(ob);
                self.call_object_method(&inner, b"next", &[]);
                Some(Value::Null)
            }
            _ => None,
        }
    }

    fn spl_limit_iterator_method(
        &mut self,
        method: &[u8],
        obj: &Rc<RefCell<PhpObject>>,
    ) -> Option<Value> {
        match method {
            b"getinneriterator" => {
                let ob = obj.borrow();
                Some(ob.get_property(b"__spl_inner"))
            }
            b"rewind" => {
                let ob = obj.borrow();
                let inner = ob.get_property(b"__spl_inner");
                let offset = ob.get_property(b"__spl_offset");
                let offset_val = if let Value::Long(o) = offset { o } else { 0 };
                drop(ob);
                self.call_object_method(&inner, b"rewind", &[]);
                // Skip to offset
                for _ in 0..offset_val {
                    let valid = self.call_object_method(&inner, b"valid", &[]).unwrap_or(Value::False);
                    if !valid.is_truthy() { break; }
                    self.call_object_method(&inner, b"next", &[]);
                }
                obj.borrow_mut().set_property(b"__spl_pos".to_vec(), Value::Long(0));
                Some(Value::Null)
            }
            b"valid" => {
                let ob = obj.borrow();
                let inner = ob.get_property(b"__spl_inner");
                let count = ob.get_property(b"__spl_count");
                let pos = ob.get_property(b"__spl_pos");
                let count_val = if let Value::Long(c) = count { c } else { -1 };
                let pos_val = if let Value::Long(p) = pos { p } else { 0 };
                drop(ob);
                if count_val >= 0 && pos_val >= count_val {
                    return Some(Value::False);
                }
                let r = self.call_object_method(&inner, b"valid", &[]).unwrap_or(Value::False);
                Some(r)
            }
            b"current" => {
                let ob = obj.borrow();
                let inner = ob.get_property(b"__spl_inner");
                drop(ob);
                let r = self.call_object_method(&inner, b"current", &[]).unwrap_or(Value::Null);
                Some(r)
            }
            b"key" => {
                let ob = obj.borrow();
                let inner = ob.get_property(b"__spl_inner");
                drop(ob);
                let r = self.call_object_method(&inner, b"key", &[]).unwrap_or(Value::Null);
                Some(r)
            }
            b"next" => {
                let ob = obj.borrow();
                let inner = ob.get_property(b"__spl_inner");
                drop(ob);
                self.call_object_method(&inner, b"next", &[]);
                let mut ob = obj.borrow_mut();
                let pos = ob.get_property(b"__spl_pos");
                let pos_val = if let Value::Long(p) = pos { p } else { 0 };
                ob.set_property(b"__spl_pos".to_vec(), Value::Long(pos_val + 1));
                Some(Value::Null)
            }
            b"getposition" => {
                let ob = obj.borrow();
                let pos = ob.get_property(b"__spl_pos");
                let offset = ob.get_property(b"__spl_offset");
                let pos_val = if let Value::Long(p) = pos { p } else { 0 };
                let offset_val = if let Value::Long(o) = offset { o } else { 0 };
                Some(Value::Long(offset_val + pos_val))
            }
            _ => None,
        }
    }

    fn spl_caching_iterator_method(
        &mut self,
        method: &[u8],
        obj: &Rc<RefCell<PhpObject>>,
    ) -> Option<Value> {
        match method {
            b"getinneriterator" => {
                let ob = obj.borrow();
                Some(ob.get_property(b"__spl_inner"))
            }
            b"rewind" => {
                let ob = obj.borrow();
                let inner = ob.get_property(b"__spl_inner");
                drop(ob);
                self.call_object_method(&inner, b"rewind", &[]);
                // Cache the first value
                let valid = self.call_object_method(&inner, b"valid", &[]).unwrap_or(Value::False);
                let mut ob = obj.borrow_mut();
                if valid.is_truthy() {
                    drop(ob);
                    let current = self.call_object_method(&inner, b"current", &[]).unwrap_or(Value::Null);
                    let key = self.call_object_method(&inner, b"key", &[]).unwrap_or(Value::Null);
                    let mut ob = obj.borrow_mut();
                    ob.set_property(b"__spl_cached_current".to_vec(), current);
                    ob.set_property(b"__spl_cached_key".to_vec(), key);
                    ob.set_property(b"__spl_has_next".to_vec(), Value::True);
                } else {
                    ob.set_property(b"__spl_has_next".to_vec(), Value::False);
                }
                Some(Value::Null)
            }
            b"valid" => {
                let ob = obj.borrow();
                let has_next = ob.get_property(b"__spl_has_next");
                Some(if has_next.is_truthy() { Value::True } else { Value::False })
            }
            b"current" => {
                let ob = obj.borrow();
                Some(ob.get_property(b"__spl_cached_current"))
            }
            b"key" => {
                let ob = obj.borrow();
                Some(ob.get_property(b"__spl_cached_key"))
            }
            b"next" => {
                let ob = obj.borrow();
                let inner = ob.get_property(b"__spl_inner");
                drop(ob);
                self.call_object_method(&inner, b"next", &[]);
                let valid = self.call_object_method(&inner, b"valid", &[]).unwrap_or(Value::False);
                if valid.is_truthy() {
                    let current = self.call_object_method(&inner, b"current", &[]).unwrap_or(Value::Null);
                    let key = self.call_object_method(&inner, b"key", &[]).unwrap_or(Value::Null);
                    let mut ob = obj.borrow_mut();
                    ob.set_property(b"__spl_cached_current".to_vec(), current);
                    ob.set_property(b"__spl_cached_key".to_vec(), key);
                    ob.set_property(b"__spl_has_next".to_vec(), Value::True);
                } else {
                    let mut ob = obj.borrow_mut();
                    ob.set_property(b"__spl_has_next".to_vec(), Value::False);
                }
                Some(Value::Null)
            }
            b"hasnext" => {
                let ob = obj.borrow();
                let inner = ob.get_property(b"__spl_inner");
                drop(ob);
                // Check if the inner iterator has more after current
                // Actually, CachingIterator's hasNext() checks if the inner has already been advanced past
                let ob = obj.borrow();
                let has_next = ob.get_property(b"__spl_has_next");
                Some(if has_next.is_truthy() { Value::True } else { Value::False })
            }
            b"count" => {
                let ob = obj.borrow();
                let arr = ob.get_property(b"__spl_cache");
                if let Value::Array(a) = arr {
                    Some(Value::Long(a.borrow().len() as i64))
                } else {
                    Some(Value::Long(0))
                }
            }
            b"getflags" => {
                let ob = obj.borrow();
                let flags = ob.get_property(b"__spl_flags");
                Some(if matches!(flags, Value::Null) { Value::Long(0) } else { flags })
            }
            b"setflags" => None, // handled in handle_spl_docall
            _ => None,
        }
    }

    fn spl_append_iterator_method(
        &mut self,
        method: &[u8],
        obj: &Rc<RefCell<PhpObject>>,
    ) -> Option<Value> {
        match method {
            b"append" => None, // handled in handle_spl_docall
            b"rewind" => {
                let mut ob = obj.borrow_mut();
                ob.set_property(b"__spl_idx".to_vec(), Value::Long(0));
                drop(ob);
                // Rewind the first iterator
                let first_iter = {
                    let ob = obj.borrow();
                    let iters = ob.get_property(b"__spl_array");
                    if let Value::Array(a) = iters {
                        a.borrow().values().next().cloned()
                    } else {
                        None
                    }
                };
                if let Some(first) = first_iter {
                    self.call_object_method(&first, b"rewind", &[]);
                }
                Some(Value::Null)
            }
            b"valid" => {
                let iter = {
                    let ob = obj.borrow();
                    let idx = ob.get_property(b"__spl_idx");
                    let idx_val = if let Value::Long(i) = idx { i as usize } else { 0 };
                    let iters = ob.get_property(b"__spl_array");
                    if let Value::Array(a) = iters {
                        a.borrow().values().nth(idx_val).cloned()
                    } else {
                        None
                    }
                };
                if let Some(iter) = iter {
                    let v = self.call_object_method(&iter, b"valid", &[]).unwrap_or(Value::False);
                    Some(v)
                } else {
                    Some(Value::False)
                }
            }
            b"current" => {
                let iter = {
                    let ob = obj.borrow();
                    let idx = ob.get_property(b"__spl_idx");
                    let idx_val = if let Value::Long(i) = idx { i as usize } else { 0 };
                    let iters = ob.get_property(b"__spl_array");
                    if let Value::Array(a) = iters {
                        a.borrow().values().nth(idx_val).cloned()
                    } else {
                        None
                    }
                };
                if let Some(iter) = iter {
                    let v = self.call_object_method(&iter, b"current", &[]).unwrap_or(Value::Null);
                    Some(v)
                } else {
                    Some(Value::Null)
                }
            }
            b"key" => {
                let iter = {
                    let ob = obj.borrow();
                    let idx = ob.get_property(b"__spl_idx");
                    let idx_val = if let Value::Long(i) = idx { i as usize } else { 0 };
                    let iters = ob.get_property(b"__spl_array");
                    if let Value::Array(a) = iters {
                        a.borrow().values().nth(idx_val).cloned()
                    } else {
                        None
                    }
                };
                if let Some(iter) = iter {
                    let v = self.call_object_method(&iter, b"key", &[]).unwrap_or(Value::Null);
                    Some(v)
                } else {
                    Some(Value::Null)
                }
            }
            b"next" => {
                let (iter, idx_val, total) = {
                    let ob = obj.borrow();
                    let idx = ob.get_property(b"__spl_idx");
                    let idx_val = if let Value::Long(i) = idx { i as usize } else { 0 };
                    let iters = ob.get_property(b"__spl_array");
                    if let Value::Array(a) = iters {
                        let borrow = a.borrow();
                        let total = borrow.len();
                        let iter = borrow.values().nth(idx_val).cloned();
                        (iter, idx_val, total)
                    } else {
                        (None, idx_val, 0)
                    }
                };
                if let Some(iter) = iter {
                    self.call_object_method(&iter, b"next", &[]);
                    // If current iterator is done, move to next
                    let valid = self.call_object_method(&iter, b"valid", &[]).unwrap_or(Value::False);
                    if !valid.is_truthy() && idx_val + 1 < total {
                        obj.borrow_mut().set_property(b"__spl_idx".to_vec(), Value::Long((idx_val + 1) as i64));
                        // Rewind the next iterator
                        let next_iter = {
                            let ob = obj.borrow();
                            let iters = ob.get_property(b"__spl_array");
                            if let Value::Array(a) = iters {
                                a.borrow().values().nth(idx_val + 1).cloned()
                            } else {
                                None
                            }
                        };
                        if let Some(next_iter) = next_iter {
                            self.call_object_method(&next_iter, b"rewind", &[]);
                        }
                    }
                }
                Some(Value::Null)
            }
            b"getinneriterator" => {
                let ob = obj.borrow();
                let idx = ob.get_property(b"__spl_idx");
                let idx_val = if let Value::Long(i) = idx { i as usize } else { 0 };
                let iters = ob.get_property(b"__spl_array");
                if let Value::Array(a) = iters {
                    let a = a.borrow();
                    Some(a.values().nth(idx_val).cloned().unwrap_or(Value::Null))
                } else {
                    Some(Value::Null)
                }
            }
            _ => None,
        }
    }

    fn spl_filter_iterator_method(
        &mut self,
        method: &[u8],
        obj: &Rc<RefCell<PhpObject>>,
    ) -> Option<Value> {
        match method {
            b"accept" => {
                // For CallbackFilterIterator, call the stored callback
                let ob = obj.borrow();
                let class_lower: Vec<u8> = ob.class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                let callback = ob.get_property(b"__spl_callback");
                let inner = ob.get_property(b"__spl_inner");
                drop(ob);
                if !matches!(callback, Value::Null) {
                    let current = self.call_object_method(&inner, b"current", &[]).unwrap_or(Value::Null);
                    let key = self.call_object_method(&inner, b"key", &[]).unwrap_or(Value::Null);
                    // CallbackFilterIterator callback receives (value, key, iterator)
                    let result = self.spl_call_filter_callback(&callback, &[current, key, inner]);
                    Some(if result.is_truthy() { Value::True } else { Value::False })
                } else {
                    // For ParentIterator, accept() checks hasChildren()
                    if class_lower == b"parentiterator" {
                        let inner = {
                            let ob = obj.borrow();
                            ob.get_property(b"__spl_inner")
                        };
                        let has_children = self.call_object_method(&inner, b"hasChildren", &[]).unwrap_or(Value::False);
                        Some(has_children)
                    } else {
                        // Default accept - subclasses should override
                        Some(Value::True)
                    }
                }
            }
            b"getinneriterator" => {
                let ob = obj.borrow();
                Some(ob.get_property(b"__spl_inner"))
            }
            b"rewind" => {
                let ob = obj.borrow();
                let inner = ob.get_property(b"__spl_inner");
                drop(ob);
                self.call_object_method(&inner, b"rewind", &[]);
                // Find first accepted element
                self.spl_filter_advance_to_accepted(obj);
                Some(Value::Null)
            }
            b"valid" => {
                let ob = obj.borrow();
                let inner = ob.get_property(b"__spl_inner");
                drop(ob);
                let r = self.call_object_method(&inner, b"valid", &[]).unwrap_or(Value::False);
                Some(r)
            }
            b"current" => {
                let ob = obj.borrow();
                let inner = ob.get_property(b"__spl_inner");
                drop(ob);
                let r = self.call_object_method(&inner, b"current", &[]).unwrap_or(Value::Null);
                Some(r)
            }
            b"key" => {
                let ob = obj.borrow();
                let inner = ob.get_property(b"__spl_inner");
                drop(ob);
                let r = self.call_object_method(&inner, b"key", &[]).unwrap_or(Value::Null);
                Some(r)
            }
            b"next" => {
                let ob = obj.borrow();
                let inner = ob.get_property(b"__spl_inner");
                drop(ob);
                self.call_object_method(&inner, b"next", &[]);
                self.spl_filter_advance_to_accepted(obj);
                Some(Value::Null)
            }
            _ => None,
        }
    }

    fn spl_filter_advance_to_accepted(&mut self, obj: &Rc<RefCell<PhpObject>>) {
        for _ in 0..10000 {
            let ob = obj.borrow();
            let inner = ob.get_property(b"__spl_inner");
            drop(ob);
            let valid = self.call_object_method(&inner, b"valid", &[]).unwrap_or(Value::False);
            if !valid.is_truthy() { break; }
            // Call accept() on the filter iterator object
            let obj_val = Value::Object(obj.clone());
            let accepted = self.call_object_method(&obj_val, b"accept", &[]).unwrap_or(Value::True);
            if accepted.is_truthy() { break; }
            self.call_object_method(&inner, b"next", &[]);
        }
    }

    fn spl_multiple_iterator_method(
        &mut self,
        method: &[u8],
        obj: &Rc<RefCell<PhpObject>>,
    ) -> Option<Value> {
        match method {
            b"attachiterator" => None, // handled in handle_spl_docall
            b"rewind" => {
                let ob = obj.borrow();
                let iters = ob.get_property(b"__spl_array");
                if let Value::Array(a) = iters {
                    let vals: Vec<Value> = a.borrow().values().cloned().collect();
                    drop(ob);
                    for iter in &vals {
                        self.call_object_method(iter, b"rewind", &[]);
                    }
                }
                Some(Value::Null)
            }
            b"valid" => {
                let ob = obj.borrow();
                let flags = ob.get_property(b"__spl_flags");
                let flags_val = if let Value::Long(f) = flags { f } else { 1 }; // MIT_NEED_ALL default
                let iters = ob.get_property(b"__spl_array");
                if let Value::Array(a) = iters {
                    let vals: Vec<Value> = a.borrow().values().cloned().collect();
                    drop(ob);
                    if vals.is_empty() { return Some(Value::False); }
                    if flags_val & 1 != 0 {
                        // MIT_NEED_ALL
                        for iter in &vals {
                            let v = self.call_object_method(iter, b"valid", &[]).unwrap_or(Value::False);
                            if !v.is_truthy() { return Some(Value::False); }
                        }
                        Some(Value::True)
                    } else {
                        // MIT_NEED_ANY
                        for iter in &vals {
                            let v = self.call_object_method(iter, b"valid", &[]).unwrap_or(Value::False);
                            if v.is_truthy() { return Some(Value::True); }
                        }
                        Some(Value::False)
                    }
                } else {
                    Some(Value::False)
                }
            }
            b"current" => {
                let ob = obj.borrow();
                let iters = ob.get_property(b"__spl_array");
                if let Value::Array(a) = iters {
                    let vals: Vec<Value> = a.borrow().values().cloned().collect();
                    drop(ob);
                    let mut result = PhpArray::new();
                    for (i, iter) in vals.iter().enumerate() {
                        let v = self.call_object_method(iter, b"current", &[]).unwrap_or(Value::Null);
                        result.set(ArrayKey::Int(i as i64), v);
                    }
                    Some(Value::Array(Rc::new(RefCell::new(result))))
                } else {
                    Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
                }
            }
            b"key" => {
                let ob = obj.borrow();
                let iters = ob.get_property(b"__spl_array");
                if let Value::Array(a) = iters {
                    let vals: Vec<Value> = a.borrow().values().cloned().collect();
                    drop(ob);
                    let mut result = PhpArray::new();
                    for (i, iter) in vals.iter().enumerate() {
                        let v = self.call_object_method(iter, b"key", &[]).unwrap_or(Value::Null);
                        result.set(ArrayKey::Int(i as i64), v);
                    }
                    Some(Value::Array(Rc::new(RefCell::new(result))))
                } else {
                    Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
                }
            }
            b"next" => {
                let ob = obj.borrow();
                let iters = ob.get_property(b"__spl_array");
                if let Value::Array(a) = iters {
                    let vals: Vec<Value> = a.borrow().values().cloned().collect();
                    drop(ob);
                    for iter in &vals {
                        self.call_object_method(iter, b"next", &[]);
                    }
                }
                Some(Value::Null)
            }
            b"containsiterators" => {
                let ob = obj.borrow();
                let iters = ob.get_property(b"__spl_array");
                if let Value::Array(a) = iters {
                    Some(Value::Long(a.borrow().len() as i64))
                } else {
                    Some(Value::Long(0))
                }
            }
            b"countiterators" => {
                let ob = obj.borrow();
                let iters = ob.get_property(b"__spl_array");
                if let Value::Array(a) = iters {
                    Some(Value::Long(a.borrow().len() as i64))
                } else {
                    Some(Value::Long(0))
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
                    | b"ksort" | b"asort" | b"natsort" | b"natcasesort"
                    | b"uasort" | b"uksort" | b"setiteratorclass"
                    | b"seek"
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
                b"attach" | b"detach" | b"contains" | b"offsetget" | b"offsetset" | b"offsetexists" | b"seek"
            ),
            b"splpriorityqueue" => matches!(method, b"insert" | b"extract"),
            b"splheap" | b"splminheap" | b"splmaxheap" => matches!(
                method,
                b"insert" | b"extract" | b"next"
            ),
            b"appenditerator" => matches!(method, b"append"),
            b"multipleiterator" => matches!(method, b"attachiterator"),
            b"cachingiterator" | b"recursivecachingiterator" => matches!(method, b"setflags"),
            b"datetime" | b"datetimeimmutable" => matches!(method, b"format" | b"modify" | b"settimezone" | b"gettimezone" | b"settime" | b"setdate" | b"settimestamp" | b"add" | b"sub" | b"diff" | b"getoffset"),
            b"dateinterval" => matches!(method, b"format"),
            b"datetimezone" => matches!(method, b"getname" | b"getoffset"),
            b"reflectionclass" | b"reflectionobject" | b"reflectionenum" => matches!(
                method,
                b"getmethod" | b"getproperty" | b"getconstant" | b"hasconstant"
                    | b"hasmethod" | b"hasproperty" | b"issubclassof"
                    | b"implementsinterface" | b"isinstance"
                    | b"newinstance" | b"newinstanceargs"
                    | b"getstaticpropertyvalue" | b"setstaticpropertyvalue"
                    | b"getmethods" | b"getproperties"
                    | b"getreflectionconstant" | b"getreflectionconstants"
                    | b"getconstants"
            ),
            b"reflectionmethod" => matches!(
                method,
                b"invoke" | b"invokeargs"
                    | b"getmethod" | b"getproperty" | b"getconstant" | b"hasconstant"
                    | b"hasmethod" | b"hasproperty" | b"issubclassof"
                    | b"implementsinterface"
            ),
            b"reflectionfunction" | b"reflectionfunctionabstract" => matches!(
                method,
                b"invoke" | b"invokeargs"
            ),
            b"reflectionproperty" => matches!(
                method,
                b"getvalue" | b"setvalue"
            ),
            _ => false,
        }
    }

    /// Handle SPL method calls with arguments at DoCall time
    pub(crate) fn handle_spl_docall(
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
                        b"setiteratorclass" => {
                            let class_name = args.get(1).cloned().unwrap_or(Value::String(PhpString::from_bytes(b"ArrayIterator")));
                            let mut ob = obj.borrow_mut();
                            ob.set_property(b"__spl_iterator_class".to_vec(), class_name);
                            Some(Value::Null)
                        }
                        b"ksort" => {
                            let _sort_flags = args.get(1).map(|v| v.to_long()).unwrap_or(0);
                            let ob = obj.borrow();
                            let arr = ob.get_property(b"__spl_array");
                            if let Value::Array(a) = arr {
                                let mut entries: Vec<_> = a.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                                entries.sort_by(|(a_key, _), (b_key, _)| {
                                    match (a_key, b_key) {
                                        (ArrayKey::Int(a), ArrayKey::Int(b)) => a.cmp(b),
                                        (ArrayKey::String(a), ArrayKey::String(b)) => a.as_bytes().cmp(b.as_bytes()),
                                        (ArrayKey::Int(_), ArrayKey::String(_)) => std::cmp::Ordering::Less,
                                        (ArrayKey::String(_), ArrayKey::Int(_)) => std::cmp::Ordering::Greater,
                                    }
                                });
                                let mut new_arr = PhpArray::new();
                                for (k, v) in entries {
                                    new_arr.set(k, v);
                                }
                                *a.borrow_mut() = new_arr;
                            }
                            Some(Value::True)
                        }
                        b"asort" => {
                            let _sort_flags = args.get(1).map(|v| v.to_long()).unwrap_or(0);
                            let ob = obj.borrow();
                            let arr = ob.get_property(b"__spl_array");
                            if let Value::Array(a) = arr {
                                let mut entries: Vec<_> = a.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                                entries.sort_by(|(_, a_val), (_, b_val)| {
                                    Self::php_compare_values(a_val, b_val)
                                });
                                let mut new_arr = PhpArray::new();
                                for (k, v) in entries {
                                    new_arr.set(k, v);
                                }
                                *a.borrow_mut() = new_arr;
                            }
                            Some(Value::True)
                        }
                        b"natsort" => {
                            let ob = obj.borrow();
                            let arr = ob.get_property(b"__spl_array");
                            if let Value::Array(a) = arr {
                                let mut entries: Vec<_> = a.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                                entries.sort_by(|(_, a_val), (_, b_val)| {
                                    let a_str = a_val.to_php_string().to_string_lossy();
                                    let b_str = b_val.to_php_string().to_string_lossy();
                                    Self::strnatcmp(&a_str, &b_str, false)
                                });
                                let mut new_arr = PhpArray::new();
                                for (k, v) in entries {
                                    new_arr.set(k, v);
                                }
                                *a.borrow_mut() = new_arr;
                            }
                            Some(Value::True)
                        }
                        b"natcasesort" => {
                            let ob = obj.borrow();
                            let arr = ob.get_property(b"__spl_array");
                            if let Value::Array(a) = arr {
                                let mut entries: Vec<_> = a.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                                entries.sort_by(|(_, a_val), (_, b_val)| {
                                    let a_str = a_val.to_php_string().to_string_lossy();
                                    let b_str = b_val.to_php_string().to_string_lossy();
                                    Self::strnatcmp(&a_str, &b_str, true)
                                });
                                let mut new_arr = PhpArray::new();
                                for (k, v) in entries {
                                    new_arr.set(k, v);
                                }
                                *a.borrow_mut() = new_arr;
                            }
                            Some(Value::True)
                        }
                        b"uasort" => {
                            // User comparison sort by value - need to call callback
                            let callback = args.get(1).cloned().unwrap_or(Value::Null);
                            let ob = obj.borrow();
                            let arr = ob.get_property(b"__spl_array");
                            if let Value::Array(a) = arr {
                                let mut entries: Vec<_> = a.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                                drop(ob);
                                // Pre-compute comparison using insertion sort to avoid borrow issues
                                let len = entries.len();
                                for i in 1..len {
                                    let mut j = i;
                                    while j > 0 {
                                        let cmp_result = self.spl_call_compare_callback(&callback, &entries[j-1].1, &entries[j].1);
                                        if cmp_result > 0 {
                                            entries.swap(j-1, j);
                                            j -= 1;
                                        } else {
                                            break;
                                        }
                                    }
                                }
                                let arr_ref = obj.borrow().get_property(b"__spl_array");
                                if let Value::Array(a) = arr_ref {
                                    let mut new_arr = PhpArray::new();
                                    for (k, v) in entries {
                                        new_arr.set(k, v);
                                    }
                                    *a.borrow_mut() = new_arr;
                                }
                            }
                            Some(Value::True)
                        }
                        b"uksort" => {
                            let callback = args.get(1).cloned().unwrap_or(Value::Null);
                            let ob = obj.borrow();
                            let arr = ob.get_property(b"__spl_array");
                            if let Value::Array(a) = arr {
                                let mut entries: Vec<_> = a.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                                drop(ob);
                                let len = entries.len();
                                for i in 1..len {
                                    let mut j = i;
                                    while j > 0 {
                                        let a_val = match &entries[j-1].0 {
                                            ArrayKey::Int(n) => Value::Long(*n),
                                            ArrayKey::String(s) => Value::String(s.clone()),
                                        };
                                        let b_val = match &entries[j].0 {
                                            ArrayKey::Int(n) => Value::Long(*n),
                                            ArrayKey::String(s) => Value::String(s.clone()),
                                        };
                                        let cmp_result = self.spl_call_compare_callback(&callback, &a_val, &b_val);
                                        if cmp_result > 0 {
                                            entries.swap(j-1, j);
                                            j -= 1;
                                        } else {
                                            break;
                                        }
                                    }
                                }
                                let arr_ref = obj.borrow().get_property(b"__spl_array");
                                if let Value::Array(a) = arr_ref {
                                    let mut new_arr = PhpArray::new();
                                    for (k, v) in entries {
                                        new_arr.set(k, v);
                                    }
                                    *a.borrow_mut() = new_arr;
                                }
                            }
                            Some(Value::True)
                        }
                        b"seek" => {
                            let pos = args.get(1).map(|v| v.to_long()).unwrap_or(0);
                            let mut ob = obj.borrow_mut();
                            ob.set_property(b"__spl_pos".to_vec(), Value::Long(pos));
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
                            let mode_val = mode.to_long();
                            // Check restrictions for SplStack and SplQueue
                            let ob = obj.borrow();
                            let class_lower_name: Vec<u8> = ob.class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                            drop(ob);
                            let is_stack = class_lower_name == b"SplStack".to_ascii_lowercase().as_slice();
                            let is_queue = class_lower_name == b"SplQueue".to_ascii_lowercase().as_slice();
                            if is_stack && (mode_val & 2) == 0 {
                                // SplStack must be LIFO
                                let exc = self.create_exception(b"RuntimeException", "Iterators' LIFO/FIFO modes for SplStack/SplQueue objects are frozen", 0);
                                self.current_exception = Some(exc);
                                return Some(Value::Null);
                            }
                            if is_queue && (mode_val & 2) != 0 {
                                // SplQueue must be FIFO
                                let exc = self.create_exception(b"RuntimeException", "Iterators' LIFO/FIFO modes for SplStack/SplQueue objects are frozen", 0);
                                self.current_exception = Some(exc);
                                return Some(Value::Null);
                            }
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
                                } else if (size as usize) < current {
                                    // Truncate: rebuild array with only the first `size` elements
                                    let mut new_arr = PhpArray::new();
                                    for i in 0..size {
                                        let key = ArrayKey::Int(i);
                                        let val = a.get(&key).cloned().unwrap_or(Value::Null);
                                        new_arr.set(key, val);
                                    }
                                    *a = new_arr;
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
                                        ArrayKey::String(PhpString::from_string(hash.clone())),
                                        data,
                                    );
                                }
                                // Also store the object reference
                                let objects = ob.get_property(b"__spl_objects");
                                if let Value::Array(o) = objects {
                                    o.borrow_mut().set(
                                        ArrayKey::String(PhpString::from_string(hash)),
                                        key_obj.clone(),
                                    );
                                } else {
                                    drop(ob);
                                    let mut objs = PhpArray::new();
                                    objs.set(ArrayKey::String(PhpString::from_string(hash)), key_obj.clone());
                                    obj.borrow_mut().set_property(b"__spl_objects".to_vec(), Value::Array(Rc::new(RefCell::new(objs))));
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
                                    a.borrow_mut().remove(&ArrayKey::String(PhpString::from_string(hash.clone())));
                                }
                                let objects = ob.get_property(b"__spl_objects");
                                if let Value::Array(o) = objects {
                                    o.borrow_mut().remove(&ArrayKey::String(PhpString::from_string(hash)));
                                }
                            }
                            Some(Value::Null)
                        }
                        b"contains" | b"offsetexists" => {
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
                        b"offsetget" => {
                            let key_obj = args.get(1)?;
                            if let Value::Object(key_o) = key_obj {
                                let hash = format!("{:016x}", key_o.borrow().object_id);
                                let ob = obj.borrow();
                                let arr = ob.get_property(b"__spl_array");
                                if let Value::Array(a) = arr {
                                    Some(a.borrow().get(&ArrayKey::String(PhpString::from_string(hash))).cloned().unwrap_or(Value::Null))
                                } else {
                                    Some(Value::Null)
                                }
                            } else {
                                Some(Value::Null)
                            }
                        }
                        b"offsetset" => {
                            let key_obj = args.get(1)?;
                            let val = args.get(2).cloned().unwrap_or(Value::Null);
                            if let Value::Object(key_o) = key_obj {
                                let hash = format!("{:016x}", key_o.borrow().object_id);
                                let ob = obj.borrow();
                                let arr = ob.get_property(b"__spl_array");
                                if let Value::Array(a) = arr {
                                    a.borrow_mut().set(
                                        ArrayKey::String(PhpString::from_string(hash.clone())),
                                        val,
                                    );
                                }
                                // Also store the object reference
                                let objects = ob.get_property(b"__spl_objects");
                                if let Value::Array(o) = objects {
                                    o.borrow_mut().set(
                                        ArrayKey::String(PhpString::from_string(hash)),
                                        key_obj.clone(),
                                    );
                                }
                            }
                            Some(Value::Null)
                        }
                        b"seek" => {
                            let pos = args.get(1).map(|v| v.to_long()).unwrap_or(0);
                            let ob = obj.borrow();
                            let arr = ob.get_property(b"__spl_array");
                            let len = if let Value::Array(a) = &arr { a.borrow().len() as i64 } else { 0 };
                            if pos < 0 || pos >= len {
                                drop(ob);
                                let exc = self.create_exception(b"OutOfBoundsException",
                                    &format!("Seek position {} is out of range", pos), 0);
                                self.current_exception = Some(exc);
                                return Some(Value::Null);
                            }
                            drop(ob);
                            obj.borrow_mut().set_property(b"__spl_pos".to_vec(), Value::Long(pos));
                            Some(Value::Null)
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
                b"splheap" | b"splminheap" | b"splmaxheap" => {
                    match method {
                        b"insert" => {
                            let val = args.get(1).cloned().unwrap_or(Value::Null);
                            let is_min = class == b"splminheap";
                            let is_max = class == b"splmaxheap";
                            // Check if this object has a user-defined compare() method
                            let obj_class_lower: Vec<u8> = obj.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                            let has_user_compare = self.classes.get(&obj_class_lower)
                                .map(|c| c.get_method(b"compare").is_some())
                                .unwrap_or(false);
                            let this_val = this.clone();

                            let mut ob = obj.borrow_mut();
                            let arr = ob.get_property(b"__spl_array");
                            if let Value::Array(a) = arr {
                                a.borrow_mut().push(val.clone());
                                let mut entries: Vec<Value> = a.borrow().values().cloned().collect();
                                drop(ob);
                                let len = entries.len();
                                if len > 1 {
                                    if has_user_compare {
                                        // Use insertion sort with user compare
                                        for i in 1..len {
                                            let mut j = i;
                                            while j > 0 {
                                                let cmp = self.call_object_method(&this_val, b"compare", &[entries[j].clone(), entries[j-1].clone()])
                                                    .unwrap_or(Value::Long(0)).to_long();
                                                if cmp > 0 {
                                                    entries.swap(j-1, j);
                                                    j -= 1;
                                                } else {
                                                    break;
                                                }
                                            }
                                        }
                                    } else {
                                        entries.sort_by(|a_v, b_v| {
                                            let cmp = Self::php_compare_values(a_v, b_v);
                                            if is_min { cmp } else { cmp.reverse() }
                                        });
                                    }
                                    let arr_ref = obj.borrow().get_property(b"__spl_array");
                                    if let Value::Array(a) = arr_ref {
                                        let mut new_arr = PhpArray::new();
                                        for v in entries {
                                            new_arr.push(v);
                                        }
                                        *a.borrow_mut() = new_arr;
                                    }
                                }
                            } else {
                                let mut new_arr = PhpArray::new();
                                new_arr.push(val);
                                ob.set_property(b"__spl_array".to_vec(), Value::Array(Rc::new(RefCell::new(new_arr))));
                            }
                            Some(Value::True)
                        }
                        b"extract" | b"next" => {
                            let ob = obj.borrow();
                            let arr = ob.get_property(b"__spl_array");
                            if let Value::Array(a) = arr {
                                let mut a = a.borrow_mut();
                                if a.len() == 0 {
                                    drop(a);
                                    drop(ob);
                                    let exc = self.create_exception(b"RuntimeException", "Can't extract from an empty heap", 0);
                                    self.current_exception = Some(exc);
                                    Some(Value::Null)
                                } else {
                                    // Extract the top (first element)
                                    let val = a.shift().unwrap_or(Value::Null);
                                    drop(a);
                                    drop(ob);
                                    // Update key counter
                                    let mut ob = obj.borrow_mut();
                                    let key = ob.get_property(b"__spl_heap_key");
                                    let key_val = if let Value::Long(k) = key { k } else { 0 };
                                    ob.set_property(b"__spl_heap_key".to_vec(), Value::Long(key_val + 1));
                                    if method == b"extract" {
                                        Some(val)
                                    } else {
                                        Some(Value::Null)
                                    }
                                }
                            } else {
                                drop(ob);
                                let exc = self.create_exception(b"RuntimeException", "Can't extract from an empty heap", 0);
                                self.current_exception = Some(exc);
                                Some(Value::Null)
                            }
                        }
                        _ => None,
                    }
                }
                b"appenditerator" => {
                    match method {
                        b"append" => {
                            let iter = args.get(1).cloned().unwrap_or(Value::Null);
                            let ob = obj.borrow();
                            let arr = ob.get_property(b"__spl_array");
                            if let Value::Array(a) = arr {
                                a.borrow_mut().push(iter);
                            } else {
                                drop(ob);
                                let mut new_arr = PhpArray::new();
                                new_arr.push(iter);
                                obj.borrow_mut().set_property(b"__spl_array".to_vec(), Value::Array(Rc::new(RefCell::new(new_arr))));
                            }
                            Some(Value::Null)
                        }
                        _ => None,
                    }
                }
                b"multipleiterator" => {
                    match method {
                        b"attachiterator" => {
                            let iter = args.get(1).cloned().unwrap_or(Value::Null);
                            let ob = obj.borrow();
                            let arr = ob.get_property(b"__spl_array");
                            if let Value::Array(a) = arr {
                                a.borrow_mut().push(iter);
                            } else {
                                drop(ob);
                                let mut new_arr = PhpArray::new();
                                new_arr.push(iter);
                                obj.borrow_mut().set_property(b"__spl_array".to_vec(), Value::Array(Rc::new(RefCell::new(new_arr))));
                            }
                            Some(Value::Null)
                        }
                        _ => None,
                    }
                }
                b"cachingiterator" | b"recursivecachingiterator" => {
                    match method {
                        b"setflags" => {
                            let flags = args.get(1).cloned().unwrap_or(Value::Long(0));
                            let mut ob = obj.borrow_mut();
                            ob.set_property(b"__spl_flags".to_vec(), flags);
                            Some(Value::Null)
                        }
                        _ => None,
                    }
                }
                b"datetime" | b"datetimeimmutable" => {
                    let is_immutable = class == b"datetimeimmutable";
                    match method {
                        b"format" => {
                            let format_str = args.get(1).cloned().unwrap_or(Value::Null).to_php_string().to_string_lossy();
                            let ob = obj.borrow();
                            let timestamp = ob.get_property(b"__timestamp").to_long();
                            let result = self.format_datetime_timestamp(&format_str, timestamp);
                            Some(Value::String(PhpString::from_string(result)))
                        }
                        b"gettimestamp" => {
                            let ob = obj.borrow();
                            Some(ob.get_property(b"__timestamp"))
                        }
                        b"settimestamp" => {
                            let ts = args.get(1).cloned().unwrap_or(Value::Null).to_long();
                            if is_immutable {
                                // Return new object
                                let obj_id = self.next_object_id;
                                self.next_object_id += 1;
                                let mut new_obj = PhpObject::new(b"DateTimeImmutable".to_vec(), obj_id);
                                new_obj.set_property(b"__timestamp".to_vec(), Value::Long(ts));
                                Some(Value::Object(Rc::new(RefCell::new(new_obj))))
                            } else {
                                obj.borrow_mut().set_property(b"__timestamp".to_vec(), Value::Long(ts));
                                Some(this.clone())
                            }
                        }
                        b"modify" => {
                            let modifier = args.get(1).cloned().unwrap_or(Value::Null).to_php_string().to_string_lossy();
                            let ts = obj.borrow().get_property(b"__timestamp").to_long();
                            if let Some(new_ts) = vm_apply_relative_modification(&modifier, ts) {
                                if is_immutable {
                                    let obj_id = self.next_object_id;
                                    self.next_object_id += 1;
                                    let mut new_obj = PhpObject::new(b"DateTimeImmutable".to_vec(), obj_id);
                                    new_obj.set_property(b"__timestamp".to_vec(), Value::Long(new_ts));
                                    Some(Value::Object(Rc::new(RefCell::new(new_obj))))
                                } else {
                                    obj.borrow_mut().set_property(b"__timestamp".to_vec(), Value::Long(new_ts));
                                    Some(this.clone())
                                }
                            } else {
                                Some(Value::False)
                            }
                        }
                        b"settimezone" => {
                            // For now just return $this (we don't really handle timezones)
                            if is_immutable {
                                let obj_id = self.next_object_id;
                                self.next_object_id += 1;
                                let ts = obj.borrow().get_property(b"__timestamp").to_long();
                                let mut new_obj = PhpObject::new(b"DateTimeImmutable".to_vec(), obj_id);
                                new_obj.set_property(b"__timestamp".to_vec(), Value::Long(ts));
                                Some(Value::Object(Rc::new(RefCell::new(new_obj))))
                            } else {
                                Some(this.clone())
                            }
                        }
                        b"gettimezone" => {
                            let obj_id = self.next_object_id;
                            self.next_object_id += 1;
                            let mut tz_obj = PhpObject::new(b"DateTimeZone".to_vec(), obj_id);
                            tz_obj.set_property(b"timezone".to_vec(), Value::String(PhpString::from_bytes(b"UTC")));
                            Some(Value::Object(Rc::new(RefCell::new(tz_obj))))
                        }
                        b"setdate" => {
                            let year = args.get(1).cloned().unwrap_or(Value::Null).to_long();
                            let month = args.get(2).cloned().unwrap_or(Value::Null).to_long() as u32;
                            let day = args.get(3).cloned().unwrap_or(Value::Null).to_long() as u32;
                            let ts = obj.borrow().get_property(b"__timestamp").to_long();
                            let time_of_day = ((ts % 86400) + 86400) % 86400;
                            let new_days = vm_ymd_to_days(year, month, day);
                            let new_ts = new_days * 86400 + time_of_day;
                            if is_immutable {
                                let obj_id = self.next_object_id;
                                self.next_object_id += 1;
                                let mut new_obj = PhpObject::new(b"DateTimeImmutable".to_vec(), obj_id);
                                new_obj.set_property(b"__timestamp".to_vec(), Value::Long(new_ts));
                                Some(Value::Object(Rc::new(RefCell::new(new_obj))))
                            } else {
                                obj.borrow_mut().set_property(b"__timestamp".to_vec(), Value::Long(new_ts));
                                Some(this.clone())
                            }
                        }
                        b"settime" => {
                            let hour = args.get(1).cloned().unwrap_or(Value::Null).to_long();
                            let minute = args.get(2).cloned().unwrap_or(Value::Null).to_long();
                            let second = args.get(3).map(|v| v.to_long()).unwrap_or(0);
                            let ts = obj.borrow().get_property(b"__timestamp").to_long();
                            let days = ts / 86400;
                            let new_ts = days * 86400 + hour * 3600 + minute * 60 + second;
                            if is_immutable {
                                let obj_id = self.next_object_id;
                                self.next_object_id += 1;
                                let mut new_obj = PhpObject::new(b"DateTimeImmutable".to_vec(), obj_id);
                                new_obj.set_property(b"__timestamp".to_vec(), Value::Long(new_ts));
                                Some(Value::Object(Rc::new(RefCell::new(new_obj))))
                            } else {
                                obj.borrow_mut().set_property(b"__timestamp".to_vec(), Value::Long(new_ts));
                                Some(this.clone())
                            }
                        }
                        b"diff" => {
                            let other = args.get(1).cloned().unwrap_or(Value::Null);
                            let absolute = args.get(2).map(|v| v.is_truthy()).unwrap_or(false);
                            let ts1 = obj.borrow().get_property(b"__timestamp").to_long();
                            let ts2 = if let Value::Object(o2) = &other {
                                o2.borrow().get_property(b"__timestamp").to_long()
                            } else {
                                0
                            };
                            Some(create_date_interval_from_timestamps(self, ts1, ts2, absolute))
                        }
                        b"add" => {
                            // DateTime::add(DateInterval $interval) - add interval
                            let interval = args.get(1).cloned().unwrap_or(Value::Null);
                            let ts = obj.borrow().get_property(b"__timestamp").to_long();
                            let new_ts = if let Value::Object(iv) = &interval {
                                let iv = iv.borrow();
                                let y = iv.get_property(b"y").to_long();
                                let m = iv.get_property(b"m").to_long();
                                let d = iv.get_property(b"d").to_long();
                                let h = iv.get_property(b"h").to_long();
                                let i = iv.get_property(b"i").to_long();
                                let s = iv.get_property(b"s").to_long();
                                let invert = iv.get_property(b"invert").to_long();
                                let sign: i64 = if invert != 0 { -1 } else { 1 };
                                let mut result_ts = ts;
                                // Apply year/month changes
                                if y != 0 || m != 0 {
                                    let days_now = result_ts / 86400;
                                    let tod = ((result_ts % 86400) + 86400) % 86400;
                                    let (cy, cm, cd) = vm_days_to_ymd(days_now);
                                    let new_m = cm as i64 + sign * m;
                                    let total_months = (cy * 12 + new_m - 1 + sign * y * 12) as i64;
                                    let ny = total_months / 12;
                                    let nm = (total_months % 12 + 1) as u32;
                                    let is_leap = ny % 4 == 0 && (ny % 100 != 0 || ny % 400 == 0);
                                    let max_d = match nm { 2 => if is_leap {29} else {28}, 4|6|9|11 => 30, _ => 31 };
                                    let nd = cd.min(max_d);
                                    let new_days = vm_ymd_to_days(ny, nm, nd);
                                    result_ts = new_days * 86400 + tod;
                                }
                                result_ts += sign * (d * 86400 + h * 3600 + i * 60 + s);
                                result_ts
                            } else { ts };
                            if is_immutable {
                                let obj_id = self.next_object_id;
                                self.next_object_id += 1;
                                let mut new_obj = PhpObject::new(b"DateTimeImmutable".to_vec(), obj_id);
                                new_obj.set_property(b"__timestamp".to_vec(), Value::Long(new_ts));
                                Some(Value::Object(Rc::new(RefCell::new(new_obj))))
                            } else {
                                obj.borrow_mut().set_property(b"__timestamp".to_vec(), Value::Long(new_ts));
                                Some(this.clone())
                            }
                        }
                        b"sub" => {
                            // DateTime::sub(DateInterval $interval) - subtract interval
                            let interval = args.get(1).cloned().unwrap_or(Value::Null);
                            let ts = obj.borrow().get_property(b"__timestamp").to_long();
                            let new_ts = if let Value::Object(iv) = &interval {
                                let iv = iv.borrow();
                                let y = iv.get_property(b"y").to_long();
                                let m = iv.get_property(b"m").to_long();
                                let d = iv.get_property(b"d").to_long();
                                let h = iv.get_property(b"h").to_long();
                                let i = iv.get_property(b"i").to_long();
                                let s = iv.get_property(b"s").to_long();
                                let invert = iv.get_property(b"invert").to_long();
                                let sign: i64 = if invert != 0 { 1 } else { -1 };
                                let mut result_ts = ts;
                                if y != 0 || m != 0 {
                                    let days_now = result_ts / 86400;
                                    let tod = ((result_ts % 86400) + 86400) % 86400;
                                    let (cy, cm, cd) = vm_days_to_ymd(days_now);
                                    let new_m = cm as i64 + sign * m;
                                    let total_months = (cy * 12 + new_m - 1 + sign * y * 12) as i64;
                                    let ny = total_months / 12;
                                    let nm = (total_months % 12 + 1) as u32;
                                    let is_leap = ny % 4 == 0 && (ny % 100 != 0 || ny % 400 == 0);
                                    let max_d = match nm { 2 => if is_leap {29} else {28}, 4|6|9|11 => 30, _ => 31 };
                                    let nd = cd.min(max_d);
                                    let new_days = vm_ymd_to_days(ny, nm, nd);
                                    result_ts = new_days * 86400 + tod;
                                }
                                result_ts += sign * (d * 86400 + h * 3600 + i * 60 + s);
                                result_ts
                            } else { ts };
                            if is_immutable {
                                let obj_id = self.next_object_id;
                                self.next_object_id += 1;
                                let mut new_obj = PhpObject::new(b"DateTimeImmutable".to_vec(), obj_id);
                                new_obj.set_property(b"__timestamp".to_vec(), Value::Long(new_ts));
                                Some(Value::Object(Rc::new(RefCell::new(new_obj))))
                            } else {
                                obj.borrow_mut().set_property(b"__timestamp".to_vec(), Value::Long(new_ts));
                                Some(this.clone())
                            }
                        }
                        b"getoffset" => Some(Value::Long(0)), // UTC
                        _ => None,
                    }
                }
                b"dateinterval" => {
                    match method {
                        b"format" => {
                            let format_str = args.get(1).cloned().unwrap_or(Value::Null).to_php_string().to_string_lossy();
                            let ob = obj.borrow();
                            let y = ob.get_property(b"y").to_long();
                            let m = ob.get_property(b"m").to_long();
                            let d = ob.get_property(b"d").to_long();
                            let h = ob.get_property(b"h").to_long();
                            let i = ob.get_property(b"i").to_long();
                            let s = ob.get_property(b"s").to_long();
                            let days = ob.get_property(b"days");
                            let invert = ob.get_property(b"invert").to_long();

                            let mut result = String::new();
                            let fmt_bytes = format_str.as_bytes();
                            let mut fi = 0;
                            while fi < fmt_bytes.len() {
                                if fmt_bytes[fi] == b'%' && fi + 1 < fmt_bytes.len() {
                                    fi += 1;
                                    match fmt_bytes[fi] {
                                        b'Y' => result.push_str(&format!("{:04}", y)),
                                        b'y' => result.push_str(&format!("{}", y)),
                                        b'M' => result.push_str(&format!("{:02}", m)),
                                        b'm' => result.push_str(&format!("{}", m)),
                                        b'D' => result.push_str(&format!("{:02}", d)),
                                        b'd' => result.push_str(&format!("{}", d)),
                                        b'H' => result.push_str(&format!("{:02}", h)),
                                        b'h' => result.push_str(&format!("{}", h)),
                                        b'I' => result.push_str(&format!("{:02}", i)),
                                        b'i' => result.push_str(&format!("{}", i)),
                                        b'S' => result.push_str(&format!("{:02}", s)),
                                        b's' => result.push_str(&format!("{}", s)),
                                        b'a' => {
                                            if let Value::Long(d_val) = &days {
                                                result.push_str(&format!("{}", d_val));
                                            } else {
                                                result.push_str("(unknown)");
                                            }
                                        }
                                        b'R' => result.push(if invert != 0 { '-' } else { '+' }),
                                        b'r' => { if invert != 0 { result.push('-'); } }
                                        b'%' => result.push('%'),
                                        _ => { result.push('%'); result.push(fmt_bytes[fi] as char); }
                                    }
                                } else {
                                    result.push(fmt_bytes[fi] as char);
                                }
                                fi += 1;
                            }
                            Some(Value::String(PhpString::from_string(result)))
                        }
                        _ => None,
                    }
                }
                b"datetimezone" => {
                    match method {
                        b"getname" => {
                            let ob = obj.borrow();
                            let tz = ob.get_property(b"timezone");
                            if matches!(tz, Value::Null) {
                                Some(Value::String(PhpString::from_bytes(b"UTC")))
                            } else {
                                Some(tz)
                            }
                        }
                        b"getoffset" => Some(Value::Long(0)),
                        _ => None,
                    }
                }
                b"reflectionclass" | b"reflectionobject" | b"reflectionenum" => {
                    self.reflection_class_docall(method, args)
                }
                b"reflectionmethod" => {
                    self.reflection_method_docall(method, args)
                }
                b"reflectionfunction" | b"reflectionfunctionabstract" => {
                    self.reflection_function_docall(method, args)
                }
                b"reflectionproperty" => {
                    self.reflection_property_docall(method, args)
                }
                _ => None,
            }
        } else {
            None
        }
    }

    // ==================== Reflection API implementation ====================

    /// ReflectionClass constructor: sets up __reflection_target and name properties
    fn reflection_class_construct(&mut self, args: &[Value], line: u32) -> bool {
        let this = match args.first() {
            Some(Value::Object(o)) => o.clone(),
            _ => return true,
        };
        let arg = args.get(1).cloned().unwrap_or(Value::Null);

        let class_name = match &arg {
            Value::Object(obj) => {
                let ob = obj.borrow();
                String::from_utf8_lossy(&ob.class_name).to_string()
            }
            Value::String(s) => {
                let name = s.to_string_lossy();
                // Closure strings should map to "Closure" class
                if name.starts_with("__closure_") || name.starts_with("__arrow_") || name.starts_with("__bound_closure_") || name.starts_with("__closure_fcc_") {
                    "Closure".to_string()
                } else {
                    name
                }
            }
            _ => arg.to_php_string().to_string_lossy(),
        };

        // Strip leading backslash
        let class_name = if class_name.starts_with('\\') {
            class_name[1..].to_string()
        } else {
            class_name
        };
        let class_lower: Vec<u8> = class_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();

        // Check if class exists
        let class_exists = self.classes.contains_key(&class_lower)
            || self.is_known_builtin_class(&class_lower);

        if !class_exists {
            let err_msg = format!("Class \"{}\" does not exist", class_name);
            let exc = self.create_exception(b"ReflectionException", &err_msg, line);
            self.current_exception = Some(exc);
            return false;
        }

        // Get canonical name
        let canonical = if let Some(ce) = self.classes.get(&class_lower) {
            String::from_utf8_lossy(&ce.name).to_string()
        } else {
            // Built-in class - use proper casing
            self.builtin_canonical_name(&class_lower)
        };

        let mut obj = this.borrow_mut();
        obj.set_property(b"name".to_vec(), Value::String(PhpString::from_string(canonical.clone())));
        obj.set_property(b"__reflection_target".to_vec(), Value::String(PhpString::from_string(canonical)));
        // If constructed from an object, store the object reference
        if let Value::Object(_) = &arg {
            obj.set_property(b"__reflection_object".to_vec(), arg);
        }
        true
    }

    /// Check if a class name is a known built-in class
    fn is_known_builtin_class(&self, class_lower: &[u8]) -> bool {
        matches!(
            class_lower,
            b"stdclass" | b"exception" | b"error" | b"typeerror" | b"valueerror"
                | b"runtimeexception" | b"logicexception" | b"invalidargumentexception"
                | b"badmethodcallexception" | b"closure" | b"generator"
                | b"reflectionclass" | b"reflectionobject" | b"reflectionmethod"
                | b"reflectionfunction" | b"reflectionproperty" | b"reflectionparameter"
                | b"reflectionextension" | b"reflectionexception"
                | b"reflectionfunctionabstract" | b"reflectionnamedtype"
                | b"reflectionuniontype" | b"reflectionintersectiontype"
                | b"reflectionenum" | b"reflectionenumunitcase" | b"reflectionenumbackedcase"
                | b"reflectionclassconstant" | b"reflectiongenerator"
                | b"reflectionattribute"
                | b"arithmeticerror" | b"divisionbyzeroerror" | b"argumentcounterror"
                | b"rangeerror" | b"unhandledmatcherror" | b"assertionerror"
                | b"closedgeneratorexception" | b"badfunctioncallexception"
                | b"overflowexception" | b"underflowexception" | b"outofboundsexception"
                | b"domainexception" | b"unexpectedvalueexception"
                | b"lengthexception" | b"outofrangeexception" | b"errorexception"
                | b"arrayobject" | b"arrayiterator" | b"recursivearrayiterator" | b"splfixedarray"
                | b"spldoublylinkedlist" | b"splstack" | b"splqueue"
                | b"splpriorityqueue" | b"splobjectstorage"
                | b"splheap" | b"splminheap" | b"splmaxheap"
                | b"emptyiterator" | b"iteratoriterator" | b"recursiveiteratoriterator"
                | b"norewinditerator" | b"infiniteiterator" | b"limititerator"
                | b"cachingiterator" | b"recursivecachingiterator"
                | b"appenditerator" | b"filteriterator" | b"callbackfilteriterator"
                | b"recursivefilteriterator" | b"recursivecallbackfilteriterator"
                | b"regexiterator" | b"recursiveregexiterator"
                | b"multipleiterator" | b"parentiterator"
                | b"spltempfileobject" | b"splfileinfo" | b"splfileobject"
                | b"directoryiterator" | b"filesystemiterator" | b"recursivedirectoryiterator"
                | b"globiterator"
                | b"datetime" | b"datetimeimmutable" | b"datetimezone" | b"dateinterval"
                | b"throwable" | b"stringable" | b"traversable" | b"iterator"
                | b"iteratoraggregate" | b"arrayaccess" | b"countable" | b"serializable"
                | b"seekableiterator" | b"outeriterator" | b"recursiveiterator"
                | b"splobserver" | b"splsubject"
                | b"reflectiontype" | b"reflectionreference" | b"reflectionconstant"
        )
    }

    /// Get canonical (properly cased) name for built-in class
    fn builtin_canonical_name(&self, class_lower: &[u8]) -> String {
        let canonical = match class_lower {
            b"stdclass" => "stdClass",
            b"exception" => "Exception",
            b"error" => "Error",
            b"typeerror" => "TypeError",
            b"valueerror" => "ValueError",
            b"runtimeexception" => "RuntimeException",
            b"logicexception" => "LogicException",
            b"invalidargumentexception" => "InvalidArgumentException",
            b"badmethodcallexception" => "BadMethodCallException",
            b"badfunctioncallexception" => "BadFunctionCallException",
            b"overflowexception" => "OverflowException",
            b"underflowexception" => "UnderflowException",
            b"outofboundsexception" => "OutOfBoundsException",
            b"lengthexception" => "LengthException",
            b"outofrangeexception" => "OutOfRangeException",
            b"domainexception" => "DomainException",
            b"unexpectedvalueexception" => "UnexpectedValueException",
            b"closedgeneratorexception" => "ClosedGeneratorException",
            b"errorexception" => "ErrorException",
            b"arithmeticerror" => "ArithmeticError",
            b"divisionbyzeroerror" => "DivisionByZeroError",
            b"argumentcounterror" => "ArgumentCountError",
            b"rangeerror" => "RangeError",
            b"unhandledmatcherror" => "UnhandledMatchError",
            b"assertionerror" => "AssertionError",
            b"closure" => "Closure",
            b"generator" => "Generator",
            b"reflectionclass" => "ReflectionClass",
            b"reflectionobject" => "ReflectionObject",
            b"reflectionmethod" => "ReflectionMethod",
            b"reflectionfunction" => "ReflectionFunction",
            b"reflectionproperty" => "ReflectionProperty",
            b"reflectionparameter" => "ReflectionParameter",
            b"reflectionextension" => "ReflectionExtension",
            b"reflectionexception" => "ReflectionException",
            b"reflectionfunctionabstract" => "ReflectionFunctionAbstract",
            b"reflectionnamedtype" => "ReflectionNamedType",
            b"reflectionuniontype" => "ReflectionUnionType",
            b"reflectionintersectiontype" => "ReflectionIntersectionType",
            b"reflectionenum" => "ReflectionEnum",
            b"reflectionenumunitcase" => "ReflectionEnumUnitCase",
            b"reflectionenumbackedcase" => "ReflectionEnumBackedCase",
            b"reflectionclassconstant" => "ReflectionClassConstant",
            b"reflectiongenerator" => "ReflectionGenerator",
            b"reflectionattribute" => "ReflectionAttribute",
            b"arrayobject" => "ArrayObject",
            b"arrayiterator" => "ArrayIterator",
            b"recursivearrayiterator" => "RecursiveArrayIterator",
            b"splfixedarray" => "SplFixedArray",
            b"spldoublylinkedlist" => "SplDoublyLinkedList",
            b"splstack" => "SplStack",
            b"splqueue" => "SplQueue",
            b"splpriorityqueue" => "SplPriorityQueue",
            b"splobjectstorage" => "SplObjectStorage",
            b"splheap" => "SplHeap",
            b"splminheap" => "SplMinHeap",
            b"splmaxheap" => "SplMaxHeap",
            b"emptyiterator" => "EmptyIterator",
            b"iteratoriterator" => "IteratorIterator",
            b"recursiveiteratoriterator" => "RecursiveIteratorIterator",
            b"norewinditerator" => "NoRewindIterator",
            b"infiniteiterator" => "InfiniteIterator",
            b"limititerator" => "LimitIterator",
            b"cachingiterator" => "CachingIterator",
            b"recursivecachingiterator" => "RecursiveCachingIterator",
            b"appenditerator" => "AppendIterator",
            b"filteriterator" => "FilterIterator",
            b"callbackfilteriterator" => "CallbackFilterIterator",
            b"recursivefilteriterator" => "RecursiveFilterIterator",
            b"recursivecallbackfilteriterator" => "RecursiveCallbackFilterIterator",
            b"regexiterator" => "RegexIterator",
            b"recursiveregexiterator" => "RecursiveRegexIterator",
            b"multipleiterator" => "MultipleIterator",
            b"parentiterator" => "ParentIterator",
            b"splfileinfo" => "SplFileInfo",
            b"splfileobject" => "SplFileObject",
            b"spltempfileobject" => "SplTempFileObject",
            b"directoryiterator" => "DirectoryIterator",
            b"filesystemiterator" => "FilesystemIterator",
            b"recursivedirectoryiterator" => "RecursiveDirectoryIterator",
            b"globiterator" => "GlobIterator",
            b"datetime" => "DateTime",
            b"datetimeimmutable" => "DateTimeImmutable",
            b"dateinterval" => "DateInterval",
            b"datetimezone" => "DateTimeZone",
            _ => return String::from_utf8_lossy(class_lower).to_string(),
        };
        canonical.to_string()
    }

    /// ReflectionMethod constructor
    fn reflection_method_construct(&mut self, args: &[Value], line: u32) -> bool {
        let this = match args.first() {
            Some(Value::Object(o)) => o.clone(),
            _ => return true,
        };

        let (class_name, method_name) = if args.len() >= 3 {
            // new ReflectionMethod($class, $method)
            let class_arg = &args[1];
            let method_arg = args[2].to_php_string().to_string_lossy();
            let class_str = match class_arg {
                Value::Object(obj) => {
                    let ob = obj.borrow();
                    String::from_utf8_lossy(&ob.class_name).to_string()
                }
                Value::String(s) => {
                    let name = s.to_string_lossy();
                    // Closure strings map to "Closure" class
                    if name.starts_with("__closure_") || name.starts_with("__arrow_") || name.starts_with("__bound_closure_") || name.starts_with("__closure_fcc_") {
                        "Closure".to_string()
                    } else {
                        name
                    }
                }
                _ => class_arg.to_php_string().to_string_lossy(),
            };
            (class_str, method_arg)
        } else if args.len() >= 2 {
            // new ReflectionMethod('Class::method')
            let arg = args[1].to_php_string().to_string_lossy();
            if let Some(pos) = arg.find("::") {
                (arg[..pos].to_string(), arg[pos + 2..].to_string())
            } else {
                let err_msg = format!(
                    "ReflectionMethod::__construct(): Argument #1 ($objectOrMethod) must be a valid method name"
                );
                let exc = self.create_exception(b"ReflectionException", &err_msg, line);
                self.current_exception = Some(exc);
                return false;
            }
        } else {
            return true;
        };

        let class_lower: Vec<u8> = class_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
        let method_lower: Vec<u8> = method_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();

        // Check if class exists (user-defined or built-in)
        let is_user_class = self.classes.contains_key(&class_lower);
        let is_builtin_class = self.is_known_builtin_class(&class_lower);
        if !is_user_class && !is_builtin_class {
            let err_msg = format!("Class \"{}\" does not exist", class_name);
            let exc = self.create_exception(b"ReflectionException", &err_msg, line);
            self.current_exception = Some(exc);
            return false;
        }

        if is_user_class {
            // Check method exists in user class
            let method_exists = self.classes.get(&class_lower)
                .map(|c| c.get_method(&method_lower).is_some())
                .unwrap_or(false);

            if !method_exists {
                let canonical_class = self.classes.get(&class_lower)
                    .map(|c| String::from_utf8_lossy(&c.name).to_string())
                    .unwrap_or(class_name.clone());
                let err_msg = format!("Method {}::{}() does not exist", canonical_class, method_name);
                let exc = self.create_exception(b"ReflectionException", &err_msg, line);
                self.current_exception = Some(exc);
                return false;
            }
        }
        // For built-in classes, we accept all method names without checking

        let canonical_class = if is_user_class {
            self.classes.get(&class_lower)
                .map(|c| String::from_utf8_lossy(&c.name).to_string())
                .unwrap_or(class_name.clone())
        } else {
            self.builtin_canonical_name(&class_lower)
        };

        let canonical_method = if is_user_class {
            self.classes.get(&class_lower)
                .and_then(|c| c.get_method(&method_lower).map(|m| String::from_utf8_lossy(&m.name).to_string()))
                .unwrap_or(method_name.clone())
        } else {
            method_name.clone()
        };

        let mut obj = this.borrow_mut();
        obj.set_property(b"name".to_vec(), Value::String(PhpString::from_string(canonical_method)));
        obj.set_property(b"class".to_vec(), Value::String(PhpString::from_string(canonical_class.clone())));
        obj.set_property(b"__reflection_class".to_vec(), Value::String(PhpString::from_string(canonical_class)));
        obj.set_property(b"__reflection_method".to_vec(), Value::String(PhpString::from_vec(method_lower)));
        true
    }

    /// ReflectionFunction constructor
    fn reflection_function_construct(&mut self, args: &[Value], line: u32) -> bool {
        let this = match args.first() {
            Some(Value::Object(o)) => o.clone(),
            _ => return true,
        };
        let arg = args.get(1).cloned().unwrap_or(Value::Null);

        // Handle Closure objects
        if let Value::Object(closure_obj) = &arg {
            let class_lower: Vec<u8> = closure_obj.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
            if class_lower == b"closure" {
                let mut obj = this.borrow_mut();
                obj.set_property(b"name".to_vec(), Value::String(PhpString::from_bytes(b"{closure}")));
                obj.set_property(b"__reflection_target".to_vec(), Value::String(PhpString::from_bytes(b"")));
                obj.set_property(b"__reflection_is_closure".to_vec(), Value::True);
                return true;
            }
        }

        // Handle array callables like [ClassName, method] - these should throw for ReflectionFunction
        if let Value::Array(_) = &arg {
            let err_msg = "Function Array() does not exist".to_string();
            let exc = self.create_exception(b"ReflectionException", &err_msg, line);
            self.current_exception = Some(exc);
            return false;
        }

        let func_name = match &arg {
            Value::String(s) => {
                let name = s.to_string_lossy();
                // Check for closures
                if name.starts_with("__closure_") || name.starts_with("__arrow_") || name.starts_with("__bound_closure_") || name.starts_with("__closure_fcc_") {
                    let mut obj = this.borrow_mut();
                    obj.set_property(b"name".to_vec(), Value::String(PhpString::from_bytes(b"{closure}")));
                    obj.set_property(b"__reflection_target".to_vec(), Value::String(s.clone()));
                    obj.set_property(b"__reflection_is_closure".to_vec(), Value::True);
                    return true;
                }
                name
            }
            _ => arg.to_php_string().to_string_lossy(),
        };

        // Strip leading backslash
        let func_name = if func_name.starts_with('\\') {
            func_name[1..].to_string()
        } else {
            func_name
        };
        let func_lower: Vec<u8> = func_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();

        // Check if function exists
        let exists = self.user_functions.contains_key(&func_lower) || self.functions.contains_key(&func_lower);
        if !exists {
            let err_msg = format!("Function {}() does not exist", func_name);
            let exc = self.create_exception(b"ReflectionException", &err_msg, line);
            self.current_exception = Some(exc);
            return false;
        }

        let mut obj = this.borrow_mut();
        obj.set_property(b"name".to_vec(), Value::String(PhpString::from_string(func_name.clone())));
        obj.set_property(b"__reflection_target".to_vec(), Value::String(PhpString::from_vec(func_lower)));
        true
    }

    /// ReflectionProperty constructor
    fn reflection_property_construct(&mut self, args: &[Value], line: u32) -> bool {
        let this = match args.first() {
            Some(Value::Object(o)) => o.clone(),
            _ => return true,
        };

        let class_arg = args.get(1).cloned().unwrap_or(Value::Null);
        let prop_arg = args.get(2).cloned().unwrap_or(Value::Null);

        let class_name = match &class_arg {
            Value::Object(obj) => {
                let ob = obj.borrow();
                String::from_utf8_lossy(&ob.class_name).to_string()
            }
            _ => class_arg.to_php_string().to_string_lossy(),
        };
        let prop_name = prop_arg.to_php_string().to_string_lossy();

        let class_lower: Vec<u8> = class_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();

        let is_user_class = self.classes.contains_key(&class_lower);
        let is_builtin = self.is_known_builtin_class(&class_lower);

        if !is_user_class && !is_builtin {
            let err_msg = format!("Class \"{}\" does not exist", class_name);
            let exc = self.create_exception(b"ReflectionException", &err_msg, line);
            self.current_exception = Some(exc);
            return false;
        }

        let canonical_class = if is_user_class {
            self.classes.get(&class_lower)
                .map(|c| String::from_utf8_lossy(&c.name).to_string())
                .unwrap_or(class_name.clone())
        } else {
            self.builtin_canonical_name(&class_lower)
        };

        // For user classes, check property exists; for built-in classes, accept any property name
        if is_user_class {
            let prop_exists = self.classes.get(&class_lower)
                .map(|c| c.properties.iter().any(|p| p.name == prop_name.as_bytes()))
                .unwrap_or(false);

            if !prop_exists {
                let err_msg = format!("Property {}::${} does not exist", canonical_class, prop_name);
                let exc = self.create_exception(b"ReflectionException", &err_msg, line);
                self.current_exception = Some(exc);
                return false;
            }
        }

        let mut obj = this.borrow_mut();
        obj.set_property(b"name".to_vec(), Value::String(PhpString::from_string(prop_name.clone())));
        obj.set_property(b"class".to_vec(), Value::String(PhpString::from_string(canonical_class.clone())));
        obj.set_property(b"__reflection_class".to_vec(), Value::String(PhpString::from_string(canonical_class)));
        obj.set_property(b"__reflection_prop".to_vec(), Value::String(PhpString::from_string(prop_name)));
        true
    }

    /// ReflectionParameter constructor
    fn reflection_parameter_construct(&mut self, args: &[Value], line: u32) -> bool {
        let this = match args.first() {
            Some(Value::Object(o)) => o.clone(),
            _ => return true,
        };
        let func_arg = args.get(1).cloned().unwrap_or(Value::Null);
        let param_arg = args.get(2).cloned().unwrap_or(Value::Null);

        let func_name = func_arg.to_php_string().to_string_lossy();
        let param_idx = param_arg.to_long() as usize;

        let func_lower: Vec<u8> = func_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();

        // Look up the function
        if let Some(op_array) = self.user_functions.get(&func_lower).cloned() {
            let param_name = if param_idx < op_array.cv_names.len() {
                String::from_utf8_lossy(&op_array.cv_names[param_idx]).to_string()
            } else {
                format!("param{}", param_idx)
            };

            let mut obj = this.borrow_mut();
            obj.set_property(b"name".to_vec(), Value::String(PhpString::from_string(param_name)));
            obj.set_property(b"__reflection_func".to_vec(), Value::String(PhpString::from_vec(func_lower)));
            obj.set_property(b"__reflection_param_idx".to_vec(), Value::Long(param_idx as i64));
        }
        true
    }

    /// ReflectionClass no-arg method dispatch
    fn reflection_class_method(
        &mut self,
        method: &[u8],
        obj: &Rc<RefCell<PhpObject>>,
    ) -> Option<Value> {
        let ob = obj.borrow();
        let target = ob.get_property(b"__reflection_target").to_php_string().to_string_lossy();
        let class_lower: Vec<u8> = target.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
        drop(ob);

        match method {
            b"getname" => {
                let ob = obj.borrow();
                Some(ob.get_property(b"name"))
            }
            b"getparentclass" => {
                if let Some(ce) = self.classes.get(&class_lower) {
                    if let Some(ref parent) = ce.parent {
                        // Create a ReflectionClass for the parent
                        let parent_name = String::from_utf8_lossy(parent).to_string();
                        Some(self.create_reflection_class(&parent_name))
                    } else {
                        Some(Value::False)
                    }
                } else {
                    // Check built-in parent chain
                    let parents = builtin_parent_chain(&class_lower);
                    if let Some(first_parent) = parents.first() {
                        let parent_name = self.builtin_canonical_name(first_parent);
                        Some(self.create_reflection_class(&parent_name))
                    } else {
                        Some(Value::False)
                    }
                }
            }
            b"isabstract" => {
                let is_abstract = self.classes.get(&class_lower)
                    .map(|c| c.is_abstract)
                    .unwrap_or(false);
                Some(if is_abstract { Value::True } else { Value::False })
            }
            b"isfinal" => {
                let is_final = self.classes.get(&class_lower)
                    .map(|c| c.is_final || c.is_enum) // Enums are implicitly final
                    .unwrap_or(false);
                Some(if is_final { Value::True } else { Value::False })
            }
            b"isinterface" => {
                let is_interface = self.classes.get(&class_lower)
                    .map(|c| c.is_interface)
                    .unwrap_or(false);
                Some(if is_interface { Value::True } else { Value::False })
            }
            b"istrait" => {
                let is_trait = self.classes.get(&class_lower)
                    .map(|c| c.is_trait)
                    .unwrap_or(false);
                Some(if is_trait { Value::True } else { Value::False })
            }
            b"isenum" => {
                let is_enum = self.classes.get(&class_lower)
                    .map(|c| c.is_enum)
                    .unwrap_or(false);
                Some(if is_enum { Value::True } else { Value::False })
            }
            b"isreadonly" => {
                let is_readonly = self.classes.get(&class_lower)
                    .map(|c| c.is_readonly)
                    .unwrap_or(false);
                Some(if is_readonly { Value::True } else { Value::False })
            }
            b"isinstantiable" => {
                if let Some(ce) = self.classes.get(&class_lower) {
                    let instantiable = !ce.is_abstract && !ce.is_interface && !ce.is_trait && !ce.is_enum;
                    Some(if instantiable { Value::True } else { Value::False })
                } else {
                    // Built-in classes are generally instantiable
                    Some(Value::True)
                }
            }
            b"iscloneable" => {
                Some(Value::True)
            }
            b"isinternal" => {
                // User-defined classes are not internal
                let is_internal = !self.classes.contains_key(&class_lower);
                Some(if is_internal { Value::True } else { Value::False })
            }
            b"isuserdefined" => {
                let is_user = self.classes.contains_key(&class_lower);
                Some(if is_user { Value::True } else { Value::False })
            }
            b"isanonymous" => {
                Some(Value::False)
            }
            b"isiterable" | b"isiterateable" => {
                // Check if class implements Iterator or IteratorAggregate
                let is_iterable = self.class_implements_interface(&class_lower, b"iterator")
                    || self.class_implements_interface(&class_lower, b"iteratoraggregate")
                    || self.builtin_implements_interface(&class_lower, b"iterator")
                    || self.builtin_implements_interface(&class_lower, b"iteratoraggregate");
                Some(if is_iterable { Value::True } else { Value::False })
            }
            b"getinterfacenames" => {
                let mut names = PhpArray::new();
                if let Some(ce) = self.classes.get(&class_lower) {
                    for iface in &ce.interfaces {
                        names.push(Value::String(PhpString::from_vec(iface.clone())));
                    }
                }
                Some(Value::Array(Rc::new(RefCell::new(names))))
            }
            b"getinterfaces" => {
                let mut result = PhpArray::new();
                if let Some(ce) = self.classes.get(&class_lower) {
                    for iface in ce.interfaces.clone() {
                        let iface_name = String::from_utf8_lossy(&iface).to_string();
                        let rc = self.create_reflection_class(&iface_name);
                        result.set(ArrayKey::String(PhpString::from_vec(iface.clone())), rc);
                    }
                }
                Some(Value::Array(Rc::new(RefCell::new(result))))
            }
            b"gettraitnames" => {
                let mut names = PhpArray::new();
                if let Some(ce) = self.classes.get(&class_lower) {
                    for t in &ce.traits {
                        names.push(Value::String(PhpString::from_vec(t.clone())));
                    }
                }
                Some(Value::Array(Rc::new(RefCell::new(names))))
            }
            b"gettraits" => {
                let mut result = PhpArray::new();
                if let Some(ce) = self.classes.get(&class_lower) {
                    for t in ce.traits.clone() {
                        let trait_name = String::from_utf8_lossy(&t).to_string();
                        let rc = self.create_reflection_class(&trait_name);
                        result.set(ArrayKey::String(PhpString::from_vec(t.clone())), rc);
                    }
                }
                Some(Value::Array(Rc::new(RefCell::new(result))))
            }
            b"getconstructor" => {
                if let Some(ce) = self.classes.get(&class_lower) {
                    if ce.get_method(b"__construct").is_some() {
                        Some(self.create_reflection_method(&target, "__construct"))
                    } else {
                        Some(Value::Null)
                    }
                } else {
                    Some(Value::Null)
                }
            }
            b"getmodifiers" => {
                let mut mods = 0i64;
                if let Some(ce) = self.classes.get(&class_lower) {
                    if ce.is_abstract { mods |= 0x40; } // IS_EXPLICIT_ABSTRACT
                    if ce.is_final { mods |= 0x20; } // IS_FINAL
                    if ce.is_readonly { mods |= 0x10000; } // IS_READONLY
                }
                Some(Value::Long(mods))
            }
            b"getdefaultproperties" => {
                let mut result = PhpArray::new();
                if let Some(ce) = self.classes.get(&class_lower) {
                    for prop in &ce.properties {
                        if !prop.is_static {
                            result.set(
                                ArrayKey::String(PhpString::from_vec(prop.name.clone())),
                                prop.default.clone(),
                            );
                        }
                    }
                    // Also add parent properties
                    let mut parent = ce.parent.clone();
                    while let Some(ref p) = parent {
                        let p_lower: Vec<u8> = p.iter().map(|b| b.to_ascii_lowercase()).collect();
                        if let Some(pce) = self.classes.get(&p_lower) {
                            for prop in &pce.properties {
                                if !prop.is_static {
                                    let key = ArrayKey::String(PhpString::from_vec(prop.name.clone()));
                                    // Don't override child properties
                                    if result.get(&key).is_none() {
                                        // Skip private properties from parent
                                        if prop.visibility != Visibility::Private {
                                            result.set(key, prop.default.clone());
                                        }
                                    }
                                }
                            }
                            parent = pce.parent.clone();
                        } else {
                            break;
                        }
                    }
                }
                Some(Value::Array(Rc::new(RefCell::new(result))))
            }
            b"getstaticproperties" => {
                let mut result = PhpArray::new();
                if let Some(ce) = self.classes.get(&class_lower) {
                    for (name, val) in &ce.static_properties {
                        result.set(
                            ArrayKey::String(PhpString::from_vec(name.clone())),
                            val.clone(),
                        );
                    }
                }
                Some(Value::Array(Rc::new(RefCell::new(result))))
            }
            b"getfilename" => {
                // User-defined classes - return false for built-in
                if self.classes.contains_key(&class_lower) {
                    Some(Value::String(PhpString::from_string(self.current_file.clone())))
                } else {
                    Some(Value::False)
                }
            }
            b"getstartline" => {
                Some(Value::False)
            }
            b"getendline" => {
                Some(Value::False)
            }
            b"getdoccomment" => {
                Some(Value::False)
            }
            b"newinstancewithoutconstructor" => {
                // Create an instance without calling the constructor
                let obj_id = self.next_object_id;
                self.next_object_id += 1;
                let canonical = self.classes.get(&class_lower)
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| self.builtin_canonical_name(&class_lower).into_bytes());
                let mut new_obj = PhpObject::new(canonical, obj_id);
                // Set default property values
                if let Some(ce) = self.classes.get(&class_lower) {
                    for prop in &ce.properties {
                        if !prop.is_static {
                            new_obj.set_property(prop.name.clone(), prop.default.clone());
                        }
                    }
                }
                Some(Value::Object(Rc::new(RefCell::new(new_obj))))
            }
            b"getextension" => {
                Some(Value::Null)
            }
            b"getextensionname" => {
                Some(Value::False)
            }
            b"getattributes" => {
                Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
            }
            b"innamespace" => {
                Some(if target.contains('\\') { Value::True } else { Value::False })
            }
            b"getnamespacename" => {
                if let Some(pos) = target.rfind('\\') {
                    Some(Value::String(PhpString::from_string(target[..pos].to_string())))
                } else {
                    Some(Value::String(PhpString::empty()))
                }
            }
            b"getshortname" => {
                if let Some(pos) = target.rfind('\\') {
                    Some(Value::String(PhpString::from_string(target[pos + 1..].to_string())))
                } else {
                    Some(Value::String(PhpString::from_string(target)))
                }
            }
            b"__tostring" => {
                // Build a __toString representation for ReflectionClass
                let ob = obj.borrow();
                let name = ob.get_property(b"name").to_php_string().to_string_lossy();
                drop(ob);
                let mut s = String::new();
                // Determine class type
                let is_interface = self.classes.get(&class_lower).map(|c| c.is_interface).unwrap_or(false);
                let is_trait = self.classes.get(&class_lower).map(|c| c.is_trait).unwrap_or(false);
                let is_abstract = self.classes.get(&class_lower).map(|c| c.is_abstract).unwrap_or(false);
                let is_final = self.classes.get(&class_lower).map(|c| c.is_final).unwrap_or(false);

                if is_interface {
                    s.push_str(&format!("Interface [ <user> interface {} ", name));
                } else if is_trait {
                    s.push_str(&format!("Trait [ <user> trait {} ", name));
                } else {
                    let kind = if self.classes.contains_key(&class_lower) { "user" } else { "internal" };
                    let modifiers = if is_abstract { "abstract " } else if is_final { "final " } else { "" };
                    s.push_str(&format!("Class [ <{}> {}class {} ", kind, modifiers, name));
                }

                // Parent
                if let Some(ce) = self.classes.get(&class_lower) {
                    if let Some(ref parent) = ce.parent {
                        s.push_str(&format!("extends {} ", String::from_utf8_lossy(parent)));
                    }
                    if !ce.interfaces.is_empty() {
                        s.push_str("implements ");
                        let ifaces: Vec<String> = ce.interfaces.iter().map(|i| String::from_utf8_lossy(i).to_string()).collect();
                        s.push_str(&ifaces.join(", "));
                        s.push(' ');
                    }
                }
                s.push_str("] {\n}\n");
                Some(Value::String(PhpString::from_string(s)))
            }
            _ => None,
        }
    }

    /// Create a ReflectionClass object for a given class name
    fn create_reflection_class(&mut self, class_name: &str) -> Value {
        let obj_id = self.next_object_id;
        self.next_object_id += 1;
        let mut obj = PhpObject::new(b"ReflectionClass".to_vec(), obj_id);
        obj.set_property(b"name".to_vec(), Value::String(PhpString::from_string(class_name.to_string())));
        obj.set_property(b"__reflection_target".to_vec(), Value::String(PhpString::from_string(class_name.to_string())));
        Value::Object(Rc::new(RefCell::new(obj)))
    }

    /// Create a ReflectionMethod object for a given class and method name
    fn create_reflection_method(&mut self, class_name: &str, method_name: &str) -> Value {
        let class_lower: Vec<u8> = class_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
        let method_lower: Vec<u8> = method_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();

        let obj_id = self.next_object_id;
        self.next_object_id += 1;
        let mut obj = PhpObject::new(b"ReflectionMethod".to_vec(), obj_id);

        let canonical_method = self.classes.get(&class_lower)
            .and_then(|c| c.get_method(&method_lower).map(|m| String::from_utf8_lossy(&m.name).to_string()))
            .unwrap_or_else(|| method_name.to_string());

        let canonical_class = self.classes.get(&class_lower)
            .map(|c| String::from_utf8_lossy(&c.name).to_string())
            .unwrap_or_else(|| class_name.to_string());

        obj.set_property(b"name".to_vec(), Value::String(PhpString::from_string(canonical_method)));
        obj.set_property(b"class".to_vec(), Value::String(PhpString::from_string(canonical_class.clone())));
        obj.set_property(b"__reflection_class".to_vec(), Value::String(PhpString::from_string(canonical_class)));
        obj.set_property(b"__reflection_method".to_vec(), Value::String(PhpString::from_vec(method_lower)));
        Value::Object(Rc::new(RefCell::new(obj)))
    }

    /// ReflectionClass methods that need args (dispatched via handle_spl_docall)
    fn reflection_class_docall(
        &mut self,
        method: &[u8],
        args: &[Value],
    ) -> Option<Value> {
        let this = args.first()?;
        if let Value::Object(obj) = this {
            let ob = obj.borrow();
            let target = ob.get_property(b"__reflection_target").to_php_string().to_string_lossy();
            let class_lower: Vec<u8> = target.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
            drop(ob);

            match method {
                b"hasmethod" => {
                    let method_name = args.get(1)?.to_php_string().to_string_lossy();
                    let method_lower: Vec<u8> = method_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
                    let has = self.classes.get(&class_lower)
                        .map(|c| c.get_method(&method_lower).is_some())
                        .unwrap_or(false);
                    Some(if has { Value::True } else { Value::False })
                }
                b"hasproperty" => {
                    let prop_name = args.get(1)?.to_php_string();
                    let has = self.classes.get(&class_lower)
                        .map(|c| c.properties.iter().any(|p| p.name == prop_name.as_bytes()))
                        .unwrap_or(false);
                    Some(if has { Value::True } else { Value::False })
                }
                b"hasconstant" => {
                    let const_name = args.get(1)?.to_php_string();
                    let has = self.reflection_class_has_constant(&class_lower, const_name.as_bytes());
                    Some(if has { Value::True } else { Value::False })
                }
                b"getconstant" => {
                    let const_name = args.get(1)?.to_php_string();
                    let val = self.reflection_class_get_constant(&class_lower, const_name.as_bytes());
                    if let Some(v) = val {
                        Some(v)
                    } else {
                        // Emit deprecated warning
                        self.emit_deprecated_at(
                            "ReflectionClass::getConstant() for a non-existent constant is deprecated, use ReflectionClass::hasConstant() to check if the constant exists",
                            self.current_line,
                        );
                        Some(Value::False)
                    }
                }
                b"getconstants" => {
                    let mut result = PhpArray::new();
                    if let Some(ce) = self.classes.get(&class_lower) {
                        for (name, val) in &ce.constants {
                            result.set(ArrayKey::String(PhpString::from_vec(name.clone())), val.clone());
                        }
                    }
                    // Also check parent constants
                    self.reflection_collect_parent_constants(&class_lower, &mut result);
                    Some(Value::Array(Rc::new(RefCell::new(result))))
                }
                b"getmethod" => {
                    let method_name = args.get(1)?.to_php_string().to_string_lossy();
                    let method_lower: Vec<u8> = method_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
                    let has = self.classes.get(&class_lower)
                        .map(|c| c.get_method(&method_lower).is_some())
                        .unwrap_or(false);
                    if has {
                        let canonical_class = self.classes.get(&class_lower)
                            .map(|c| String::from_utf8_lossy(&c.name).to_string())
                            .unwrap_or(target.clone());
                        Some(self.create_reflection_method(&canonical_class, &method_name))
                    } else {
                        let canonical_class = self.classes.get(&class_lower)
                            .map(|c| String::from_utf8_lossy(&c.name).to_string())
                            .unwrap_or(target.clone());
                        let err_msg = format!("Method {}::{}() does not exist", canonical_class, method_name);
                        let exc = self.create_exception(b"ReflectionException", &err_msg, self.current_line);
                        self.current_exception = Some(exc);
                        Some(Value::Null)
                    }
                }
                b"getmethods" => {
                    let filter = args.get(1).map(|v| v.to_long()).unwrap_or(-1);
                    let mut result = PhpArray::new();
                    // Collect method info first to avoid borrow issues
                    let method_info: Vec<(String, String)> = if let Some(ce) = self.classes.get(&class_lower) {
                        ce.methods.values().filter_map(|method_def| {
                            if filter != -1 {
                                let method_mod = Self::reflection_method_modifiers_static(method_def);
                                if method_mod & filter == 0 {
                                    return None;
                                }
                            }
                            let method_name = String::from_utf8_lossy(&method_def.name).to_string();
                            let declaring = String::from_utf8_lossy(&method_def.declaring_class).to_string();
                            Some((declaring, method_name))
                        }).collect()
                    } else {
                        vec![]
                    };
                    for (declaring, method_name) in method_info {
                        let declaring_lower: Vec<u8> = declaring.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
                        let declaring_canonical = self.classes.get(&declaring_lower)
                            .map(|c| String::from_utf8_lossy(&c.name).to_string())
                            .unwrap_or(declaring);
                        result.push(self.create_reflection_method(
                            &declaring_canonical,
                            &method_name,
                        ));
                    }
                    Some(Value::Array(Rc::new(RefCell::new(result))))
                }
                b"getproperty" => {
                    let prop_name = args.get(1)?.to_php_string().to_string_lossy();
                    let has = self.classes.get(&class_lower)
                        .map(|c| c.properties.iter().any(|p| p.name == prop_name.as_bytes()))
                        .unwrap_or(false);
                    if has {
                        Some(self.create_reflection_property(&target, &prop_name))
                    } else {
                        let err_msg = format!("Property {}::${} does not exist", target, prop_name);
                        let exc = self.create_exception(b"ReflectionException", &err_msg, self.current_line);
                        self.current_exception = Some(exc);
                        Some(Value::Null)
                    }
                }
                b"getproperties" => {
                    let filter = args.get(1).map(|v| v.to_long()).unwrap_or(-1);
                    let mut result = PhpArray::new();
                    let prop_names: Vec<String> = if let Some(ce) = self.classes.get(&class_lower) {
                        ce.properties.iter().filter_map(|prop| {
                            if filter != -1 {
                                let prop_mod = Self::reflection_property_modifiers_static(prop);
                                if prop_mod & filter == 0 {
                                    return None;
                                }
                            }
                            if !prop.is_static {
                                Some(String::from_utf8_lossy(&prop.name).to_string())
                            } else {
                                None
                            }
                        }).collect()
                    } else {
                        vec![]
                    };
                    for prop_name in prop_names {
                        result.push(self.create_reflection_property(&target, &prop_name));
                    }
                    Some(Value::Array(Rc::new(RefCell::new(result))))
                }
                b"issubclassof" => {
                    let parent_arg = args.get(1)?;
                    let parent_name = match parent_arg {
                        Value::Object(o) => {
                            let ob = o.borrow();
                            // If it's a ReflectionClass, use its name
                            if ob.class_name.eq_ignore_ascii_case(b"ReflectionClass") {
                                ob.get_property(b"name").to_php_string().to_string_lossy()
                            } else {
                                String::from_utf8_lossy(&ob.class_name).to_string()
                            }
                        }
                        _ => parent_arg.to_php_string().to_string_lossy(),
                    };
                    let parent_lower: Vec<u8> = parent_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
                    // A class is not a subclass of itself
                    if class_lower == parent_lower {
                        return Some(Value::False);
                    }
                    let result = self.class_extends(&class_lower, &parent_lower)
                        || self.class_implements_interface(&class_lower, &parent_lower)
                        || is_builtin_subclass(&class_lower, &parent_lower);
                    Some(if result { Value::True } else { Value::False })
                }
                b"implementsinterface" => {
                    let iface_arg = args.get(1)?;
                    let iface_name = match iface_arg {
                        Value::Object(o) => {
                            let ob = o.borrow();
                            if ob.class_name.eq_ignore_ascii_case(b"ReflectionClass") {
                                ob.get_property(b"name").to_php_string().to_string_lossy()
                            } else {
                                String::from_utf8_lossy(&ob.class_name).to_string()
                            }
                        }
                        _ => iface_arg.to_php_string().to_string_lossy(),
                    };
                    let iface_lower: Vec<u8> = iface_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
                    let result = self.class_implements_interface(&class_lower, &iface_lower)
                        || self.builtin_implements_interface(&class_lower, &iface_lower);
                    Some(if result { Value::True } else { Value::False })
                }
                b"isinstance" => {
                    let instance = args.get(1)?;
                    if let Value::Object(inst_obj) = instance {
                        let inst_class: Vec<u8> = inst_obj.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                        let result = inst_class == class_lower
                            || self.class_extends(&inst_class, &class_lower)
                            || self.class_implements_interface(&inst_class, &class_lower)
                            || is_builtin_subclass(&inst_class, &class_lower);
                        Some(if result { Value::True } else { Value::False })
                    } else {
                        Some(Value::False)
                    }
                }
                b"newinstance" => {
                    // Create instance and call constructor with remaining args
                    let obj_id = self.next_object_id;
                    self.next_object_id += 1;
                    let canonical = self.classes.get(&class_lower)
                        .map(|c| c.name.clone())
                        .unwrap_or_else(|| self.builtin_canonical_name(&class_lower).into_bytes());

                    let has_constructor = self.classes.get(&class_lower)
                        .map(|c| c.get_method(b"__construct").is_some())
                        .unwrap_or(false);

                    // If no constructor and args were passed, throw
                    if !has_constructor && args.len() > 1 {
                        let canonical_name = String::from_utf8_lossy(&canonical).to_string();
                        let err_msg = format!("Class {} does not have a constructor, so you cannot pass any constructor arguments", canonical_name);
                        let exc = self.create_exception(b"ReflectionException", &err_msg, self.current_line);
                        self.current_exception = Some(exc);
                        return Some(Value::Null);
                    }

                    let mut new_obj = PhpObject::new(canonical, obj_id);
                    // Set default property values
                    if let Some(ce) = self.classes.get(&class_lower) {
                        for prop in &ce.properties {
                            if !prop.is_static {
                                new_obj.set_property(prop.name.clone(), prop.default.clone());
                            }
                        }
                    }
                    let new_val = Value::Object(Rc::new(RefCell::new(new_obj)));

                    // Call constructor if it exists
                    if has_constructor {
                        let ctor = self.classes.get(&class_lower)
                            .and_then(|c| c.get_method(b"__construct"))
                            .cloned();
                        if let Some(ctor_method) = ctor {
                            let mut ctor_args = vec![new_val.clone()];
                            for arg in args.iter().skip(1) {
                                ctor_args.push(arg.clone());
                            }
                            let ctor_key = {
                                let mut key = class_lower.clone();
                                key.extend_from_slice(b"::__construct");
                                key
                            };
                            self.user_functions.insert(ctor_key.clone(), ctor_method.op_array.clone());
                            let mut cvs = vec![Value::Undef; ctor_method.op_array.cv_names.len()];
                            for (i, arg) in ctor_args.iter().enumerate() {
                                if i < cvs.len() {
                                    cvs[i] = arg.clone();
                                }
                            }
                            let _ = self.execute_op_array(&ctor_method.op_array, cvs);
                        }
                    }

                    Some(new_val)
                }
                b"newinstanceargs" => {
                    // Same as newInstance but takes an array of args
                    let args_arr = args.get(1).cloned().unwrap_or(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
                    let ctor_args: Vec<Value> = if let Value::Array(arr) = &args_arr {
                        arr.borrow().iter().map(|(_, v)| v.clone()).collect()
                    } else {
                        vec![]
                    };

                    let obj_id = self.next_object_id;
                    self.next_object_id += 1;
                    let canonical = self.classes.get(&class_lower)
                        .map(|c| c.name.clone())
                        .unwrap_or_else(|| self.builtin_canonical_name(&class_lower).into_bytes());

                    let has_constructor = self.classes.get(&class_lower)
                        .map(|c| c.get_method(b"__construct").is_some())
                        .unwrap_or(false);

                    let mut new_obj = PhpObject::new(canonical, obj_id);
                    if let Some(ce) = self.classes.get(&class_lower) {
                        for prop in &ce.properties {
                            if !prop.is_static {
                                new_obj.set_property(prop.name.clone(), prop.default.clone());
                            }
                        }
                    }
                    let new_val = Value::Object(Rc::new(RefCell::new(new_obj)));

                    if has_constructor {
                        let ctor = self.classes.get(&class_lower)
                            .and_then(|c| c.get_method(b"__construct"))
                            .cloned();
                        if let Some(ctor_method) = ctor {
                            let mut all_args = vec![new_val.clone()];
                            all_args.extend(ctor_args);
                            let mut cvs = vec![Value::Undef; ctor_method.op_array.cv_names.len()];
                            for (i, arg) in all_args.iter().enumerate() {
                                if i < cvs.len() {
                                    cvs[i] = arg.clone();
                                }
                            }
                            let _ = self.execute_op_array(&ctor_method.op_array, cvs);
                        }
                    }

                    Some(new_val)
                }
                b"getstaticpropertyvalue" => {
                    let prop_name = args.get(1)?.to_php_string();
                    let default = args.get(2);
                    if let Some(ce) = self.classes.get(&class_lower) {
                        if let Some(val) = ce.static_properties.get(prop_name.as_bytes()) {
                            Some(val.clone())
                        } else if let Some(d) = default {
                            Some(d.clone())
                        } else {
                            Some(Value::Null)
                        }
                    } else if let Some(d) = default {
                        Some(d.clone())
                    } else {
                        Some(Value::Null)
                    }
                }
                b"setstaticpropertyvalue" => {
                    let prop_name = args.get(1)?.to_php_string();
                    let value = args.get(2).cloned().unwrap_or(Value::Null);
                    if let Some(ce) = self.classes.get_mut(&class_lower) {
                        ce.static_properties.insert(prop_name.as_bytes().to_vec(), value);
                    }
                    Some(Value::Null)
                }
                b"getreflectionconstant" | b"getreflectionconstants" => {
                    // Return ReflectionClassConstant objects
                    let mut result = PhpArray::new();
                    if method == b"getreflectionconstant" {
                        let const_name = args.get(1)?.to_php_string();
                        let val = self.reflection_class_get_constant(&class_lower, const_name.as_bytes());
                        if let Some(v) = val {
                            return Some(self.create_reflection_class_constant(&target, &const_name.to_string_lossy(), v));
                        } else {
                            return Some(Value::False);
                        }
                    }
                    // getReflectionConstants
                    if let Some(ce) = self.classes.get(&class_lower) {
                        for (name, val) in ce.constants.clone() {
                            let const_name = String::from_utf8_lossy(&name).to_string();
                            result.push(self.create_reflection_class_constant(&target, &const_name, val));
                        }
                    }
                    Some(Value::Array(Rc::new(RefCell::new(result))))
                }
                _ => None,
            }
        } else {
            None
        }
    }

    /// Helper to check if a class has a constant (walks parent chain)
    fn reflection_class_has_constant(&self, class_lower: &[u8], const_name: &[u8]) -> bool {
        if let Some(ce) = self.classes.get(class_lower) {
            if ce.constants.contains_key(const_name) {
                return true;
            }
            // Check parent chain
            if let Some(ref parent) = ce.parent {
                let parent_lower: Vec<u8> = parent.iter().map(|b| b.to_ascii_lowercase()).collect();
                return self.reflection_class_has_constant(&parent_lower, const_name);
            }
        }
        false
    }

    /// Helper to get a constant from a class (walks parent chain)
    fn reflection_class_get_constant(&self, class_lower: &[u8], const_name: &[u8]) -> Option<Value> {
        if let Some(ce) = self.classes.get(class_lower) {
            if let Some(val) = ce.constants.get(const_name) {
                return Some(val.clone());
            }
            // Check parent chain
            if let Some(ref parent) = ce.parent {
                let parent_lower: Vec<u8> = parent.iter().map(|b| b.to_ascii_lowercase()).collect();
                return self.reflection_class_get_constant(&parent_lower, const_name);
            }
        }
        None
    }

    /// Collect parent constants into the result array
    fn reflection_collect_parent_constants(&self, class_lower: &[u8], result: &mut PhpArray) {
        if let Some(ce) = self.classes.get(class_lower) {
            if let Some(ref parent) = ce.parent {
                let parent_lower: Vec<u8> = parent.iter().map(|b| b.to_ascii_lowercase()).collect();
                if let Some(pce) = self.classes.get(&parent_lower) {
                    for (name, val) in &pce.constants {
                        let key = ArrayKey::String(PhpString::from_vec(name.clone()));
                        if result.get(&key).is_none() {
                            result.set(key, val.clone());
                        }
                    }
                }
                self.reflection_collect_parent_constants(&parent_lower, result);
            }
        }
    }

    /// Collect parent methods
    fn reflection_collect_parent_methods(&self, _class_lower: &[u8], _class_name: &str, _filter: i64, _result: &mut PhpArray) {
        // Methods are already inherited in the class table for user classes
    }

    /// Get modifier flags for a method
    fn reflection_method_modifiers(&self, method: &MethodDef) -> i64 {
        Self::reflection_method_modifiers_static(method)
    }

    /// Get modifier flags for a method (static version for use in closures)
    fn reflection_method_modifiers_static(method: &MethodDef) -> i64 {
        let mut mods = 0i64;
        match method.visibility {
            Visibility::Public => mods |= 1,    // IS_PUBLIC
            Visibility::Protected => mods |= 2, // IS_PROTECTED
            Visibility::Private => mods |= 4,   // IS_PRIVATE
        }
        if method.is_static { mods |= 0x10; }   // IS_STATIC
        if method.is_abstract { mods |= 0x40; } // IS_ABSTRACT
        if method.is_final { mods |= 0x20; }    // IS_FINAL
        mods
    }

    /// Get modifier flags for a property
    fn reflection_property_modifiers(&self, prop: &PropertyDef) -> i64 {
        Self::reflection_property_modifiers_static(prop)
    }

    /// Get modifier flags for a property (static version for use in closures)
    fn reflection_property_modifiers_static(prop: &PropertyDef) -> i64 {
        let mut mods = 0i64;
        match prop.visibility {
            Visibility::Public => mods |= 1,
            Visibility::Protected => mods |= 2,
            Visibility::Private => mods |= 4,
        }
        if prop.is_static { mods |= 0x10; }
        if prop.is_readonly { mods |= 0x10000; }
        mods
    }

    /// Create a ReflectionProperty object
    fn create_reflection_property(&mut self, class_name: &str, prop_name: &str) -> Value {
        let obj_id = self.next_object_id;
        self.next_object_id += 1;
        let mut obj = PhpObject::new(b"ReflectionProperty".to_vec(), obj_id);
        obj.set_property(b"name".to_vec(), Value::String(PhpString::from_string(prop_name.to_string())));
        obj.set_property(b"class".to_vec(), Value::String(PhpString::from_string(class_name.to_string())));
        obj.set_property(b"__reflection_class".to_vec(), Value::String(PhpString::from_string(class_name.to_string())));
        obj.set_property(b"__reflection_prop".to_vec(), Value::String(PhpString::from_string(prop_name.to_string())));
        Value::Object(Rc::new(RefCell::new(obj)))
    }

    /// Create a ReflectionClassConstant object
    fn create_reflection_class_constant(&mut self, class_name: &str, const_name: &str, value: Value) -> Value {
        let obj_id = self.next_object_id;
        self.next_object_id += 1;
        let mut obj = PhpObject::new(b"ReflectionClassConstant".to_vec(), obj_id);
        obj.set_property(b"name".to_vec(), Value::String(PhpString::from_string(const_name.to_string())));
        obj.set_property(b"class".to_vec(), Value::String(PhpString::from_string(class_name.to_string())));
        obj.set_property(b"__reflection_value".to_vec(), value);
        Value::Object(Rc::new(RefCell::new(obj)))
    }

    /// ReflectionMethod no-arg method dispatch
    fn reflection_method_method(
        &mut self,
        method: &[u8],
        obj: &Rc<RefCell<PhpObject>>,
    ) -> Option<Value> {
        let ob = obj.borrow();
        let class_name = ob.get_property(b"__reflection_class").to_php_string().to_string_lossy();
        let method_lower_val = ob.get_property(b"__reflection_method");
        let method_lower = method_lower_val.to_php_string();
        let class_lower: Vec<u8> = class_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
        drop(ob);

        match method {
            b"getname" => {
                let ob = obj.borrow();
                Some(ob.get_property(b"name"))
            }
            b"getdeclaringclass" => {
                let class = self.classes.get(&class_lower)
                    .and_then(|c| c.get_method(method_lower.as_bytes())
                        .map(|m| String::from_utf8_lossy(&m.declaring_class).to_string()))
                    .unwrap_or(class_name.clone());
                let declaring_canonical = self.classes.get(class.as_bytes())
                    .map(|c| String::from_utf8_lossy(&c.name).to_string())
                    .unwrap_or(class.clone());
                Some(self.create_reflection_class(&declaring_canonical))
            }
            b"ispublic" => {
                let vis = self.classes.get(&class_lower)
                    .and_then(|c| c.get_method(method_lower.as_bytes()).map(|m| m.visibility))
                    .unwrap_or(Visibility::Public);
                Some(if vis == Visibility::Public { Value::True } else { Value::False })
            }
            b"isprotected" => {
                let vis = self.classes.get(&class_lower)
                    .and_then(|c| c.get_method(method_lower.as_bytes()).map(|m| m.visibility))
                    .unwrap_or(Visibility::Public);
                Some(if vis == Visibility::Protected { Value::True } else { Value::False })
            }
            b"isprivate" => {
                let vis = self.classes.get(&class_lower)
                    .and_then(|c| c.get_method(method_lower.as_bytes()).map(|m| m.visibility))
                    .unwrap_or(Visibility::Public);
                Some(if vis == Visibility::Private { Value::True } else { Value::False })
            }
            b"isstatic" => {
                let is_static = self.classes.get(&class_lower)
                    .and_then(|c| c.get_method(method_lower.as_bytes()).map(|m| m.is_static))
                    .unwrap_or(false);
                Some(if is_static { Value::True } else { Value::False })
            }
            b"isabstract" => {
                let is_abstract = self.classes.get(&class_lower)
                    .and_then(|c| c.get_method(method_lower.as_bytes()).map(|m| m.is_abstract))
                    .unwrap_or(false);
                Some(if is_abstract { Value::True } else { Value::False })
            }
            b"isfinal" => {
                let is_final = self.classes.get(&class_lower)
                    .and_then(|c| c.get_method(method_lower.as_bytes()).map(|m| m.is_final))
                    .unwrap_or(false);
                Some(if is_final { Value::True } else { Value::False })
            }
            b"isconstructor" => {
                Some(if method_lower.as_bytes() == b"__construct" { Value::True } else { Value::False })
            }
            b"isdestructor" => {
                Some(if method_lower.as_bytes() == b"__destruct" { Value::True } else { Value::False })
            }
            b"getmodifiers" => {
                let mods = self.classes.get(&class_lower)
                    .and_then(|c| c.get_method(method_lower.as_bytes()).map(|m| self.reflection_method_modifiers(m)))
                    .unwrap_or(1); // default public
                Some(Value::Long(mods))
            }
            b"getnumberofparameters" => {
                let count = self.classes.get(&class_lower)
                    .and_then(|c| c.get_method(method_lower.as_bytes()).map(|m| m.op_array.param_count))
                    .unwrap_or(0);
                Some(Value::Long(count as i64))
            }
            b"getnumberofrequiredparameters" => {
                let count = self.classes.get(&class_lower)
                    .and_then(|c| c.get_method(method_lower.as_bytes()).map(|m| m.op_array.required_param_count))
                    .unwrap_or(0);
                Some(Value::Long(count as i64))
            }
            b"getparameters" => {
                let op_array = self.classes.get(&class_lower)
                    .and_then(|c| c.get_method(method_lower.as_bytes()))
                    .map(|m| m.op_array.clone());
                let params = if let Some(oa) = op_array {
                    self.create_reflection_parameters(&oa)
                } else {
                    Value::Array(Rc::new(RefCell::new(PhpArray::new())))
                };
                Some(params)
            }
            b"getreturntype" => {
                let ret = self.classes.get(&class_lower)
                    .and_then(|c| c.get_method(method_lower.as_bytes()))
                    .and_then(|m| m.op_array.return_type.as_ref())
                    .cloned();
                if let Some(rt) = ret {
                    Some(self.create_reflection_type(&rt))
                } else {
                    Some(Value::Null)
                }
            }
            b"hasreturntype" => {
                let has = self.classes.get(&class_lower)
                    .and_then(|c| c.get_method(method_lower.as_bytes()))
                    .and_then(|m| m.op_array.return_type.as_ref())
                    .is_some();
                Some(if has { Value::True } else { Value::False })
            }
            b"isuserdefined" => {
                Some(Value::True)
            }
            b"isinternal" => {
                Some(Value::False)
            }
            b"returnsreference" => {
                Some(Value::False)
            }
            b"getfilename" => {
                Some(Value::String(PhpString::from_string(self.current_file.clone())))
            }
            b"getstartline" => {
                let line = self.classes.get(&class_lower)
                    .and_then(|c| c.get_method(method_lower.as_bytes()).map(|m| m.op_array.decl_line))
                    .unwrap_or(0);
                Some(Value::Long(line as i64))
            }
            b"getendline" => {
                Some(Value::False)
            }
            b"getdoccomment" => {
                Some(Value::False)
            }
            b"isvariadic" => {
                let is_variadic = self.classes.get(&class_lower)
                    .and_then(|c| c.get_method(method_lower.as_bytes()))
                    .and_then(|m| m.op_array.variadic_param)
                    .is_some();
                Some(if is_variadic { Value::True } else { Value::False })
            }
            b"isdeprecated" => {
                Some(Value::False)
            }
            b"getclosure" => {
                // getClosure() with no args returns a closure for the method
                Some(Value::Null)
            }
            b"getstaticvariables" => {
                Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
            }
            b"setaccessible" => {
                Some(Value::Null)
            }
            b"getattributes" => {
                Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
            }
            b"isinternal" => {
                Some(Value::False)
            }
            b"__tostring" => {
                let ob = obj.borrow();
                let name = ob.get_property(b"name").to_php_string().to_string_lossy();
                let class = ob.get_property(b"class").to_php_string().to_string_lossy();
                drop(ob);

                let method_def = self.classes.get(&class_lower)
                    .and_then(|c| c.get_method(method_lower.as_bytes()))
                    .cloned();

                if let Some(m) = method_def {
                    let vis = match m.visibility {
                        Visibility::Public => "public",
                        Visibility::Protected => "protected",
                        Visibility::Private => "private",
                    };
                    let modifiers = format!("{}{}{}",
                        if m.is_abstract { "abstract " } else { "" },
                        if m.is_final { "final " } else { "" },
                        vis,
                    );
                    let mut s = format!("Method [ <user> {} method {} ] {{\n", modifiers, name);
                    s.push_str(&format!("  @@ {} {} - {}\n", self.current_file, m.op_array.decl_line, m.op_array.decl_line));
                    s.push_str(&format!("\n  - Parameters [{}] {{\n", m.op_array.param_count));
                    for i in 0..m.op_array.param_count as usize {
                        if i < m.op_array.cv_names.len() {
                            let pname = String::from_utf8_lossy(&m.op_array.cv_names[i]);
                            let required = i < m.op_array.required_param_count as usize;
                            s.push_str(&format!("    Parameter #{} [ <{}> ${} ]\n", i,
                                if required { "required" } else { "optional" }, pname));
                        }
                    }
                    s.push_str("  }\n}\n");
                    Some(Value::String(PhpString::from_string(s)))
                } else {
                    Some(Value::String(PhpString::from_string(format!("Method [ {} {} ]", class, name))))
                }
            }
            _ => None,
        }
    }

    /// ReflectionMethod methods that need args
    fn reflection_method_docall(
        &mut self,
        method: &[u8],
        args: &[Value],
    ) -> Option<Value> {
        let this = args.first()?;
        if let Value::Object(obj) = this {
            let ob = obj.borrow();
            let class_name = ob.get_property(b"__reflection_class").to_php_string().to_string_lossy();
            let method_lower_val = ob.get_property(b"__reflection_method");
            let method_lower_bytes = method_lower_val.to_php_string();
            let class_lower: Vec<u8> = class_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
            drop(ob);

            match method {
                b"invoke" => {
                    // invoke($object, ...$args)
                    let target_obj = args.get(1)?.clone();
                    let invoke_args: Vec<Value> = args.iter().skip(2).cloned().collect();

                    let method_def = self.classes.get(&class_lower)
                        .and_then(|c| c.get_method(method_lower_bytes.as_bytes()))
                        .cloned();

                    if let Some(m) = method_def {
                        let mut func_key = class_lower.clone();
                        func_key.extend_from_slice(b"::");
                        func_key.extend_from_slice(method_lower_bytes.as_bytes());
                        self.user_functions.insert(func_key.clone(), m.op_array.clone());

                        let mut cvs = vec![Value::Undef; m.op_array.cv_names.len()];
                        if !m.is_static {
                            if !cvs.is_empty() {
                                cvs[0] = target_obj;
                            }
                            for (i, arg) in invoke_args.iter().enumerate() {
                                if i + 1 < cvs.len() {
                                    cvs[i + 1] = arg.clone();
                                }
                            }
                        } else {
                            for (i, arg) in invoke_args.iter().enumerate() {
                                if i < cvs.len() {
                                    cvs[i] = arg.clone();
                                }
                            }
                        }

                        // Push class scope
                        self.called_class_stack.push(class_lower.clone());
                        self.class_scope_stack.push(class_lower.clone());

                        let result = self.execute_op_array(&m.op_array, cvs).unwrap_or(Value::Null);

                        self.called_class_stack.pop();
                        self.class_scope_stack.pop();

                        Some(result)
                    } else {
                        Some(Value::Null)
                    }
                }
                b"invokeargs" => {
                    let target_obj = args.get(1)?.clone();
                    let args_arr = args.get(2).cloned().unwrap_or(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
                    let invoke_args: Vec<Value> = if let Value::Array(arr) = &args_arr {
                        arr.borrow().iter().map(|(_, v)| v.clone()).collect()
                    } else {
                        vec![]
                    };

                    let method_def = self.classes.get(&class_lower)
                        .and_then(|c| c.get_method(method_lower_bytes.as_bytes()))
                        .cloned();

                    if let Some(m) = method_def {
                        let mut cvs = vec![Value::Undef; m.op_array.cv_names.len()];
                        if !m.is_static {
                            if !cvs.is_empty() {
                                cvs[0] = target_obj;
                            }
                            for (i, arg) in invoke_args.iter().enumerate() {
                                if i + 1 < cvs.len() {
                                    cvs[i + 1] = arg.clone();
                                }
                            }
                        } else {
                            for (i, arg) in invoke_args.iter().enumerate() {
                                if i < cvs.len() {
                                    cvs[i] = arg.clone();
                                }
                            }
                        }

                        self.called_class_stack.push(class_lower.clone());
                        self.class_scope_stack.push(class_lower.clone());

                        let result = self.execute_op_array(&m.op_array, cvs).unwrap_or(Value::Null);

                        self.called_class_stack.pop();
                        self.class_scope_stack.pop();

                        Some(result)
                    } else {
                        Some(Value::Null)
                    }
                }
                _ => None,
            }
        } else {
            None
        }
    }

    /// ReflectionFunction no-arg method dispatch
    fn reflection_function_method(
        &mut self,
        method: &[u8],
        obj: &Rc<RefCell<PhpObject>>,
    ) -> Option<Value> {
        let ob = obj.borrow();
        let target = ob.get_property(b"__reflection_target").to_php_string();
        let func_lower = target.as_bytes().to_vec();
        drop(ob);

        match method {
            b"getname" => {
                let ob = obj.borrow();
                Some(ob.get_property(b"name"))
            }
            b"isinternal" => {
                let is_internal = self.functions.contains_key(func_lower.as_slice());
                Some(if is_internal { Value::True } else { Value::False })
            }
            b"isuserdefined" => {
                let is_user = self.user_functions.contains_key(func_lower.as_slice());
                Some(if is_user { Value::True } else { Value::False })
            }
            b"getfilename" => {
                if self.user_functions.contains_key(func_lower.as_slice()) {
                    Some(Value::String(PhpString::from_string(self.current_file.clone())))
                } else {
                    Some(Value::False)
                }
            }
            b"getstartline" => {
                if let Some(op_array) = self.user_functions.get(func_lower.as_slice()) {
                    Some(Value::Long(op_array.decl_line as i64))
                } else {
                    Some(Value::False)
                }
            }
            b"getendline" => {
                // We don't track end line in op_array, return false
                Some(Value::False)
            }
            b"getdoccomment" => {
                Some(Value::False)
            }
            b"getstaticvariables" => {
                // Return static variables of the function
                let mut result = PhpArray::new();
                // Look up static vars with the function name prefix
                let prefix = format!("{}::", String::from_utf8_lossy(&func_lower));
                for (key, val) in &self.static_vars {
                    let key_str = String::from_utf8_lossy(key);
                    if key_str.starts_with(&prefix) {
                        let var_name = &key_str[prefix.len()..];
                        result.set(ArrayKey::String(PhpString::from_string(var_name.to_string())), val.clone());
                    }
                }
                // Also check OpArray for declared static vars with default values
                if let Some(op_array) = self.user_functions.get(func_lower.as_slice()) {
                    // Static vars are initialized by ops, we check the cv_names
                    // Actually we just return what we have from static_vars
                }
                Some(Value::Array(Rc::new(RefCell::new(result))))
            }
            b"getnumberofparameters" => {
                if let Some(op_array) = self.user_functions.get(func_lower.as_slice()) {
                    Some(Value::Long(op_array.param_count as i64))
                } else if let Some(_) = self.functions.get(func_lower.as_slice()) {
                    // For built-in functions, check param names
                    let count = self.builtin_param_names.get(func_lower.as_slice())
                        .map(|p| p.len() as i64)
                        .unwrap_or(0);
                    Some(Value::Long(count))
                } else {
                    Some(Value::Long(0))
                }
            }
            b"getnumberofrequiredparameters" => {
                if let Some(op_array) = self.user_functions.get(func_lower.as_slice()) {
                    Some(Value::Long(op_array.required_param_count as i64))
                } else {
                    Some(Value::Long(0))
                }
            }
            b"returnsreference" => {
                Some(Value::False)
            }
            b"getparameters" => {
                if let Some(op_array) = self.user_functions.get(func_lower.as_slice()).cloned() {
                    Some(self.create_reflection_parameters(&op_array))
                } else {
                    Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
                }
            }
            b"getreturntype" => {
                if let Some(op_array) = self.user_functions.get(func_lower.as_slice()) {
                    if let Some(ref rt) = op_array.return_type {
                        let rt = rt.clone();
                        Some(self.create_reflection_type(&rt))
                    } else {
                        Some(Value::Null)
                    }
                } else {
                    Some(Value::Null)
                }
            }
            b"hasreturntype" => {
                let has = self.user_functions.get(func_lower.as_slice())
                    .and_then(|op| op.return_type.as_ref())
                    .is_some();
                Some(if has { Value::True } else { Value::False })
            }
            b"isclosure" => {
                let ob = obj.borrow();
                let is_closure = ob.has_property(b"__reflection_is_closure");
                Some(if is_closure { Value::True } else { Value::False })
            }
            b"isvariadic" => {
                let is_variadic = self.user_functions.get(func_lower.as_slice())
                    .and_then(|op| op.variadic_param)
                    .is_some();
                Some(if is_variadic { Value::True } else { Value::False })
            }
            b"isdeprecated" => {
                Some(Value::False)
            }
            b"isgenerator" => {
                let is_gen = self.user_functions.get(func_lower.as_slice())
                    .map(|op| op.is_generator)
                    .unwrap_or(false);
                Some(if is_gen { Value::True } else { Value::False })
            }
            b"getextension" => {
                Some(Value::Null)
            }
            b"getextensionname" => {
                if self.functions.contains_key(func_lower.as_slice()) {
                    // Built-in functions have extension names, but we don't track them
                    Some(Value::False)
                } else {
                    Some(Value::False)
                }
            }
            b"getattributes" => {
                Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
            }
            b"__tostring" => {
                let ob = obj.borrow();
                let name = ob.get_property(b"name").to_php_string().to_string_lossy();
                drop(ob);
                if let Some(op_array) = self.user_functions.get(func_lower.as_slice()).cloned() {
                    let mut s = format!("Function [ <user> function {} ] {{\n", name);
                    s.push_str(&format!("  @@ {} {}\n\n", self.current_file, op_array.decl_line));
                    s.push_str(&format!("  - Parameters [{}] {{\n", op_array.param_count));
                    for i in 0..op_array.param_count as usize {
                        if i < op_array.cv_names.len() {
                            let pname = String::from_utf8_lossy(&op_array.cv_names[i]);
                            let required = i < op_array.required_param_count as usize;
                            s.push_str(&format!("    Parameter #{} [ <{}> ${} ]\n", i,
                                if required { "required" } else { "optional" }, pname));
                        }
                    }
                    s.push_str("  }\n}\n");
                    Some(Value::String(PhpString::from_string(s)))
                } else {
                    Some(Value::String(PhpString::from_string(format!("Function [ <internal> function {} ]", name))))
                }
            }
            _ => None,
        }
    }

    /// ReflectionFunction methods with args
    fn reflection_function_docall(
        &mut self,
        method: &[u8],
        args: &[Value],
    ) -> Option<Value> {
        let this = args.first()?;
        if let Value::Object(obj) = this {
            let ob = obj.borrow();
            let target = ob.get_property(b"__reflection_target").to_php_string();
            let func_lower = target.as_bytes().to_vec();
            drop(ob);

            match method {
                b"invoke" => {
                    let invoke_args: Vec<Value> = args.iter().skip(1).cloned().collect();
                    if let Some(op_array) = self.user_functions.get(&func_lower).cloned() {
                        let mut cvs = vec![Value::Undef; op_array.cv_names.len()];
                        for (i, arg) in invoke_args.iter().enumerate() {
                            if i < cvs.len() {
                                cvs[i] = arg.clone();
                            }
                        }
                        let result = self.execute_op_array(&op_array, cvs).unwrap_or(Value::Null);
                        Some(result)
                    } else if let Some(func) = self.functions.get(&func_lower).cloned() {
                        let result = func(self, &invoke_args).unwrap_or(Value::Null);
                        Some(result)
                    } else {
                        Some(Value::Null)
                    }
                }
                b"invokeargs" => {
                    let args_arr = args.get(1).cloned().unwrap_or(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
                    let invoke_args: Vec<Value> = if let Value::Array(arr) = &args_arr {
                        arr.borrow().iter().map(|(_, v)| v.clone()).collect()
                    } else {
                        vec![]
                    };
                    if let Some(op_array) = self.user_functions.get(&func_lower).cloned() {
                        let mut cvs = vec![Value::Undef; op_array.cv_names.len()];
                        for (i, arg) in invoke_args.iter().enumerate() {
                            if i < cvs.len() {
                                cvs[i] = arg.clone();
                            }
                        }
                        let result = self.execute_op_array(&op_array, cvs).unwrap_or(Value::Null);
                        Some(result)
                    } else if let Some(func) = self.functions.get(&func_lower).cloned() {
                        let result = func(self, &invoke_args).unwrap_or(Value::Null);
                        Some(result)
                    } else {
                        Some(Value::Null)
                    }
                }
                _ => None,
            }
        } else {
            None
        }
    }

    /// ReflectionProperty no-arg method dispatch
    fn reflection_property_method(
        &mut self,
        method: &[u8],
        obj: &Rc<RefCell<PhpObject>>,
    ) -> Option<Value> {
        let ob = obj.borrow();
        let class_name = ob.get_property(b"__reflection_class").to_php_string().to_string_lossy();
        let prop_name = ob.get_property(b"__reflection_prop").to_php_string().to_string_lossy();
        let class_lower: Vec<u8> = class_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
        drop(ob);

        match method {
            b"getname" => {
                let ob = obj.borrow();
                Some(ob.get_property(b"name"))
            }
            b"getdeclaringclass" => {
                let declaring = self.classes.get(&class_lower)
                    .and_then(|c| c.properties.iter().find(|p| p.name == prop_name.as_bytes())
                        .map(|p| String::from_utf8_lossy(&p.declaring_class).to_string()))
                    .unwrap_or(class_name.clone());
                let declaring_canonical = self.classes.get(declaring.as_bytes().to_ascii_lowercase().as_slice())
                    .map(|c| String::from_utf8_lossy(&c.name).to_string())
                    .unwrap_or(declaring.clone());
                Some(self.create_reflection_class(&declaring_canonical))
            }
            b"ispublic" => {
                let vis = self.classes.get(&class_lower)
                    .and_then(|c| c.properties.iter().find(|p| p.name == prop_name.as_bytes()).map(|p| p.visibility))
                    .unwrap_or(Visibility::Public);
                Some(if vis == Visibility::Public { Value::True } else { Value::False })
            }
            b"isprotected" => {
                let vis = self.classes.get(&class_lower)
                    .and_then(|c| c.properties.iter().find(|p| p.name == prop_name.as_bytes()).map(|p| p.visibility))
                    .unwrap_or(Visibility::Public);
                Some(if vis == Visibility::Protected { Value::True } else { Value::False })
            }
            b"isprivate" => {
                let vis = self.classes.get(&class_lower)
                    .and_then(|c| c.properties.iter().find(|p| p.name == prop_name.as_bytes()).map(|p| p.visibility))
                    .unwrap_or(Visibility::Public);
                Some(if vis == Visibility::Private { Value::True } else { Value::False })
            }
            b"isstatic" => {
                let is_static = self.classes.get(&class_lower)
                    .and_then(|c| c.properties.iter().find(|p| p.name == prop_name.as_bytes()).map(|p| p.is_static))
                    .unwrap_or(false);
                Some(if is_static { Value::True } else { Value::False })
            }
            b"isreadonly" => {
                let is_readonly = self.classes.get(&class_lower)
                    .and_then(|c| c.properties.iter().find(|p| p.name == prop_name.as_bytes()).map(|p| p.is_readonly))
                    .unwrap_or(false);
                Some(if is_readonly { Value::True } else { Value::False })
            }
            b"isdefault" => {
                let is_default = self.classes.get(&class_lower)
                    .map(|c| c.properties.iter().any(|p| p.name == prop_name.as_bytes()))
                    .unwrap_or(false);
                Some(if is_default { Value::True } else { Value::False })
            }
            b"getdefaultvalue" => {
                let default = self.classes.get(&class_lower)
                    .and_then(|c| c.properties.iter().find(|p| p.name == prop_name.as_bytes()).map(|p| p.default.clone()))
                    .unwrap_or(Value::Null);
                Some(default)
            }
            b"hasdefaultvalue" => {
                let has = self.classes.get(&class_lower)
                    .and_then(|c| c.properties.iter().find(|p| p.name == prop_name.as_bytes()))
                    .map(|p| !matches!(p.default, Value::Undef))
                    .unwrap_or(false);
                Some(if has { Value::True } else { Value::False })
            }
            b"getmodifiers" => {
                let mods = self.classes.get(&class_lower)
                    .and_then(|c| c.properties.iter().find(|p| p.name == prop_name.as_bytes()).map(|p| self.reflection_property_modifiers(p)))
                    .unwrap_or(1);
                Some(Value::Long(mods))
            }
            b"getdoccomment" => {
                Some(Value::False)
            }
            b"hastype" => {
                Some(Value::False)
            }
            b"gettype" => {
                Some(Value::Null)
            }
            b"setaccessible" => {
                Some(Value::Null)
            }
            b"getattributes" => {
                Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
            }
            _ => None,
        }
    }

    /// ReflectionProperty methods with args
    fn reflection_property_docall(
        &mut self,
        method: &[u8],
        args: &[Value],
    ) -> Option<Value> {
        let this = args.first()?;
        if let Value::Object(obj) = this {
            let ob = obj.borrow();
            let prop_name = ob.get_property(b"__reflection_prop").to_php_string().to_string_lossy();
            drop(ob);

            match method {
                b"getvalue" => {
                    let target = args.get(1)?;
                    if let Value::Object(target_obj) = target {
                        let target_ob = target_obj.borrow();
                        Some(target_ob.get_property(prop_name.as_bytes()))
                    } else {
                        Some(Value::Null)
                    }
                }
                b"setvalue" => {
                    if args.len() >= 3 {
                        let target = &args[1];
                        let value = args[2].clone();
                        if let Value::Object(target_obj) = target {
                            let mut target_ob = target_obj.borrow_mut();
                            target_ob.set_property(prop_name.as_bytes().to_vec(), value);
                        }
                    }
                    Some(Value::Null)
                }
                _ => None,
            }
        } else {
            None
        }
    }

    /// ReflectionParameter no-arg method dispatch
    fn reflection_parameter_method(
        &mut self,
        method: &[u8],
        obj: &Rc<RefCell<PhpObject>>,
    ) -> Option<Value> {
        let ob = obj.borrow();
        let func_target = ob.get_property(b"__reflection_func").to_php_string();
        let param_idx = ob.get_property(b"__reflection_param_idx").to_long() as usize;
        drop(ob);

        match method {
            b"getname" => {
                let ob = obj.borrow();
                Some(ob.get_property(b"name"))
            }
            b"getposition" => {
                Some(Value::Long(param_idx as i64))
            }
            b"isoptional" => {
                if let Some(op_array) = self.user_functions.get(func_target.as_bytes()) {
                    Some(if param_idx >= op_array.required_param_count as usize {
                        Value::True
                    } else {
                        Value::False
                    })
                } else {
                    Some(Value::False)
                }
            }
            b"hasdefaultvalue" | b"isdefaultvalueavailable" => {
                if let Some(op_array) = self.user_functions.get(func_target.as_bytes()) {
                    Some(if param_idx >= op_array.required_param_count as usize {
                        Value::True
                    } else {
                        Value::False
                    })
                } else {
                    Some(Value::False)
                }
            }
            b"getdefaultvalue" => {
                // We don't easily have access to default values at runtime
                Some(Value::Null)
            }
            b"allowsnull" => {
                Some(Value::True)
            }
            b"isvariadic" => {
                if let Some(op_array) = self.user_functions.get(func_target.as_bytes()) {
                    if let Some(variadic_idx) = op_array.variadic_param {
                        Some(if param_idx == variadic_idx as usize {
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
            b"ispassedbyreference" => {
                Some(Value::False)
            }
            b"hastype" => {
                if let Some(op_array) = self.user_functions.get(func_target.as_bytes()) {
                    let has = param_idx < op_array.param_types.len()
                        && op_array.param_types[param_idx].is_some();
                    Some(if has { Value::True } else { Value::False })
                } else {
                    Some(Value::False)
                }
            }
            b"gettype" => {
                let param_type = self.user_functions.get(func_target.as_bytes())
                    .and_then(|op_array| {
                        if param_idx < op_array.param_types.len() {
                            op_array.param_types[param_idx].as_ref().map(|pti| pti.param_type.clone())
                        } else {
                            None
                        }
                    });
                if let Some(pt) = param_type {
                    Some(self.create_reflection_type(&pt))
                } else {
                    Some(Value::Null)
                }
            }
            b"getdeclaringfunction" => {
                // Return a ReflectionFunction for the declaring function
                let ob = obj.borrow();
                let func_name = ob.get_property(b"name").to_php_string().to_string_lossy();
                drop(ob);
                let obj_id = self.next_object_id;
                self.next_object_id += 1;
                let mut rf_obj = PhpObject::new(b"ReflectionFunction".to_vec(), obj_id);
                rf_obj.set_property(b"name".to_vec(), Value::String(PhpString::from_string(func_name.clone())));
                rf_obj.set_property(b"__reflection_target".to_vec(), Value::String(func_target.clone()));
                Some(Value::Object(Rc::new(RefCell::new(rf_obj))))
            }
            b"getdeclaringclass" => {
                // Returns null for functions, ReflectionClass for methods
                Some(Value::Null)
            }
            b"getclass" => {
                // Deprecated: returns the class if the param type hints a class
                Some(Value::Null)
            }
            b"isdefaultvalueconstant" => {
                Some(Value::False)
            }
            b"getdefaultvalueconstantname" => {
                Some(Value::String(PhpString::empty()))
            }
            b"ispromoted" => {
                Some(Value::False)
            }
            b"canbepassedbyvalue" => {
                Some(Value::True)
            }
            b"getattributes" => {
                Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
            }
            b"__tostring" => {
                let ob = obj.borrow();
                let name = ob.get_property(b"name").to_php_string().to_string_lossy();
                drop(ob);
                let is_optional = if let Some(op_array) = self.user_functions.get(func_target.as_bytes()) {
                    param_idx >= op_array.required_param_count as usize
                } else {
                    false
                };
                let kind = if is_optional { "optional" } else { "required" };
                Some(Value::String(PhpString::from_string(
                    format!("Parameter #{} [ <{}> ${} ]", param_idx, kind, name)
                )))
            }
            _ => None,
        }
    }

    /// ReflectionExtension no-arg method dispatch
    fn reflection_extension_method(
        &mut self,
        method: &[u8],
        obj: &Rc<RefCell<PhpObject>>,
    ) -> Option<Value> {
        let ob = obj.borrow();
        let name = ob.get_property(b"name").to_php_string().to_string_lossy();
        drop(ob);

        match method {
            b"getname" => {
                Some(Value::String(PhpString::from_string(name)))
            }
            b"getversion" => {
                Some(Value::String(PhpString::from_bytes(b"8.5.4")))
            }
            b"getfunctions" => {
                Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
            }
            b"getclasses" => {
                Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
            }
            b"getclassnames" => {
                Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
            }
            b"getconstants" => {
                Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
            }
            b"getinientries" => {
                Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
            }
            b"getdependencies" => {
                Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
            }
            b"info" => {
                Some(Value::Null)
            }
            b"ispersistent" => {
                Some(Value::True)
            }
            b"istemporary" => {
                Some(Value::False)
            }
            _ => None,
        }
    }

    /// ReflectionNamedType no-arg method dispatch
    fn reflection_named_type_method(
        &mut self,
        method: &[u8],
        obj: &Rc<RefCell<PhpObject>>,
    ) -> Option<Value> {
        let ob = obj.borrow();
        let type_name = ob.get_property(b"__type_name").to_php_string().to_string_lossy();
        let allows_null = ob.get_property(b"__allows_null");
        let is_builtin_type = ob.get_property(b"__is_builtin");
        drop(ob);

        match method {
            b"getname" => {
                Some(Value::String(PhpString::from_string(type_name)))
            }
            b"allowsnull" => {
                Some(if matches!(allows_null, Value::True) { Value::True } else { Value::False })
            }
            b"isbuiltin" => {
                Some(if matches!(is_builtin_type, Value::True) { Value::True } else { Value::False })
            }
            b"__tostring" => {
                let ob = obj.borrow();
                let nullable = ob.get_property(b"__allows_null");
                let name = ob.get_property(b"__type_name").to_php_string().to_string_lossy();
                drop(ob);
                if matches!(nullable, Value::True) && name != "null" && name != "mixed" {
                    Some(Value::String(PhpString::from_string(format!("?{}", name))))
                } else {
                    Some(Value::String(PhpString::from_string(name)))
                }
            }
            _ => None,
        }
    }

    /// ReflectionUnionType / ReflectionIntersectionType method dispatch
    fn reflection_composite_type_method(
        &mut self,
        method: &[u8],
        obj: &Rc<RefCell<PhpObject>>,
    ) -> Option<Value> {
        let ob = obj.borrow();
        let allows_null = ob.get_property(b"__allows_null");
        drop(ob);

        match method {
            b"gettypes" => {
                let ob = obj.borrow();
                Some(ob.get_property(b"__types"))
            }
            b"allowsnull" => {
                Some(if matches!(allows_null, Value::True) { Value::True } else { Value::False })
            }
            b"__tostring" => {
                let ob = obj.borrow();
                let types = ob.get_property(b"__types");
                let class_lower: Vec<u8> = ob.class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                drop(ob);
                let sep = if class_lower == b"reflectionuniontype" { "|" } else { "&" };
                if let Value::Array(arr) = types {
                    let names: Vec<String> = arr.borrow().iter().map(|(_, v)| {
                        if let Value::Object(t) = v {
                            let t = t.borrow();
                            t.get_property(b"__type_name").to_php_string().to_string_lossy()
                        } else {
                            String::new()
                        }
                    }).collect();
                    Some(Value::String(PhpString::from_string(names.join(sep))))
                } else {
                    Some(Value::String(PhpString::empty()))
                }
            }
            _ => None,
        }
    }

    /// ReflectionClassConstant method dispatch
    fn reflection_class_constant_method(
        &mut self,
        method: &[u8],
        obj: &Rc<RefCell<PhpObject>>,
    ) -> Option<Value> {
        let ob = obj.borrow();
        match method {
            b"getname" => Some(ob.get_property(b"name")),
            b"getvalue" => Some(ob.get_property(b"__reflection_value")),
            b"getdeclaringclass" => {
                let class_name = ob.get_property(b"class").to_php_string().to_string_lossy();
                drop(ob);
                Some(self.create_reflection_class(&class_name))
            }
            b"ispublic" => Some(Value::True),
            b"isprotected" => Some(Value::False),
            b"isprivate" => Some(Value::False),
            b"getmodifiers" => Some(Value::Long(1)), // IS_PUBLIC
            b"getdoccomment" => Some(Value::False),
            b"isfinal" => Some(Value::False),
            b"isenumcase" => {
                let val = ob.get_property(b"__reflection_value");
                drop(ob);
                Some(if Self::is_enum_case(&val) { Value::True } else { Value::False })
            }
            b"isdeprecated" => Some(Value::False),
            b"hastype" => Some(Value::False),
            b"gettype" => Some(Value::Null),
            b"__tostring" => {
                let name = ob.get_property(b"name").to_php_string().to_string_lossy();
                let val = ob.get_property(b"__reflection_value");
                drop(ob);
                Some(Value::String(PhpString::from_string(format!("Constant [ public {} {} ]", name, val.to_php_string().to_string_lossy()))))
            }
            _ => None,
        }
    }

    /// Create ReflectionParameter objects for a function's parameters
    fn create_reflection_parameters(&mut self, op_array: &OpArray) -> Value {
        let mut result = PhpArray::new();
        for i in 0..op_array.param_count as usize {
            let param_name = if i < op_array.cv_names.len() {
                String::from_utf8_lossy(&op_array.cv_names[i]).to_string()
            } else {
                format!("param{}", i)
            };
            let obj_id = self.next_object_id;
            self.next_object_id += 1;
            let mut obj = PhpObject::new(b"ReflectionParameter".to_vec(), obj_id);
            obj.set_property(b"name".to_vec(), Value::String(PhpString::from_string(param_name)));
            result.push(Value::Object(Rc::new(RefCell::new(obj))));
        }
        Value::Array(Rc::new(RefCell::new(result)))
    }

    /// Handle static method calls on Reflection classes
    /// Get a constant for a built-in class
    fn get_builtin_class_constant(&self, class_lower: &[u8], const_name: &[u8]) -> Option<Value> {
        match class_lower {
            b"reflectionmethod" | b"reflectionfunction" | b"reflectionfunctionabstract" => {
                match const_name {
                    b"IS_STATIC" => Some(Value::Long(0x10)),
                    b"IS_ABSTRACT" => Some(Value::Long(0x40)),
                    b"IS_FINAL" => Some(Value::Long(0x20)),
                    b"IS_PUBLIC" => Some(Value::Long(1)),
                    b"IS_PROTECTED" => Some(Value::Long(2)),
                    b"IS_PRIVATE" => Some(Value::Long(4)),
                    b"IS_DEPRECATED" => Some(Value::Long(0x40000)),
                    _ => None,
                }
            }
            b"reflectionproperty" => {
                match const_name {
                    b"IS_STATIC" => Some(Value::Long(0x10)),
                    b"IS_PUBLIC" => Some(Value::Long(1)),
                    b"IS_PROTECTED" => Some(Value::Long(2)),
                    b"IS_PRIVATE" => Some(Value::Long(4)),
                    b"IS_READONLY" => Some(Value::Long(0x10000)),
                    _ => None,
                }
            }
            b"reflectionclass" => {
                match const_name {
                    b"IS_IMPLICIT_ABSTRACT" => Some(Value::Long(0x10)),
                    b"IS_EXPLICIT_ABSTRACT" => Some(Value::Long(0x40)),
                    b"IS_FINAL" => Some(Value::Long(0x20)),
                    b"IS_READONLY" => Some(Value::Long(0x10000)),
                    _ => None,
                }
            }
            b"reflectionclassconstant" => {
                match const_name {
                    b"IS_PUBLIC" => Some(Value::Long(1)),
                    b"IS_PROTECTED" => Some(Value::Long(2)),
                    b"IS_PRIVATE" => Some(Value::Long(4)),
                    b"IS_FINAL" => Some(Value::Long(0x20)),
                    _ => None,
                }
            }
            b"reflectionattribute" => {
                match const_name {
                    b"IS_INSTANCEOF" => Some(Value::Long(2)),
                    _ => None,
                }
            }
            b"arrayobject" | b"arrayiterator" | b"recursivearrayiterator" => {
                match const_name {
                    b"STD_PROP_LIST" => Some(Value::Long(1)),
                    b"ARRAY_AS_PROPS" => Some(Value::Long(2)),
                    _ => None,
                }
            }
            b"spldoublylinkedlist" | b"splstack" | b"splqueue" => {
                match const_name {
                    b"IT_MODE_LIFO" => Some(Value::Long(2)),
                    b"IT_MODE_FIFO" => Some(Value::Long(0)),
                    b"IT_MODE_DELETE" => Some(Value::Long(1)),
                    b"IT_MODE_KEEP" => Some(Value::Long(0)),
                    _ => None,
                }
            }
            b"splobjectstorage" => {
                match const_name {
                    _ => None,
                }
            }
            b"cachingiterator" => {
                match const_name {
                    b"CALL_TOSTRING" => Some(Value::Long(1)),
                    b"CATCH_GET_CHILD" => Some(Value::Long(16)),
                    b"TOSTRING_USE_KEY" => Some(Value::Long(2)),
                    b"TOSTRING_USE_CURRENT" => Some(Value::Long(4)),
                    b"TOSTRING_USE_INNER" => Some(Value::Long(8)),
                    b"FULL_CACHE" => Some(Value::Long(256)),
                    _ => None,
                }
            }
            b"regexiterator" | b"recursiveregexiterator" => {
                match const_name {
                    b"MATCH" | b"USE_KEY" => Some(Value::Long(0)),
                    b"GET_MATCH" => Some(Value::Long(1)),
                    b"ALL_MATCHES" => Some(Value::Long(2)),
                    b"SPLIT" => Some(Value::Long(3)),
                    b"REPLACE" => Some(Value::Long(4)),
                    _ => None,
                }
            }
            b"multipleiterator" => {
                match const_name {
                    b"MIT_NEED_ANY" => Some(Value::Long(0)),
                    b"MIT_NEED_ALL" => Some(Value::Long(1)),
                    b"MIT_KEYS_NUMERIC" => Some(Value::Long(0)),
                    b"MIT_KEYS_ASSOC" => Some(Value::Long(2)),
                    _ => None,
                }
            }
            b"splfileobject" => {
                match const_name {
                    b"DROP_NEW_LINE" => Some(Value::Long(1)),
                    b"READ_AHEAD" => Some(Value::Long(2)),
                    b"SKIP_EMPTY" => Some(Value::Long(4)),
                    b"READ_CSV" => Some(Value::Long(8)),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Handle static method calls on Reflection classes
    fn reflection_static_call(&mut self, class_lower: &str, method_lower: &str, args: &[Value], line: u32) -> Option<Value> {
        match class_lower {
            "reflection" => {
                match method_lower {
                    "getmodifiernames" => {
                        let modifiers = args.first().map(|v| v.to_long()).unwrap_or(0);
                        let mut names = PhpArray::new();
                        if modifiers & 0x10 != 0 { names.push(Value::String(PhpString::from_bytes(b"static"))); }
                        if modifiers & 0x40 != 0 { names.push(Value::String(PhpString::from_bytes(b"abstract"))); }
                        if modifiers & 0x20 != 0 { names.push(Value::String(PhpString::from_bytes(b"final"))); }
                        if modifiers & 1 != 0 { names.push(Value::String(PhpString::from_bytes(b"public"))); }
                        if modifiers & 2 != 0 { names.push(Value::String(PhpString::from_bytes(b"protected"))); }
                        if modifiers & 4 != 0 { names.push(Value::String(PhpString::from_bytes(b"private"))); }
                        if modifiers & 0x10000 != 0 { names.push(Value::String(PhpString::from_bytes(b"readonly"))); }
                        Some(Value::Array(Rc::new(RefCell::new(names))))
                    }
                    _ => None,
                }
            }
            "reflectionmethod" => {
                match method_lower {
                    "createfrommethodname" => {
                        let method_str = args.first().map(|v| v.to_php_string().to_string_lossy()).unwrap_or_default();
                        if let Some(pos) = method_str.find("::") {
                            let class_name = &method_str[..pos];
                            let method_name = &method_str[pos + 2..];
                            // Create a ReflectionMethod
                            let class_lower: Vec<u8> = class_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
                            let method_lower_bytes: Vec<u8> = method_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();

                            // Check class exists
                            if !self.classes.contains_key(&class_lower) {
                                let err_msg = format!("Class \"{}\" does not exist", class_name);
                                let exc = self.create_exception(b"ReflectionException", &err_msg, line);
                                self.current_exception = Some(exc);
                                return Some(Value::Null);
                            }

                            // Check method exists
                            let method_exists = self.classes.get(&class_lower)
                                .map(|c| c.get_method(&method_lower_bytes).is_some())
                                .unwrap_or(false);

                            if !method_exists {
                                let canonical_class = self.classes.get(&class_lower)
                                    .map(|c| String::from_utf8_lossy(&c.name).to_string())
                                    .unwrap_or(class_name.to_string());
                                let err_msg = format!("Method {}::{}() does not exist", canonical_class, method_name);
                                let exc = self.create_exception(b"ReflectionException", &err_msg, line);
                                self.current_exception = Some(exc);
                                return Some(Value::Null);
                            }

                            let canonical_class = self.classes.get(&class_lower)
                                .map(|c| String::from_utf8_lossy(&c.name).to_string())
                                .unwrap_or(class_name.to_string());
                            Some(self.create_reflection_method(&canonical_class, method_name))
                        } else {
                            let err_msg = "ReflectionMethod::createFromMethodName(): Argument #1 ($method) must be a valid method name".to_string();
                            let exc = self.create_exception(b"ReflectionException", &err_msg, line);
                            self.current_exception = Some(exc);
                            Some(Value::Null)
                        }
                    }
                    _ => None,
                }
            }
            "reflectionclass" => {
                match method_lower {
                    "export" => {
                        // Deprecated, return null
                        Some(Value::Null)
                    }
                    _ => None,
                }
            }
            "reflectionenum" => {
                match method_lower {
                    "export" => Some(Value::Null),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Create a ReflectionType object from a ParamType
    fn create_reflection_type(&mut self, param_type: &ParamType) -> Value {
        match param_type {
            ParamType::Simple(name) => {
                let type_name = String::from_utf8_lossy(name).to_string();
                let obj_id = self.next_object_id;
                self.next_object_id += 1;
                let mut obj = PhpObject::new(b"ReflectionNamedType".to_vec(), obj_id);
                let is_builtin = matches!(
                    name.as_slice(),
                    b"int" | b"float" | b"string" | b"bool" | b"array" | b"callable"
                        | b"void" | b"null" | b"mixed" | b"never" | b"object"
                        | b"iterable" | b"false" | b"true"
                );
                obj.set_property(b"__type_name".to_vec(), Value::String(PhpString::from_string(type_name)));
                obj.set_property(b"__allows_null".to_vec(), Value::False);
                obj.set_property(b"__is_builtin".to_vec(), if is_builtin { Value::True } else { Value::False });
                Value::Object(Rc::new(RefCell::new(obj)))
            }
            ParamType::Nullable(inner) => {
                match inner.as_ref() {
                    ParamType::Simple(name) => {
                        let type_name = String::from_utf8_lossy(name).to_string();
                        let obj_id = self.next_object_id;
                        self.next_object_id += 1;
                        let mut obj = PhpObject::new(b"ReflectionNamedType".to_vec(), obj_id);
                        let is_builtin = matches!(
                            name.as_slice(),
                            b"int" | b"float" | b"string" | b"bool" | b"array" | b"callable"
                                | b"void" | b"null" | b"mixed" | b"never" | b"object"
                                | b"iterable" | b"false" | b"true"
                        );
                        obj.set_property(b"__type_name".to_vec(), Value::String(PhpString::from_string(type_name)));
                        obj.set_property(b"__allows_null".to_vec(), Value::True);
                        obj.set_property(b"__is_builtin".to_vec(), if is_builtin { Value::True } else { Value::False });
                        Value::Object(Rc::new(RefCell::new(obj)))
                    }
                    _ => {
                        // Nullable complex type becomes union with null
                        let inner_type = self.create_reflection_type(inner);
                        let null_type = self.create_reflection_type(&ParamType::Simple(b"null".to_vec()));
                        let mut types = PhpArray::new();
                        types.push(inner_type);
                        types.push(null_type);
                        let obj_id = self.next_object_id;
                        self.next_object_id += 1;
                        let mut obj = PhpObject::new(b"ReflectionUnionType".to_vec(), obj_id);
                        obj.set_property(b"__types".to_vec(), Value::Array(Rc::new(RefCell::new(types))));
                        obj.set_property(b"__allows_null".to_vec(), Value::True);
                        Value::Object(Rc::new(RefCell::new(obj)))
                    }
                }
            }
            ParamType::Union(types) => {
                let mut type_arr = PhpArray::new();
                let mut allows_null = false;
                for t in types {
                    let rt = self.create_reflection_type(t);
                    if let ParamType::Simple(name) = t {
                        if name == b"null" {
                            allows_null = true;
                        }
                    }
                    type_arr.push(rt);
                }
                let obj_id = self.next_object_id;
                self.next_object_id += 1;
                let mut obj = PhpObject::new(b"ReflectionUnionType".to_vec(), obj_id);
                obj.set_property(b"__types".to_vec(), Value::Array(Rc::new(RefCell::new(type_arr))));
                obj.set_property(b"__allows_null".to_vec(), if allows_null { Value::True } else { Value::False });
                Value::Object(Rc::new(RefCell::new(obj)))
            }
            ParamType::Intersection(types) => {
                let mut type_arr = PhpArray::new();
                for t in types {
                    let rt = self.create_reflection_type(t);
                    type_arr.push(rt);
                }
                let obj_id = self.next_object_id;
                self.next_object_id += 1;
                let mut obj = PhpObject::new(b"ReflectionIntersectionType".to_vec(), obj_id);
                obj.set_property(b"__types".to_vec(), Value::Array(Rc::new(RefCell::new(type_arr))));
                obj.set_property(b"__allows_null".to_vec(), Value::False);
                Value::Object(Rc::new(RefCell::new(obj)))
            }
        }
    }

    /// Get timezone offset in seconds and abbreviation for a timezone name
    fn get_tz_offset(&self, tz_name: &str) -> (i64, String) {
        match tz_name {
            "UTC" | "utc" => (0, "UTC".to_string()),
            "America/New_York" | "US/Eastern" => (-5 * 3600, "EST".to_string()),
            "America/Chicago" | "US/Central" => (-6 * 3600, "CST".to_string()),
            "America/Denver" | "US/Mountain" => (-7 * 3600, "MST".to_string()),
            "America/Los_Angeles" | "US/Pacific" => (-8 * 3600, "PST".to_string()),
            "Europe/London" => (0, "GMT".to_string()),
            "Europe/Paris" | "Europe/Berlin" | "Europe/Amsterdam" | "Europe/Brussels" | "Europe/Rome" | "CET" => (3600, "CET".to_string()),
            "Europe/Helsinki" | "Europe/Athens" | "EET" => (2 * 3600, "EET".to_string()),
            "Europe/Moscow" => (3 * 3600, "MSK".to_string()),
            "Asia/Tokyo" | "Japan" => (9 * 3600, "JST".to_string()),
            "Asia/Shanghai" | "Asia/Hong_Kong" | "PRC" => (8 * 3600, "CST".to_string()),
            "Asia/Kolkata" | "Asia/Calcutta" => (5 * 3600 + 1800, "IST".to_string()),
            "Australia/Sydney" => (10 * 3600, "AEST".to_string()),
            "Pacific/Auckland" | "NZ" => (12 * 3600, "NZST".to_string()),
            _ => (0, "UTC".to_string()),
        }
    }

    /// Format a timestamp with timezone info
    fn format_datetime_timestamp_tz(&self, format: &str, local_secs: i64, tz_abbrev: &str, offset_secs: i64) -> String {
        let days_since_epoch = if local_secs >= 0 { local_secs / 86400 } else { (local_secs - 86399) / 86400 };
        let time_of_day = ((local_secs % 86400) + 86400) % 86400;
        let hours = time_of_day / 3600;
        let minutes = (time_of_day % 3600) / 60;
        let seconds = time_of_day % 60;
        let (year, month, day) = Self::days_to_ymd_static(days_since_epoch);

        let mut result = String::new();
        let fmt_bytes = format.as_bytes();
        let mut i = 0;
        while i < fmt_bytes.len() {
            let c = fmt_bytes[i];
            if c == b'\\' && i + 1 < fmt_bytes.len() { result.push(fmt_bytes[i+1] as char); i += 2; continue; }
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
                b'g' => result.push_str(&format!("{}", if hours == 0 { 12 } else if hours > 12 { hours - 12 } else { hours })),
                b'h' => result.push_str(&format!("{:02}", if hours == 0 { 12 } else if hours > 12 { hours - 12 } else { hours })),
                b'e' | b'T' => result.push_str(tz_abbrev),
                b'O' => { let sign = if offset_secs < 0 { '-' } else { '+' }; let abs = offset_secs.unsigned_abs(); result.push_str(&format!("{}{:02}{:02}", sign, abs/3600, (abs%3600)/60)); }
                b'P' => { let sign = if offset_secs < 0 { '-' } else { '+' }; let abs = offset_secs.unsigned_abs(); result.push_str(&format!("{}{:02}:{:02}", sign, abs/3600, (abs%3600)/60)); }
                b'p' => { if offset_secs == 0 { result.push('Z'); } else { let sign = if offset_secs < 0 { '-' } else { '+' }; let abs = offset_secs.unsigned_abs(); result.push_str(&format!("{}{:02}:{:02}", sign, abs/3600, (abs%3600)/60)); } }
                b'Z' => result.push_str(&format!("{}", offset_secs)),
                b'U' => result.push_str(&format!("{}", local_secs - offset_secs)),
                b'N' => { let dow = ((days_since_epoch % 7 + 7) % 7) + 1; result.push_str(&format!("{}", if dow == 0 { 7 } else { dow })); }
                b'w' => { let dow = ((days_since_epoch + 4) % 7 + 7) % 7; result.push_str(&format!("{}", dow)); }
                b'D' => { let dow = ((days_since_epoch + 4) % 7 + 7) % 7; let names = ["Sun","Mon","Tue","Wed","Thu","Fri","Sat"]; result.push_str(names[dow as usize % 7]); }
                b'l' => { let dow = ((days_since_epoch + 4) % 7 + 7) % 7; let names = ["Sunday","Monday","Tuesday","Wednesday","Thursday","Friday","Saturday"]; result.push_str(names[dow as usize % 7]); }
                b'F' => { let names = ["January","February","March","April","May","June","July","August","September","October","November","December"]; if month >= 1 && month <= 12 { result.push_str(names[(month-1) as usize]); } }
                b'M' => { let names = ["Jan","Feb","Mar","Apr","May","Jun","Jul","Aug","Sep","Oct","Nov","Dec"]; if month >= 1 && month <= 12 { result.push_str(names[(month-1) as usize]); } }
                b't' => { let dim = match month { 1|3|5|7|8|10|12 => 31, 4|6|9|11 => 30, 2 => if (year%4==0 && year%100!=0) || year%400==0 { 29 } else { 28 }, _ => 30 }; result.push_str(&format!("{}", dim)); }
                b'L' => { let leap = if (year%4==0 && year%100!=0) || year%400==0 { 1 } else { 0 }; result.push_str(&format!("{}", leap)); }
                b'c' => { let sign = if offset_secs < 0 { '-' } else { '+' }; let abs = offset_secs.unsigned_abs(); result.push_str(&format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{}{:02}:{:02}", year, month, day, hours, minutes, seconds, sign, abs/3600, (abs%3600)/60)); }
                b'r' => { let dow = ((days_since_epoch + 4) % 7 + 7) % 7; let dnames = ["Sun","Mon","Tue","Wed","Thu","Fri","Sat"]; let mnames = ["Jan","Feb","Mar","Apr","May","Jun","Jul","Aug","Sep","Oct","Nov","Dec"]; let sign = if offset_secs < 0 { '-' } else { '+' }; let abs = offset_secs.unsigned_abs(); result.push_str(&format!("{}, {:02} {} {:04} {:02}:{:02}:{:02} {}{:02}{:02}", dnames[dow as usize % 7], day, mnames[(month-1).max(0) as usize % 12], year, hours, minutes, seconds, sign, abs/3600, (abs%3600)/60)); }
                _ => result.push(c as char),
            }
            i += 1;
        }
        result
    }

    fn days_to_ymd_static(days: i64) -> (i64, u32, u32) {
        let z = days + 719468;
        let era = if z >= 0 { z } else { z - 146096 } / 146097;
        let doe = z - era * 146097;
        let yoe = (doe - doe/1460 + doe/36524 - doe/146096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365*yoe + yoe/4 - yoe/100);
        let mp = (5*doy + 2) / 153;
        let d = doy - (153*mp + 2)/5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = if m <= 2 { y + 1 } else { y };
        (year, m as u32, d as u32)
    }

    /// Format a timestamp using PHP date format characters
    /// Format a UTC timestamp as "Y-m-d H:i:s.000000" for DateTime var_dump display
    fn format_utc_datetime(secs: i64) -> String {
        let days_since_epoch = if secs >= 0 { secs / 86400 } else { (secs - 86399) / 86400 };
        let time_of_day = ((secs % 86400) + 86400) % 86400;
        let hours = time_of_day / 3600;
        let minutes = (time_of_day % 3600) / 60;
        let seconds = time_of_day % 60;

        fn dty(days: i64) -> (i64, u32, u32) {
            let mut y = 1970i64;
            let mut d = days;
            if d >= 0 {
                loop {
                    let dy = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) { 366 } else { 365 };
                    if d < dy { break; }
                    d -= dy;
                    y += 1;
                }
            } else {
                loop {
                    y -= 1;
                    let dy = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) { 366 } else { 365 };
                    d += dy;
                    if d >= 0 { break; }
                }
            }
            let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
            let mdays = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
            let mut m = 0u32;
            for md in &mdays {
                if d < *md as i64 { break; }
                d -= *md as i64;
                m += 1;
            }
            (y, m + 1, d as u32 + 1)
        }
        let (year, month, day) = dty(days_since_epoch);
        format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}.000000", year, month, day, hours, minutes, seconds)
    }

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

    /// Max output buffer size (64MB) to prevent OOM from infinite echo loops
    const MAX_OUTPUT_SIZE: usize = 64 * 1024 * 1024;

    pub fn write_output(&mut self, data: &[u8]) {
        if let Some(buf) = self.ob_stack.last_mut() {
            if buf.len() + data.len() <= Self::MAX_OUTPUT_SIZE {
                buf.extend_from_slice(data);
            }
        } else if self.output.len() + data.len() <= Self::MAX_OUTPUT_SIZE {
            self.output.extend_from_slice(data);
        }
    }

    /// Compare two PHP values (for sorting)
    fn php_compare_values(a: &Value, b: &Value) -> std::cmp::Ordering {
        match (a, b) {
            (Value::Long(a), Value::Long(b)) => a.cmp(b),
            (Value::Double(a), Value::Double(b)) => a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal),
            (Value::Long(a), Value::Double(b)) => (*a as f64).partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal),
            (Value::Double(a), Value::Long(b)) => a.partial_cmp(&(*b as f64)).unwrap_or(std::cmp::Ordering::Equal),
            (Value::String(a), Value::String(b)) => {
                // Try numeric comparison first
                let a_str = a.to_string_lossy();
                let b_str = b.to_string_lossy();
                if let (Ok(a_n), Ok(b_n)) = (a_str.parse::<i64>(), b_str.parse::<i64>()) {
                    return a_n.cmp(&b_n);
                }
                if let (Ok(a_f), Ok(b_f)) = (a_str.parse::<f64>(), b_str.parse::<f64>()) {
                    return a_f.partial_cmp(&b_f).unwrap_or(std::cmp::Ordering::Equal);
                }
                a.as_bytes().cmp(b.as_bytes())
            }
            (Value::Null, Value::Null) => std::cmp::Ordering::Equal,
            (Value::Null, _) => std::cmp::Ordering::Less,
            (_, Value::Null) => std::cmp::Ordering::Greater,
            (Value::False, Value::True) => std::cmp::Ordering::Less,
            (Value::True, Value::False) => std::cmp::Ordering::Greater,
            _ => {
                let a_cmp = a.to_long();
                let b_cmp = b.to_long();
                a_cmp.cmp(&b_cmp)
            }
        }
    }

    /// Natural order string comparison
    fn strnatcmp(a: &str, b: &str, case_insensitive: bool) -> std::cmp::Ordering {
        let a_bytes: Vec<u8> = if case_insensitive {
            a.bytes().map(|b| b.to_ascii_lowercase()).collect()
        } else {
            a.bytes().collect()
        };
        let b_bytes: Vec<u8> = if case_insensitive {
            b.bytes().map(|b| b.to_ascii_lowercase()).collect()
        } else {
            b.bytes().collect()
        };
        let mut ai = 0;
        let mut bi = 0;
        loop {
            if ai >= a_bytes.len() && bi >= b_bytes.len() {
                return std::cmp::Ordering::Equal;
            }
            if ai >= a_bytes.len() {
                return std::cmp::Ordering::Less;
            }
            if bi >= b_bytes.len() {
                return std::cmp::Ordering::Greater;
            }
            let a_is_digit = a_bytes[ai].is_ascii_digit();
            let b_is_digit = b_bytes[bi].is_ascii_digit();
            if a_is_digit && b_is_digit {
                // Compare numbers naturally
                // Skip leading zeros
                let a_start = ai;
                let b_start = bi;
                while ai < a_bytes.len() && a_bytes[ai] == b'0' { ai += 1; }
                while bi < b_bytes.len() && b_bytes[bi] == b'0' { bi += 1; }
                let a_num_start = ai;
                let b_num_start = bi;
                while ai < a_bytes.len() && a_bytes[ai].is_ascii_digit() { ai += 1; }
                while bi < b_bytes.len() && b_bytes[bi].is_ascii_digit() { bi += 1; }
                let a_num_len = ai - a_num_start;
                let b_num_len = bi - b_num_start;
                if a_num_len != b_num_len {
                    return a_num_len.cmp(&b_num_len);
                }
                for (a_d, b_d) in a_bytes[a_num_start..ai].iter().zip(b_bytes[b_num_start..bi].iter()) {
                    if a_d != b_d {
                        return a_d.cmp(b_d);
                    }
                }
                // Same number, compare by leading zeros
                let a_zeros = a_num_start - a_start;
                let b_zeros = b_num_start - b_start;
                if a_zeros != b_zeros {
                    // More leading zeros means smaller (PHP behavior: "01" < "1")
                    // Actually in PHP strnatcmp, leading zeros cause character-by-character comparison
                    // but for simplicity, continue
                }
            } else {
                if a_bytes[ai] != b_bytes[bi] {
                    return a_bytes[ai].cmp(&b_bytes[bi]);
                }
                ai += 1;
                bi += 1;
            }
        }
    }

    /// Call a PHP callback function with two arguments for sorting comparison
    fn spl_call_compare_callback(&mut self, callback: &Value, a: &Value, b: &Value) -> i64 {
        // Resolve callback to a function name
        let (func_name, captured) = match callback {
            Value::String(s) => (s.as_bytes().to_vec(), vec![]),
            Value::Array(arr) => {
                let arr = arr.borrow();
                let vals: Vec<Value> = arr.values().cloned().collect();
                if vals.len() >= 2 {
                    let first = &vals[0];
                    let method = vals[1].to_php_string();
                    match first {
                        Value::String(class_name) => {
                            let mut name = class_name.as_bytes().to_vec();
                            name.extend_from_slice(b"::");
                            name.extend_from_slice(method.as_bytes());
                            (name, vec![])
                        }
                        Value::Object(_) => {
                            let class_name = first.to_php_string();
                            let mut name = class_name.as_bytes().to_vec();
                            name.extend_from_slice(b"::");
                            name.extend_from_slice(method.as_bytes());
                            (name, vec![first.clone()])
                        }
                        _ => return 0,
                    }
                } else {
                    return 0;
                }
            }
            _ => return 0,
        };

        // Look up and execute the function
        let func_lower: Vec<u8> = func_name.iter().map(|b| b.to_ascii_lowercase()).collect();

        // Check for class::method patterns
        if let Some(sep_pos) = func_lower.windows(2).position(|w| w == b"::") {
            let class_part = &func_lower[..sep_pos];
            let method_part = &func_lower[sep_pos + 2..];
            // Try user-defined class method
            if let Some(class_def) = self.classes.get(class_part).cloned() {
                if let Some(method) = class_def.get_method(method_part) {
                    let method_op = method.op_array.clone();
                    let mut fn_cvs = vec![Value::Undef; method_op.cv_names.len()];
                    let mut arg_idx = 0;
                    for cap in &captured {
                        if arg_idx < fn_cvs.len() {
                            fn_cvs[arg_idx] = cap.clone();
                            arg_idx += 1;
                        }
                    }
                    if arg_idx < fn_cvs.len() {
                        fn_cvs[arg_idx] = a.clone();
                        arg_idx += 1;
                    }
                    if arg_idx < fn_cvs.len() {
                        fn_cvs[arg_idx] = b.clone();
                    }
                    if let Ok(result) = self.execute_op_array(&method_op, fn_cvs) {
                        return result.to_long();
                    }
                }
            }
        }

        // Try user functions
        if let Some(func) = self.user_functions.get(&func_lower).cloned() {
            let mut fn_cvs = vec![Value::Undef; func.cv_names.len()];
            let mut arg_idx = 0;
            for cap in &captured {
                if arg_idx < fn_cvs.len() {
                    fn_cvs[arg_idx] = cap.clone();
                    arg_idx += 1;
                }
            }
            if arg_idx < fn_cvs.len() {
                fn_cvs[arg_idx] = a.clone();
                arg_idx += 1;
            }
            if arg_idx < fn_cvs.len() {
                fn_cvs[arg_idx] = b.clone();
            }
            if let Ok(result) = self.execute_op_array(&func, fn_cvs) {
                return result.to_long();
            }
        }

        // Try builtin functions
        if let Some(builtin) = self.functions.get(&func_lower).copied() {
            if let Ok(result) = builtin(self, &[a.clone(), b.clone()]) {
                return result.to_long();
            }
        }

        0
    }

    /// Call a PHP callback function with arbitrary arguments (for filter callbacks etc)
    fn spl_call_filter_callback(&mut self, callback: &Value, call_args: &[Value]) -> Value {
        // Resolve callback
        let (func_name, captured) = match callback {
            Value::String(s) => (s.as_bytes().to_vec(), vec![]),
            Value::Array(arr) => {
                let arr = arr.borrow();
                let vals: Vec<Value> = arr.values().cloned().collect();
                if vals.len() >= 2 {
                    let first = &vals[0];
                    let method = vals[1].to_php_string();
                    match first {
                        Value::String(class_name) => {
                            let mut name = class_name.as_bytes().to_vec();
                            name.extend_from_slice(b"::");
                            name.extend_from_slice(method.as_bytes());
                            (name, vec![])
                        }
                        Value::Object(_) => {
                            let class_name = first.to_php_string();
                            let mut name = class_name.as_bytes().to_vec();
                            name.extend_from_slice(b"::");
                            name.extend_from_slice(method.as_bytes());
                            (name, vec![first.clone()])
                        }
                        _ => return Value::False,
                    }
                } else {
                    return Value::False;
                }
            }
            _ => {
                // Could be a closure (stored as string internally)
                if let Value::String(s) = callback {
                    (s.as_bytes().to_vec(), vec![])
                } else {
                    return Value::False;
                }
            }
        };

        let func_lower: Vec<u8> = func_name.iter().map(|b| b.to_ascii_lowercase()).collect();

        // Check for class::method patterns
        if let Some(sep_pos) = func_lower.windows(2).position(|w| w == b"::") {
            let class_part = &func_lower[..sep_pos];
            let method_part = &func_lower[sep_pos + 2..];
            if let Some(class_def) = self.classes.get(class_part).cloned() {
                if let Some(method) = class_def.get_method(method_part) {
                    let method_op = method.op_array.clone();
                    let mut fn_cvs = vec![Value::Undef; method_op.cv_names.len()];
                    let mut arg_idx = 0;
                    for cap in &captured {
                        if arg_idx < fn_cvs.len() { fn_cvs[arg_idx] = cap.clone(); arg_idx += 1; }
                    }
                    for arg in call_args {
                        if arg_idx < fn_cvs.len() { fn_cvs[arg_idx] = arg.clone(); arg_idx += 1; }
                    }
                    if let Ok(result) = self.execute_op_array(&method_op, fn_cvs) {
                        return result;
                    }
                }
            }
        }

        // Try user functions
        if let Some(func) = self.user_functions.get(&func_lower).cloned() {
            let mut fn_cvs = vec![Value::Undef; func.cv_names.len()];
            let mut arg_idx = 0;
            for cap in &captured {
                if arg_idx < fn_cvs.len() { fn_cvs[arg_idx] = cap.clone(); arg_idx += 1; }
            }
            for arg in call_args {
                if arg_idx < fn_cvs.len() { fn_cvs[arg_idx] = arg.clone(); arg_idx += 1; }
            }
            if let Ok(result) = self.execute_op_array(&func, fn_cvs) {
                return result;
            }
        }

        // Try builtin functions
        if let Some(builtin) = self.functions.get(&func_lower).copied() {
            if let Ok(result) = builtin(self, call_args) {
                return result;
            }
        }

        Value::False
    }

    /// Call a method on an object by looking up the class method and executing it
    pub fn call_object_method(
        &mut self,
        obj_val: &Value,
        method_name: &[u8],
        args: &[Value],
    ) -> Option<Value> {
        if let Value::Object(obj) = obj_val {
            let class_lower: Vec<u8> = obj.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
            let method_lower: Vec<u8> = method_name.iter().map(|b| b.to_ascii_lowercase()).collect();
            if let Some(class_def) = self.classes.get(&class_lower) {
                if let Some(method) = class_def.get_method(&method_lower) {
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

            // Try SPL built-in method dispatch
            // First try no-arg dispatch
            let result = self.dispatch_spl_method(&class_lower, &method_lower, obj);
            if result.is_some() {
                return result;
            }

            // Try args-based SPL dispatch
            if self.is_spl_args_method(&class_lower, &method_lower) {
                let mut spl_args = vec![obj_val.clone()];
                spl_args.extend_from_slice(args);
                let result = self.handle_spl_docall(&class_lower, &method_lower, &spl_args);
                if result.is_some() {
                    return result;
                }
            }

            // Check parent classes for SPL dispatch
            if let Some(parent) = get_builtin_parent(&class_lower) {
                let parent_lower: Vec<u8> = parent.iter().map(|b| b.to_ascii_lowercase()).collect();
                let result = self.dispatch_spl_method(&parent_lower, &method_lower, obj);
                if result.is_some() {
                    return result;
                }
                if self.is_spl_args_method(&parent_lower, &method_lower) {
                    let mut spl_args = vec![obj_val.clone()];
                    spl_args.extend_from_slice(args);
                    let result = self.handle_spl_docall(&parent_lower, &method_lower, &spl_args);
                    if result.is_some() {
                        return result;
                    }
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

    /// Get or create a singleton enum case object.
    /// Returns the cached Value::Object for the given class and case name.
    pub fn get_enum_case(&mut self, class_lower: &[u8], case_name: &[u8]) -> Option<Value> {
        // Build cache key: "class_lower::CaseName"
        let mut cache_key = class_lower.to_vec();
        cache_key.extend_from_slice(b"::");
        cache_key.extend_from_slice(case_name);

        // Return cached if exists
        if let Some(val) = self.enum_case_cache.get(&cache_key) {
            return Some(val.clone());
        }

        // Look up the class to get enum info
        let class = self.classes.get(class_lower)?.clone();
        if !class.is_enum {
            return None;
        }

        // Find the case in enum_cases
        let case_entry = class.enum_cases.iter().find(|(n, _)| n == case_name)?;
        let backing_value = case_entry.1.clone();

        // Create the enum case object
        let obj_id = self.next_object_id;
        self.next_object_id += 1;
        let mut obj = PhpObject::new(class.name.clone(), obj_id);
        // Set the name property
        obj.set_property(
            b"name".to_vec(),
            Value::String(PhpString::from_vec(case_name.to_vec())),
        );
        // Set the value property for backed enums
        if let Some(ref bt) = class.enum_backing_type {
            obj.set_property(b"value".to_vec(), backing_value);
            obj.set_property(b"__enum_backing_type".to_vec(),
                Value::String(PhpString::from_vec(bt.clone())));
        }
        // Mark as enum case with a special internal property
        obj.set_property(b"__enum_case".to_vec(), Value::True);

        let val = Value::Object(Rc::new(RefCell::new(obj)));
        self.enum_case_cache.insert(cache_key, val.clone());
        Some(val)
    }

    /// Check if a Value is an enum case object
    pub fn is_enum_case(val: &Value) -> bool {
        if let Value::Object(obj) = val {
            let obj = obj.borrow();
            obj.has_property(b"__enum_case")
        } else {
            false
        }
    }

    /// Execute an op_array (main entry point)
    pub fn execute(&mut self, op_array: &OpArray) -> Result<Value, VmError> {
        self.is_global_scope = true;
        let cvs = vec![Value::Undef; op_array.cv_names.len()];
        let result = self.execute_op_array(op_array, cvs)?;

        // Call __destruct on all tracked objects in reverse creation order
        // Skip objects whose constructor threw (marked with __ctor_pending)
        let destructibles = std::mem::take(&mut self.destructible_objects);
        for obj_val in destructibles.iter().rev() {
            if let Value::Object(obj_rc) = obj_val {
                // Skip objects whose constructor did not complete
                if matches!(obj_rc.borrow().get_property(b"__ctor_pending"), Value::True) {
                    continue;
                }
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
        // Track which foreach iterators are by-reference (stores the source array Rc)
        let mut foreach_ref_arrays: HashMap<u32, Rc<RefCell<PhpArray>>> = HashMap::new();
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
                    match self.value_to_string_checked(&val) {
                        Ok(s) => self.write_output(s.as_bytes()),
                        Err(e) => {
                            if let Some((_catch, _finally, _)) = exception_handlers.last() {
                                let catch = *_catch;
                                let finally = *_finally;
                                exception_handlers.pop();
                                if catch > 0 {
                                    ip = catch as usize;
                                } else if finally > 0 {
                                    ip = finally as usize;
                                }
                                continue;
                            }
                            return Err(e);
                        }
                    }
                }

                OpCode::Print => {
                    let val = self.read_operand_warn(&op.op1, &cvs, &tmps, &op_array.literals, op_array, op.line);
                    if matches!(val, Value::Array(_)) || matches!(&val, Value::Reference(r) if matches!(&*r.borrow(), Value::Array(_))) {
                        self.emit_warning_at("Array to string conversion", op.line);
                    }
                    match self.value_to_string_checked(&val) {
                        Ok(s) => self.write_output(s.as_bytes()),
                        Err(e) => {
                            if let Some((_catch, _finally, _)) = exception_handlers.last() {
                                let catch = *_catch;
                                let finally = *_finally;
                                exception_handlers.pop();
                                if catch > 0 {
                                    ip = catch as usize;
                                } else if finally > 0 {
                                    ip = finally as usize;
                                }
                                continue;
                            }
                            return Err(e);
                        }
                    }
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
                    if let OperandType::Cv(target_idx) = op.op1 {
                        let ti = target_idx as usize;
                        let ref_cell = match op.op2 {
                            OperandType::Cv(value_idx) => {
                                let vi = value_idx as usize;
                                // Get or create a reference cell for the value variable
                                if let Value::Reference(r) = &cvs[vi] {
                                    r.clone()
                                } else {
                                    let r = Rc::new(RefCell::new(cvs[vi].clone()));
                                    cvs[vi] = Value::Reference(r.clone());
                                    r
                                }
                            }
                            OperandType::Tmp(tmp_idx) => {
                                let tmp_i = tmp_idx as usize;
                                // For tmp sources (e.g., foreach by-ref), get the reference from tmp
                                if let Value::Reference(r) = &tmps[tmp_i] {
                                    r.clone()
                                } else {
                                    let r = Rc::new(RefCell::new(tmps[tmp_i].clone()));
                                    tmps[tmp_i] = Value::Reference(r.clone());
                                    r
                                }
                            }
                            _ => {
                                // Fallback
                                let val = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                                if let Value::Reference(r) = &val {
                                    r.clone()
                                } else {
                                    Rc::new(RefCell::new(val))
                                }
                            }
                        };
                        // Point the target to the same reference (direct replace, not write-through)
                        cvs[ti] = Value::Reference(ref_cell);
                    }
                }

                OpCode::Add => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.check_leading_numeric_warning(&a, op.line);
                    self.check_leading_numeric_warning(&b, op.line);
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
                    self.check_leading_numeric_warning(&a, op.line);
                    self.check_leading_numeric_warning(&b, op.line);
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
                    self.check_leading_numeric_warning(&a, op.line);
                    self.check_leading_numeric_warning(&b, op.line);
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
                    // Check for deprecated: pow(0, negative)
                    let base_val = a.to_double();
                    let exp_val = b.to_double();
                    if base_val == 0.0 && exp_val < 0.0 {
                        self.emit_deprecated_at("Power of base 0 and negative exponent is deprecated", op.line);
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
                    let a_str = match self.value_to_string_checked(&a) {
                        Ok(s) => s,
                        Err(e) => {
                            if let Some((_catch, _finally, _)) = exception_handlers.last() {
                                let catch = *_catch;
                                let finally = *_finally;
                                exception_handlers.pop();
                                if catch > 0 { ip = catch as usize; } else if finally > 0 { ip = finally as usize; }
                                continue;
                            }
                            return Err(e);
                        }
                    };
                    let b_str = match self.value_to_string_checked(&b) {
                        Ok(s) => s,
                        Err(e) => {
                            if let Some((_catch, _finally, _)) = exception_handlers.last() {
                                let catch = *_catch;
                                let finally = *_finally;
                                exception_handlers.pop();
                                if catch > 0 { ip = catch as usize; } else if finally > 0 { ip = finally as usize; }
                                continue;
                            }
                            return Err(e);
                        }
                    };
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
                OpCode::BoolXor => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    let result = if a.is_truthy() ^ b.is_truthy() {
                        Value::True
                    } else {
                        Value::False
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
                    self.emit_object_comparison_notice(&a, &b, op.line);
                    self.write_operand(
                        &op.result,
                        if a.equals_with_object_cast(&b) {
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
                    self.emit_object_comparison_notice(&a, &b, op.line);
                    self.write_operand(
                        &op.result,
                        if a.equals_with_object_cast(&b) {
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
                    self.emit_object_comparison_notice(&a, &b, op.line);
                    let cmp = a.compare(&b);
                    self.write_operand(
                        &op.result,
                        if cmp != i64::MIN && cmp < 0 {
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
                    self.emit_object_comparison_notice(&a, &b, op.line);
                    let cmp = a.compare(&b);
                    self.write_operand(
                        &op.result,
                        if cmp != i64::MIN && cmp <= 0 {
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
                    self.emit_object_comparison_notice(&a, &b, op.line);
                    let cmp = a.compare(&b);
                    self.write_operand(
                        &op.result,
                        if cmp != i64::MIN && cmp > 0 {
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
                    self.emit_object_comparison_notice(&a, &b, op.line);
                    let cmp = a.compare(&b);
                    self.write_operand(
                        &op.result,
                        if cmp != i64::MIN && cmp >= 0 {
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
                    // Check for TypeError on objects/arrays/resources
                    if let Some(err) = check_inc_dec_type(&val, true) {
                        let exc = self.create_exception(b"TypeError", &err, op.line);
                        self.current_exception = Some(exc);
                        if let Some((catch_target, _, _)) = exception_handlers.pop() {
                            ip = catch_target as usize;
                            continue;
                        }
                        return Err(VmError { message: format!("Uncaught TypeError: {}", err), line: op.line });
                    }
                    // Emit deprecation warnings for non-numeric string increment
                    emit_inc_dec_warnings(self, &val, true, op.line);
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
                    if let Some(err) = check_inc_dec_type(&val, false) {
                        let exc = self.create_exception(b"TypeError", &err, op.line);
                        self.current_exception = Some(exc);
                        if let Some((catch_target, _, _)) = exception_handlers.pop() {
                            ip = catch_target as usize;
                            continue;
                        }
                        return Err(VmError { message: format!("Uncaught TypeError: {}", err), line: op.line });
                    }
                    emit_inc_dec_warnings(self, &val, false, op.line);
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
                    if let Some(err) = check_inc_dec_type(&val, true) {
                        let exc = self.create_exception(b"TypeError", &err, op.line);
                        self.current_exception = Some(exc);
                        if let Some((catch_target, _, _)) = exception_handlers.pop() {
                            ip = catch_target as usize;
                            continue;
                        }
                        return Err(VmError { message: format!("Uncaught TypeError: {}", err), line: op.line });
                    }
                    emit_inc_dec_warnings(self, &val, true, op.line);
                    let new_val = php_increment(&val);
                    self.write_operand(&op.result, val, &mut cvs, &mut tmps, &static_cv_keys);
                    self.write_operand(&op.op1, new_val, &mut cvs, &mut tmps, &static_cv_keys);
                }
                OpCode::PostDecrement => {
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    if let Some(err) = check_inc_dec_type(&val, false) {
                        let exc = self.create_exception(b"TypeError", &err, op.line);
                        self.current_exception = Some(exc);
                        if let Some((catch_target, _, _)) = exception_handlers.pop() {
                            ip = catch_target as usize;
                            continue;
                        }
                        return Err(VmError { message: format!("Uncaught TypeError: {}", err), line: op.line });
                    }
                    emit_inc_dec_warnings(self, &val, false, op.line);
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
                OpCode::InitDynamicStaticCall => {
                    // Dynamic static call: $obj::method() or $class::method()
                    let class_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let method_val = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    let method_name = method_val.to_php_string();

                    // Resolve the class name from the value
                    let class_name = match &class_val {
                        Value::Object(obj) => {
                            obj.borrow().class_name.clone()
                        }
                        Value::String(s) => s.as_bytes().to_vec(),
                        _ => class_val.to_php_string().as_bytes().to_vec(),
                    };

                    let mut func_name = class_name;
                    func_name.extend_from_slice(b"::");
                    func_name.extend_from_slice(method_name.as_bytes());

                    self.pending_calls.push(PendingCall {
                        name: PhpString::from_vec(func_name),
                        args: Vec::new(),
                        named_args: Vec::new(),
                    });
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
                    } else if func_name_lower == b"datetime::createfromformat"
                        || func_name_lower == b"datetimeimmutable::createfromformat" {
                        let is_immutable = func_name_lower.starts_with(b"datetimeimmutable");
                        let format = call.args.first().cloned().unwrap_or(Value::Null).to_php_string().to_string_lossy();
                        let datetime_str = call.args.get(1).cloned().unwrap_or(Value::Null).to_php_string().to_string_lossy();
                        let now_secs = std::time::SystemTime::now()
                            .duration_since(std::time::SystemTime::UNIX_EPOCH)
                            .map(|d| d.as_secs() as i64)
                            .unwrap_or(0);
                        // Use a simple format-based parser
                        let ts = vm_parse_with_format(&format, &datetime_str, now_secs);
                        let result = match ts {
                            Some(timestamp) => {
                                let obj_id = self.next_object_id();
                                let class_name = if is_immutable { b"DateTimeImmutable".to_vec() } else { b"DateTime".to_vec() };
                                let mut obj = PhpObject::new(class_name, obj_id);
                                obj.set_property(b"__timestamp".to_vec(), Value::Long(timestamp));
                                Value::Object(Rc::new(RefCell::new(obj)))
                            }
                            None => Value::False,
                        };
                        self.write_operand(&op.result, result, &mut cvs, &mut tmps, &static_cv_keys);
                    } else if func_name_lower == b"datetime::createfromtimestamp"
                        || func_name_lower == b"datetimeimmutable::createfromtimestamp" {
                        let is_immutable = func_name_lower.starts_with(b"datetimeimmutable");
                        let ts = call.args.first().cloned().unwrap_or(Value::Null).to_long();
                        let obj_id = self.next_object_id();
                        let class_name = if is_immutable { b"DateTimeImmutable".to_vec() } else { b"DateTime".to_vec() };
                        let mut obj = PhpObject::new(class_name, obj_id);
                        obj.set_property(b"__timestamp".to_vec(), Value::Long(ts));
                        let result = Value::Object(Rc::new(RefCell::new(obj)));
                        self.write_operand(&op.result, result, &mut cvs, &mut tmps, &static_cv_keys);
                    } else if func_name_lower == b"datetime::createfromimmutable"
                        || func_name_lower == b"datetime::createfrominterface"
                        || func_name_lower == b"datetimeimmutable::createfrommutable"
                        || func_name_lower == b"datetimeimmutable::createfrominterface" {
                        let is_immutable = func_name_lower.starts_with(b"datetimeimmutable");
                        let ts = if let Some(Value::Object(o)) = call.args.first() {
                            o.borrow().get_property(b"__timestamp").to_long()
                        } else {
                            std::time::SystemTime::now().duration_since(std::time::SystemTime::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
                        };
                        let obj_id = self.next_object_id();
                        let class_name = if is_immutable { b"DateTimeImmutable".to_vec() } else { b"DateTime".to_vec() };
                        let mut obj = PhpObject::new(class_name, obj_id);
                        obj.set_property(b"__timestamp".to_vec(), Value::Long(ts));
                        let result = Value::Object(Rc::new(RefCell::new(obj)));
                        self.write_operand(&op.result, result, &mut cvs, &mut tmps, &static_cv_keys);
                    } else if func_name_lower == b"dateinterval::createfromdatestring" {
                        // DateInterval::createFromDateString - delegate to procedural function
                        if let Some(f) = self.functions.get(b"date_interval_create_from_date_string".as_ref()).copied() {
                            let result = f(self, &call.args)?;
                            self.write_operand(&op.result, result, &mut cvs, &mut tmps, &static_cv_keys);
                        } else {
                            self.write_operand(&op.result, Value::False, &mut cvs, &mut tmps, &static_cv_keys);
                        }
                    } else if func_name_lower == b"datetimezone::listidentifiers" {
                        // Return a basic list of timezone identifiers
                        let mut arr = PhpArray::new();
                        let timezones = ["UTC", "Europe/London", "Europe/Paris", "Europe/Berlin",
                            "America/New_York", "America/Chicago", "America/Denver", "America/Los_Angeles",
                            "Asia/Tokyo", "Asia/Shanghai", "Asia/Kolkata", "Australia/Sydney",
                            "Pacific/Auckland", "Africa/Cairo", "Africa/Johannesburg"];
                        for tz in &timezones {
                            arr.push(Value::String(PhpString::from_string(tz.to_string())));
                        }
                        let result = Value::Array(Rc::new(RefCell::new(arr)));
                        self.write_operand(&op.result, result, &mut cvs, &mut tmps, &static_cv_keys);
                    } else if func_name_lower == b"datetime::getlasterrors"
                        || func_name_lower == b"datetimeimmutable::getlasterrors" {
                        // Return false (no errors)
                        self.write_operand(&op.result, Value::False, &mut cvs, &mut tmps, &static_cv_keys);
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
                                // Check if the SPL method set an exception
                                if let Some(exc_val) = self.current_exception.take() {
                                    if let Some((catch_target, _, _)) = exception_handlers.last() {
                                        let catch_target = *catch_target;
                                        self.current_exception = Some(exc_val);
                                        ip = catch_target as usize;
                                        continue;
                                    } else {
                                        // No handler - propagate
                                        let msg = if let Value::Object(o) = &exc_val {
                                            let ob = o.borrow();
                                            ob.get_property(b"message").to_php_string().to_string_lossy()
                                        } else {
                                            "Unknown exception".to_string()
                                        };
                                        self.current_exception = Some(exc_val);
                                        return Err(VmError { message: msg, line: op.line });
                                    }
                                }
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
                        // Check if the SPL method set an exception
                        if let Some(exc_val) = self.current_exception.take() {
                            if let Some((catch_target, _, _)) = exception_handlers.last() {
                                let catch_target = *catch_target;
                                self.current_exception = Some(exc_val);
                                ip = catch_target as usize;
                                continue;
                            } else {
                                let msg = if let Value::Object(o) = &exc_val {
                                    let ob = o.borrow();
                                    ob.get_property(b"message").to_php_string().to_string_lossy()
                                } else {
                                    "Unknown exception".to_string()
                                };
                                self.current_exception = Some(exc_val);
                                return Err(VmError { message: msg, line: op.line });
                            }
                        }
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
                    } else if func_name_lower.contains(&b':') && {
                        // Check if this is an enum static method call (from/tryFrom/cases)
                        let sep_pos = func_name_lower.iter().position(|&b| b == b':').unwrap_or(0);
                        if sep_pos > 0 && func_name_lower.get(sep_pos + 1) == Some(&b':') {
                            let class_part = &func_name_lower[..sep_pos];
                            let method_part = &func_name_lower[sep_pos + 2..];
                            self.classes.get(class_part).map(|c| c.is_enum).unwrap_or(false)
                                && (method_part == b"from" || method_part == b"tryfrom" || method_part == b"cases")
                        } else {
                            false
                        }
                    } {
                        // Handle enum static methods: from(), tryFrom(), cases()
                        let sep_pos = func_name_lower.iter().position(|&b| b == b':').unwrap();
                        let class_part = func_name_lower[..sep_pos].to_vec();
                        let method_part = func_name_lower[sep_pos + 2..].to_vec();

                        let result = match method_part.as_slice() {
                            b"from" => {
                                let arg = call.args.first().cloned().unwrap_or(Value::Null);
                                let class = self.classes.get(&class_part).cloned();
                                if let Some(class) = class {
                                    if class.enum_backing_type.is_none() {
                                        // Unit enums don't have from()
                                        let class_name = String::from_utf8_lossy(&class.name);
                                        let msg = format!("Cannot use ::from on non-backed enum {}", class_name);
                                        let exc = self.create_exception(b"Error", &msg, op.line);
                                        self.current_exception = Some(exc);
                                        if let Some((catch_target, _, _)) = exception_handlers.pop() {
                                            ip = catch_target as usize;
                                            continue;
                                        }
                                        return Err(VmError { message: msg, line: op.line });
                                    }
                                    // Type check: for int-backed enums, only int is accepted
                                    // For string-backed enums, both string and int are accepted
                                    let bt = class.enum_backing_type.as_ref().unwrap();
                                    if bt.eq_ignore_ascii_case(b"int") && !matches!(arg, Value::Long(_)) {
                                        let class_name = String::from_utf8_lossy(&class.name);
                                        let arg_type = arg.type_name();
                                        let msg = format!("{}::from(): Argument #1 ($value) must be of type int, {} given",
                                            class_name, arg_type);
                                        let exc = self.create_exception(b"TypeError", &msg, op.line);
                                        self.current_exception = Some(exc);
                                        if let Some((catch_target, _, _)) = exception_handlers.pop() {
                                            ip = catch_target as usize;
                                            continue;
                                        }
                                        return Err(VmError { message: msg, line: op.line });
                                    }
                                    // For string-backed enums, convert arg to string for comparison
                                    let lookup_arg = if bt.eq_ignore_ascii_case(b"string") && !matches!(arg, Value::String(_)) {
                                        Value::String(arg.to_php_string())
                                    } else {
                                        arg.clone()
                                    };
                                    let mut found = None;
                                    for (case_name, case_val) in &class.enum_cases {
                                        if case_val.identical(&lookup_arg) {
                                            found = Some(case_name.clone());
                                            break;
                                        }
                                    }
                                    if let Some(case_name) = found {
                                        self.get_enum_case(&class_part, &case_name).unwrap_or(Value::Null)
                                    } else {
                                        let class_name = String::from_utf8_lossy(&class.name);
                                        let arg_str = lookup_arg.to_php_string().to_string_lossy();
                                        let msg = if bt.eq_ignore_ascii_case(b"string") {
                                            format!("\"{}\" is not a valid backing value for enum {}", arg_str, class_name)
                                        } else {
                                            format!("{} is not a valid backing value for enum {}", arg_str, class_name)
                                        };
                                        let exc = self.create_exception(b"ValueError", &msg, op.line);
                                        self.current_exception = Some(exc);
                                        if let Some((catch_target, _, _)) = exception_handlers.pop() {
                                            ip = catch_target as usize;
                                            continue;
                                        }
                                        return Err(VmError { message: msg, line: op.line });
                                    }
                                } else {
                                    Value::Null
                                }
                            }
                            b"tryfrom" => {
                                let arg = call.args.first().cloned().unwrap_or(Value::Null);
                                let class = self.classes.get(&class_part).cloned();
                                if let Some(class) = class {
                                    // Type check for int-backed enums
                                    if let Some(ref bt) = class.enum_backing_type {
                                        if bt.eq_ignore_ascii_case(b"int") && !matches!(arg, Value::Long(_)) {
                                            let class_name = String::from_utf8_lossy(&class.name);
                                            let arg_type = arg.type_name();
                                            let msg = format!("{}::tryFrom(): Argument #1 ($value) must be of type int, {} given",
                                                class_name, arg_type);
                                            let exc = self.create_exception(b"TypeError", &msg, op.line);
                                            self.current_exception = Some(exc);
                                            if let Some((catch_target, _, _)) = exception_handlers.pop() {
                                                ip = catch_target as usize;
                                                continue;
                                            }
                                            return Err(VmError { message: msg, line: op.line });
                                        }
                                    }
                                    // For string-backed enums, convert arg to string
                                    let lookup_arg = if class.enum_backing_type.as_ref()
                                        .map(|bt| bt.eq_ignore_ascii_case(b"string"))
                                        .unwrap_or(false) && !matches!(arg, Value::String(_)) {
                                        Value::String(arg.to_php_string())
                                    } else {
                                        arg.clone()
                                    };
                                    let mut found = None;
                                    for (case_name, case_val) in &class.enum_cases {
                                        if case_val.identical(&lookup_arg) {
                                            found = Some(case_name.clone());
                                            break;
                                        }
                                    }
                                    if let Some(case_name) = found {
                                        self.get_enum_case(&class_part, &case_name).unwrap_or(Value::Null)
                                    } else {
                                        Value::Null
                                    }
                                } else {
                                    Value::Null
                                }
                            }
                            b"cases" => {
                                let class = self.classes.get(&class_part).cloned();
                                if let Some(class) = class {
                                    let mut arr = PhpArray::new();
                                    for (case_name, _) in &class.enum_cases {
                                        let case_obj = self.get_enum_case(&class_part, case_name).unwrap_or(Value::Null);
                                        arr.push(case_obj);
                                    }
                                    Value::Array(Rc::new(RefCell::new(arr)))
                                } else {
                                    Value::Null
                                }
                            }
                            _ => Value::Null,
                        };
                        self.write_operand(
                            &op.result,
                            result,
                            &mut cvs,
                            &mut tmps,
                            &static_cv_keys,
                        );
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

                        // Special handling for __call/__callStatic:
                        // Pack extra args into an array for the $args parameter
                        // Must happen BEFORE type checking so the array arg passes the type check.
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
                                    // For late static binding, use the actual object class when $this is present.
                                    // Check call.args first (explicit $this), then caller's CVs (parent:: calls).
                                    let called_class = if let Some(Value::Object(obj)) = call.args.first() {
                                        // Instance method call (has $this) - use object's runtime class
                                        obj.borrow().class_name.clone()
                                    } else if let Some(Value::Object(obj)) = cvs.first() {
                                        // parent::method() from instance context - caller has $this
                                        obj.borrow().class_name.clone()
                                    } else {
                                        // Pure static call - use the class name from the call
                                        let orig_bytes = call.name.as_bytes();
                                        orig_bytes[..pos].to_vec()
                                    };
                                    self.called_class_stack.push(called_class);

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

                        // Auto-inject $this for parent:: / non-static method calls in instance context
                        // When calling ClassName::method() (e.g. parent::show()), the method expects
                        // $this as CV[0], but InitFCall doesn't pass it. Inject from current scope.
                        // Do NOT inject $this for static methods.
                        if pushed_called_class
                            && user_fn.cv_names.first().map(|n| n.as_slice()) == Some(b"this")
                            && !matches!(call.args.first(), Some(Value::Object(_)))
                        {
                            // Check if the method is static - if so, don't inject $this
                            let method_is_static = if let Some(pos) = func_name_lower.iter().position(|&b| b == b':') {
                                let class_part = &func_name_lower[..pos];
                                let method_part = &func_name_lower[pos + 2..];
                                self.classes.get(class_part)
                                    .and_then(|c| c.get_method(method_part))
                                    .map(|m| m.is_static)
                                    .unwrap_or(false)
                            } else {
                                false
                            };
                            if !method_is_static {
                                // Check if current scope has $this
                                if let Some(this_val @ Value::Object(_)) = cvs.first() {
                                    call.args.insert(0, this_val.clone());
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
                            // If this was a constructor call that succeeded,
                            // clear the __ctor_pending flag so the destructor runs at shutdown.
                            if func_name_lower.ends_with(b"::__construct") {
                                if let Some(Value::Object(obj_rc)) = call.args.first() {
                                    obj_rc.borrow_mut().properties.retain(|(k, _)| k != b"__ctor_pending");
                                }
                            }
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

                                    // SPL class constructors and Reflection class constructors
                                    match class_lower.as_slice() {
                                        b"arrayobject" | b"arrayiterator" | b"recursivearrayiterator" => {
                                            // __construct($array = [], $flags = 0, $iteratorClass = "ArrayIterator")
                                            let mut emit_deprecation = false;
                                            let class_canonical = self.builtin_canonical_name(&class_lower);
                                            if call.args.len() > 1 {
                                                if let Value::Array(_) = &call.args[1] {
                                                    obj_mut.set_property(b"__spl_array".to_vec(), call.args[1].clone());
                                                } else if let Value::Object(src) = &call.args[1] {
                                                    emit_deprecation = true;
                                                    let src = src.borrow();
                                                    // If src has __spl_array (e.g. ArrayObject), use it
                                                    let spl_inner = src.get_property(b"__spl_array");
                                                    if let Value::Array(_) = &spl_inner {
                                                        obj_mut.set_property(b"__spl_array".to_vec(), spl_inner);
                                                    } else {
                                                        // Copy properties as array
                                                        let mut arr = PhpArray::new();
                                                        for (name, val) in &src.properties {
                                                            if !name.starts_with(b"__spl_") && !name.starts_with(b"__reflection_") {
                                                                arr.set(ArrayKey::String(PhpString::from_vec(name.clone())), val.clone());
                                                            }
                                                        }
                                                        obj_mut.set_property(b"__spl_array".to_vec(), Value::Array(Rc::new(RefCell::new(arr))));
                                                    }
                                                }
                                            }
                                            if call.args.len() > 2 {
                                                obj_mut.set_property(b"__spl_flags".to_vec(), call.args[2].clone());
                                            }
                                            if emit_deprecation {
                                                drop(obj_mut);
                                                self.emit_deprecated_at(&format!(
                                                    "{}::__construct(): Using an object as a backing array for {} is deprecated, as it allows violating class constraints and invariants",
                                                    class_canonical, class_canonical
                                                ), op.line);
                                            }
                                        }
                                        b"splfixedarray" => {
                                            // __construct($size = 0)
                                            // Type-check: must be int
                                            if call.args.len() > 1 {
                                                let arg = &call.args[1];
                                                match arg {
                                                    Value::Long(_) | Value::Null | Value::Undef => {}
                                                    Value::Object(o) => {
                                                        let class = String::from_utf8_lossy(&o.borrow().class_name).to_string();
                                                        drop(obj_mut);
                                                        let msg = format!("SplFixedArray::__construct(): Argument #1 ($size) must be of type int, {} given", class);
                                                        let exc = self.throw_type_error(msg.clone());
                                                        self.current_exception = Some(exc);
                                                        if let Some((catch_target, _, _)) = exception_handlers.last() {
                                                            let ct = *catch_target;
                                                            ip = ct as usize;
                                                            continue;
                                                        }
                                                        return Err(VmError { message: msg, line: op.line });
                                                    }
                                                    Value::Array(_) => {
                                                        drop(obj_mut);
                                                        let msg = "SplFixedArray::__construct(): Argument #1 ($size) must be of type int, array given".to_string();
                                                        let exc = self.throw_type_error(msg.clone());
                                                        self.current_exception = Some(exc);
                                                        if let Some((catch_target, _, _)) = exception_handlers.last() {
                                                            let ct = *catch_target;
                                                            ip = ct as usize;
                                                            continue;
                                                        }
                                                        return Err(VmError { message: msg, line: op.line });
                                                    }
                                                    Value::String(s) => {
                                                        drop(obj_mut);
                                                        let msg = format!("SplFixedArray::__construct(): Argument #1 ($size) must be of type int, string given");
                                                        let exc = self.throw_type_error(msg.clone());
                                                        self.current_exception = Some(exc);
                                                        if let Some((catch_target, _, _)) = exception_handlers.last() {
                                                            let ct = *catch_target;
                                                            ip = ct as usize;
                                                            continue;
                                                        }
                                                        return Err(VmError { message: msg, line: op.line });
                                                    }
                                                    _ => {}
                                                }
                                            }
                                            let size = if call.args.len() > 1 { call.args[1].to_long() } else { 0 };
                                            let mut arr = PhpArray::new();
                                            for _i in 0..size {
                                                arr.push(Value::Null);
                                            }
                                            obj_mut.set_property(b"__spl_array".to_vec(), Value::Array(Rc::new(RefCell::new(arr))));
                                            obj_mut.set_property(b"__spl_size".to_vec(), Value::Long(size));
                                        }
                                        b"splheap" | b"splminheap" | b"splmaxheap" => {
                                            obj_mut.set_property(b"__spl_array".to_vec(), Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
                                        }
                                        b"spldoublylinkedlist" | b"splstack" | b"splqueue" => {
                                            if !matches!(obj_mut.get_property(b"__spl_array"), Value::Array(_)) {
                                                obj_mut.set_property(b"__spl_array".to_vec(), Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
                                            }
                                        }
                                        b"splobjectstorage" => {
                                            if !matches!(obj_mut.get_property(b"__spl_array"), Value::Array(_)) {
                                                obj_mut.set_property(b"__spl_array".to_vec(), Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
                                            }
                                        }
                                        b"iteratoriterator" | b"recursiveiteratoriterator"
                                        | b"norewinditerator" | b"infiniteiterator"
                                        | b"cachingiterator" | b"recursivecachingiterator"
                                        | b"filteriterator" | b"callbackfilteriterator"
                                        | b"recursivefilteriterator" | b"recursivecallbackfilteriterator"
                                        | b"regexiterator" | b"recursiveregexiterator"
                                        | b"parentiterator" => {
                                            // __construct(Iterator $iterator, ...)
                                            if call.args.len() > 1 {
                                                obj_mut.set_property(b"__spl_inner".to_vec(), call.args[1].clone());
                                            }
                                            // CachingIterator takes flags as second arg
                                            if call.args.len() > 2 && matches!(
                                                class_lower.as_slice(),
                                                b"cachingiterator" | b"recursivecachingiterator"
                                            ) {
                                                obj_mut.set_property(b"__spl_flags".to_vec(), call.args[2].clone());
                                            }
                                            // CallbackFilterIterator takes callback as second arg
                                            if call.args.len() > 2 && matches!(
                                                class_lower.as_slice(),
                                                b"callbackfilteriterator" | b"recursivecallbackfilteriterator"
                                            ) {
                                                obj_mut.set_property(b"__spl_callback".to_vec(), call.args[2].clone());
                                            }
                                            // RecursiveIteratorIterator mode
                                            if call.args.len() > 2 && matches!(
                                                class_lower.as_slice(),
                                                b"recursiveiteratoriterator"
                                            ) {
                                                obj_mut.set_property(b"__spl_mode".to_vec(), call.args[2].clone());
                                            }
                                        }
                                        b"limititerator" => {
                                            // __construct(Iterator $iterator, int $offset = 0, int $count = -1)
                                            if call.args.len() > 1 {
                                                obj_mut.set_property(b"__spl_inner".to_vec(), call.args[1].clone());
                                            }
                                            let offset = if call.args.len() > 2 { call.args[2].to_long() } else { 0 };
                                            let count = if call.args.len() > 3 { call.args[3].to_long() } else { -1 };
                                            obj_mut.set_property(b"__spl_offset".to_vec(), Value::Long(offset));
                                            obj_mut.set_property(b"__spl_count".to_vec(), Value::Long(count));
                                        }
                                        b"appenditerator" => {
                                            obj_mut.set_property(b"__spl_array".to_vec(), Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
                                            obj_mut.set_property(b"__spl_idx".to_vec(), Value::Long(0));
                                        }
                                        b"multipleiterator" => {
                                            let flags = if call.args.len() > 1 { call.args[1].to_long() } else { 1 };
                                            obj_mut.set_property(b"__spl_flags".to_vec(), Value::Long(flags));
                                            obj_mut.set_property(b"__spl_array".to_vec(), Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
                                        }
                                        // Reflection class constructors
                                        b"reflectionclass" | b"reflectionobject" | b"reflectionenum" => {
                                            // ReflectionClass::__construct(string|object $objectOrClass)
                                            drop(obj_mut);
                                            let ctor_handled = self.reflection_class_construct(&call.args, op.line);
                                            if !ctor_handled {
                                                if let Some((catch_target, _, _)) = exception_handlers.pop() {
                                                    ip = catch_target as usize;
                                                    continue;
                                                } else {
                                                    return Err(VmError {
                                                        message: "Uncaught ReflectionException".to_string(),
                                                        line: op.line,
                                                    });
                                                }
                                            }
                                        }
                                        b"reflectionmethod" => {
                                            drop(obj_mut);
                                            let ctor_handled = self.reflection_method_construct(&call.args, op.line);
                                            if !ctor_handled {
                                                if let Some((catch_target, _, _)) = exception_handlers.pop() {
                                                    ip = catch_target as usize;
                                                    continue;
                                                } else {
                                                    return Err(VmError {
                                                        message: "Uncaught ReflectionException".to_string(),
                                                        line: op.line,
                                                    });
                                                }
                                            }
                                        }
                                        b"reflectionfunction" => {
                                            drop(obj_mut);
                                            let ctor_handled = self.reflection_function_construct(&call.args, op.line);
                                            if !ctor_handled {
                                                if let Some((catch_target, _, _)) = exception_handlers.pop() {
                                                    ip = catch_target as usize;
                                                    continue;
                                                } else {
                                                    return Err(VmError {
                                                        message: "Uncaught ReflectionException".to_string(),
                                                        line: op.line,
                                                    });
                                                }
                                            }
                                        }
                                        b"reflectionproperty" => {
                                            drop(obj_mut);
                                            let ctor_handled = self.reflection_property_construct(&call.args, op.line);
                                            if !ctor_handled {
                                                if let Some((catch_target, _, _)) = exception_handlers.pop() {
                                                    ip = catch_target as usize;
                                                    continue;
                                                } else {
                                                    return Err(VmError {
                                                        message: "Uncaught ReflectionException".to_string(),
                                                        line: op.line,
                                                    });
                                                }
                                            }
                                        }
                                        b"reflectionparameter" => {
                                            drop(obj_mut);
                                            let ctor_handled = self.reflection_parameter_construct(&call.args, op.line);
                                            if !ctor_handled {
                                                if let Some((catch_target, _, _)) = exception_handlers.pop() {
                                                    ip = catch_target as usize;
                                                    continue;
                                                } else {
                                                    return Err(VmError {
                                                        message: "Uncaught ReflectionException".to_string(),
                                                        line: op.line,
                                                    });
                                                }
                                            }
                                        }
                                        b"reflectionextension" => {
                                            // ReflectionExtension::__construct(string $name)
                                            if call.args.len() > 1 {
                                                let ext_name = call.args[1].to_php_string();
                                                obj_mut.set_property(b"name".to_vec(), Value::String(ext_name));
                                            }
                                        }
                                        b"reflectiongenerator" => {
                                            // ReflectionGenerator::__construct(Generator $generator)
                                            if call.args.len() > 1 {
                                                obj_mut.set_property(b"__reflection_target".to_vec(), call.args[1].clone());
                                            }
                                        }
                                        b"datetime" | b"datetimeimmutable" => {
                                            // DateTime::__construct($datetime = "now", $timezone = null)
                                            let datetime_str = if call.args.len() > 1 {
                                                let arg = &call.args[1];
                                                if matches!(arg, Value::Null) {
                                                    String::new()
                                                } else {
                                                    arg.to_php_string().to_string_lossy()
                                                }
                                            } else {
                                                String::new()
                                            };
                                            let now_secs = std::time::SystemTime::now()
                                                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                                                .map(|d| d.as_secs() as i64)
                                                .unwrap_or(0);
                                            let timestamp = if datetime_str.is_empty() || datetime_str.eq_ignore_ascii_case("now") {
                                                now_secs
                                            } else {
                                                match vm_parse_datetime_string(&datetime_str, now_secs) {
                                                    Some(ts) => ts,
                                                    None => {
                                                        drop(obj_mut);
                                                        let err_msg = format!("Failed to parse time string ({}) at position 0", datetime_str);
                                                        let exc = self.create_exception(b"Exception", &err_msg, op.line);
                                                        self.current_exception = Some(exc);
                                                        if let Some((catch_target, _, _)) = exception_handlers.pop() {
                                                            ip = catch_target as usize;
                                                            continue;
                                                        } else {
                                                            return Err(VmError {
                                                                message: format!("Uncaught Exception: {}", err_msg),
                                                                line: op.line,
                                                            });
                                                        }
                                                    }
                                                }
                                            };
                                            obj_mut.set_property(b"__timestamp".to_vec(), Value::Long(timestamp));
                                            // Set display properties for var_dump
                                            let date_str = Self::format_utc_datetime(timestamp);
                                            obj_mut.set_property(b"date".to_vec(), Value::String(PhpString::from_string(date_str)));
                                            obj_mut.set_property(b"timezone_type".to_vec(), Value::Long(3));
                                            obj_mut.set_property(b"timezone".to_vec(), Value::String(PhpString::from_bytes(b"UTC")));
                                        }
                                        b"dateinterval" => {
                                            // DateInterval::__construct($duration) - ISO 8601 duration
                                            if call.args.len() > 1 {
                                                let spec = call.args[1].to_php_string().to_string_lossy();
                                                let (y, m, d, h, i, s) = parse_iso8601_duration(&spec);
                                                obj_mut.set_property(b"y".to_vec(), Value::Long(y));
                                                obj_mut.set_property(b"m".to_vec(), Value::Long(m));
                                                obj_mut.set_property(b"d".to_vec(), Value::Long(d));
                                                obj_mut.set_property(b"h".to_vec(), Value::Long(h));
                                                obj_mut.set_property(b"i".to_vec(), Value::Long(i));
                                                obj_mut.set_property(b"s".to_vec(), Value::Long(s));
                                                obj_mut.set_property(b"f".to_vec(), Value::Double(0.0));
                                                obj_mut.set_property(b"days".to_vec(), Value::False);
                                                obj_mut.set_property(b"invert".to_vec(), Value::Long(0));
                                            }
                                        }
                                        b"datetimezone" => {
                                            // DateTimeZone::__construct($timezone)
                                            if call.args.len() > 1 {
                                                let tz = call.args[1].to_php_string().to_string_lossy();
                                                obj_mut.set_property(b"timezone".to_vec(), Value::String(PhpString::from_string(tz)));
                                            } else {
                                                obj_mut.set_property(b"timezone".to_vec(), Value::String(PhpString::from_bytes(b"UTC")));
                                            }
                                        }
                                        _ => {
                                            // Only apply exception constructor logic for throwable classes
                                            let is_exc = class_lower == b"exception"
                                                || class_lower == b"error"
                                                || class_lower == b"errorexception"
                                                || is_builtin_subclass(&class_lower, b"exception")
                                                || is_builtin_subclass(&class_lower, b"error")
                                                || self.class_extends(&class_lower, b"exception")
                                                || self.class_extends(&class_lower, b"error");
                                            if is_exc && class_lower.as_slice() == b"errorexception" {
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
                                            } else if is_exc {
                                                // Capture stack trace for exceptions
                                                let trace = self.build_exception_trace();
                                                obj_mut.set_property(b"trace".to_vec(), trace);
                                                obj_mut.set_property(b"previous".to_vec(), Value::Null);
                                                // Default exception/error constructor: ($message, $code, $previous)
                                                if call.args.len() > 1 {
                                                    let msg_val = &call.args[1];
                                                    // Type-check: message must be a string
                                                    match msg_val {
                                                        Value::String(_) | Value::Null | Value::Undef => {
                                                            obj_mut.set_property(b"message".to_vec(), msg_val.clone());
                                                        }
                                                        Value::Long(_) | Value::Double(_) | Value::True | Value::False => {
                                                            // Coerce to string
                                                            obj_mut.set_property(b"message".to_vec(), Value::String(msg_val.to_php_string()));
                                                        }
                                                        _ => {
                                                            // Objects/arrays cannot be converted to string
                                                            let base_class = if class_lower == b"error"
                                                                || is_builtin_subclass(&class_lower, b"error")
                                                                || self.class_extends(&class_lower, b"error") {
                                                                "Error"
                                                            } else if class_lower == b"errorexception"
                                                                || is_builtin_subclass(&class_lower, b"errorexception") {
                                                                "ErrorException"
                                                            } else {
                                                                "Exception"
                                                            };
                                                            let given_type = Vm::value_type_name(msg_val);
                                                            drop(obj_mut);
                                                            let msg = format!("{}::__construct(): Argument #1 ($message) must be of type string, {} given", base_class, given_type);
                                                            let exc = self.create_exception(b"TypeError", &msg, op.line);
                                                            self.current_exception = Some(exc);
                                                            if let Some((catch_target, _, _)) = exception_handlers.pop() {
                                                                ip = catch_target as usize;
                                                                continue;
                                                            }
                                                            return Err(VmError { message: format!("Uncaught TypeError: {}", msg), line: op.line });
                                                        }
                                                    }
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
                                        // Check if we're in an instance context AND calling via parent::
                                        // Only parent::method() from instance method should use __call
                                        // Regular ClassName::method() and $obj::method() should use __callStatic
                                        let class_part_lower: Vec<u8> = class_part.iter().map(|b| b.to_ascii_lowercase()).collect();
                                        let is_parent_call = class_part_lower == b"parent";
                                        // In instance context: $this is an object AND calling class is in inheritance chain
                                        // Exclude traits: traits should always use __callStatic for direct static calls
                                        let calling_class_is_trait = self.classes.get(&class_lower)
                                            .map(|c| c.is_trait)
                                            .unwrap_or(false);
                                        let in_instance_context = if !calling_class_is_trait && matches!(cvs.first(), Some(Value::Object(_))) {
                                            if is_parent_call {
                                                true
                                            } else {
                                                // Check if calling class is in the inheritance chain of $this
                                                if let Some(Value::Object(obj)) = cvs.first() {
                                                    let obj_class: Vec<u8> = obj.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                                                    obj_class == class_lower || self.class_extends(&obj_class, &class_lower)
                                                } else {
                                                    false
                                                }
                                            }
                                        } else {
                                            false
                                        };
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
                            // Handle Reflection static methods
                            if !handled {
                                let func_display = call.name.to_string_lossy();
                                if let Some(sep_pos) = func_display.find("::") {
                                    let class_part = &func_display[..sep_pos];
                                    let method_part = &func_display[sep_pos + 2..];
                                    let class_lower_str = class_part.to_ascii_lowercase();
                                    let method_lower_str = method_part.to_ascii_lowercase();
                                    let result = self.reflection_static_call(&class_lower_str, &method_lower_str, &call.args, op.line);
                                    if let Some(val) = result {
                                        self.write_operand(
                                            &op.result,
                                            val,
                                            &mut cvs,
                                            &mut tmps,
                                            &static_cv_keys,
                                        );
                                        handled = true;
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
                    if matches!(&val, Value::Array(_)) || matches!(&val, Value::Reference(r) if matches!(&*r.borrow(), Value::Array(_))) {
                        self.emit_warning_at("Array to string conversion", op.line);
                    }
                    let str_val = match self.value_to_string_checked(&val) {
                        Ok(s) => s,
                        Err(e) => {
                            if let Some((_catch, _finally, _)) = exception_handlers.last() {
                                let catch = *_catch;
                                let finally = *_finally;
                                exception_handlers.pop();
                                if catch > 0 { ip = catch as usize; } else if finally > 0 { ip = finally as usize; }
                                continue;
                            }
                            return Err(e);
                        }
                    };
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
                    } else if matches!(&arr_val, Value::Null | Value::Undef | Value::False) {
                        // Auto-initialize null/undef/false to array and append
                        if matches!(&arr_val, Value::False) {
                            self.emit_deprecated_at("Automatic conversion of false to array is deprecated", op.line);
                        }
                        let mut arr = PhpArray::new();
                        arr.push(val);
                        let new_arr = Value::Array(Rc::new(RefCell::new(arr)));
                        if let OperandType::Cv(cv_idx) = &op.op1 {
                            let i = *cv_idx as usize;
                            if let Some(cv_val) = cvs.get_mut(i) {
                                match cv_val {
                                    Value::Reference(r) => {
                                        *r.borrow_mut() = new_arr;
                                    }
                                    _ => {
                                        *cv_val = new_arr;
                                    }
                                }
                            }
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
                    } else if matches!(&arr_val, Value::Null | Value::Undef | Value::False) {
                        // Auto-initialize null/undef/false to array
                        if matches!(&arr_val, Value::False) {
                            self.emit_deprecated_at("Automatic conversion of false to array is deprecated", op.line);
                        }
                        let mut arr = PhpArray::new();
                        let key = Self::value_to_array_key(key_val);
                        arr.set(key, val);
                        let new_arr = Value::Array(Rc::new(RefCell::new(arr)));
                        // Write back to the CV or reference
                        if let OperandType::Cv(cv_idx) = &op.op1 {
                            let i = *cv_idx as usize;
                            if let Some(cv_val) = cvs.get_mut(i) {
                                match cv_val {
                                    Value::Reference(r) => {
                                        *r.borrow_mut() = new_arr;
                                    }
                                    _ => {
                                        *cv_val = new_arr;
                                    }
                                }
                            }
                        }
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
                        let class_name_orig = String::from_utf8_lossy(&obj.borrow().class_name).to_string();
                        // Check if class implements ArrayAccess
                        let is_spl_array = matches!(class_lower.as_slice(),
                            b"arrayobject" | b"arrayiterator" | b"recursivearrayiterator" |
                            b"splfixedarray" | b"splobjectstorage");
                        let has_user_offset = self.classes.get(&class_lower)
                            .map(|c| c.get_method(b"offsetget").is_some())
                            .unwrap_or(false);
                        if !is_spl_array && !has_user_offset {
                            return Err(VmError {
                                message: format!("Uncaught Error: Cannot use object of type {} as array", class_name_orig),
                                line: op.line,
                            });
                        }
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
                            Value::True => "true",
                            Value::False => "false",
                            Value::Long(_) => "int",
                            Value::Double(_) => "float",
                            Value::Null | Value::Undef => "null",
                            _ => "",
                        };
                        if !type_name.is_empty() && type_name != "null" {
                            self.emit_warning_at(&format!("Trying to access array offset on {}", type_name), op.line);
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
                            match gen_borrow.resume(self) {
                                Ok(_) => {}
                                Err(e) => {
                                    drop(gen_borrow);
                                    // Propagate generator error
                                    let exc_val = self.create_exception(b"Error", &e.message, e.line);
                                    self.current_exception = Some(exc_val);
                                    return Err(e);
                                }
                            }
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

                OpCode::ForeachInitRef => {
                    let arr_val = self.read_operand_warn(&op.op1, &cvs, &tmps, &op_array.literals, &op_array, op.line);
                    let iter_idx = match op.result {
                        OperandType::Tmp(idx) => idx,
                        _ => 0,
                    };

                    // For by-ref foreach, we need to work on the original array
                    // If the source is a CV, get the actual Rc from it
                    let arr_rc = match &arr_val {
                        Value::Array(a) => a.clone(),
                        Value::Reference(r) => {
                            let inner = r.borrow();
                            match &*inner {
                                Value::Array(a) => a.clone(),
                                _ => {
                                    // Not an array - emit warning
                                    let type_name = Vm::value_type_name(&inner);
                                    self.emit_warning_at(&format!(
                                        "foreach() argument must be of type array|object, {} given",
                                        type_name
                                    ), op.line);
                                    Rc::new(RefCell::new(PhpArray::new()))
                                }
                            }
                        }
                        _ => {
                            match &arr_val {
                                Value::Object(obj) => {
                                    // For objects, convert to array of references to properties
                                    let obj_borrow = obj.borrow();
                                    let mut arr = PhpArray::new();
                                    for (name, value) in &obj_borrow.properties {
                                        arr.set(
                                            ArrayKey::String(PhpString::from_vec(name.clone())),
                                            value.clone(),
                                        );
                                    }
                                    Rc::new(RefCell::new(arr))
                                }
                                _ => {
                                    // Not an array or object - emit warning
                                    let type_name = Vm::value_type_name(&arr_val);
                                    self.emit_warning_at(&format!(
                                        "foreach() argument must be of type array|object, {} given",
                                        type_name
                                    ), op.line);
                                    Rc::new(RefCell::new(PhpArray::new()))
                                }
                            }
                        }
                    };

                    // Also make the source CV point to the same array if it was a CV
                    // (ensure modifications through &$v modify the original)
                    match &op.op1 {
                        OperandType::Cv(idx) => {
                            cvs[*idx as usize] = Value::Array(arr_rc.clone());
                        }
                        _ => {}
                    }

                    foreach_ref_arrays.insert(iter_idx, arr_rc.clone());

                    // Store as Value::Array for the iteration state
                    self.write_operand(
                        &op.result,
                        Value::Array(arr_rc.clone()),
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );

                    foreach_positions.insert(iter_idx, 0usize);
                    // Snapshot keys for stable iteration
                    let keys: Vec<ArrayKey> = arr_rc.borrow().keys().cloned().collect();
                    foreach_keys.insert(iter_idx, keys);
                }

                OpCode::ForeachNextRef => {
                    let iter_idx = match op.op1 {
                        OperandType::Tmp(idx) => idx,
                        _ => 0,
                    };
                    let pos = foreach_positions.get(&iter_idx).copied().unwrap_or(0);

                    // Re-snapshot keys each iteration for by-ref (elements may be added)
                    if let Some(arr_rc) = foreach_ref_arrays.get(&iter_idx) {
                        let keys: Vec<ArrayKey> = arr_rc.borrow().keys().cloned().collect();
                        foreach_keys.insert(iter_idx, keys);
                    }

                    let done = if let Some(keys) = foreach_keys.get(&iter_idx) {
                        if let Some(arr_rc) = foreach_ref_arrays.get(&iter_idx).cloned() {
                            // Find next valid key
                            let mut found = false;
                            let mut next_pos = pos;
                            while next_pos < keys.len() {
                                let key = &keys[next_pos];
                                let has_key = arr_rc.borrow().get(key).is_some();
                                if has_key {
                                    // Get or create a reference to this element
                                    let ref_val = {
                                        let mut arr_borrow = arr_rc.borrow_mut();
                                        let current = arr_borrow.get(key).unwrap().clone();
                                        let reference = match current {
                                            Value::Reference(r) => r,
                                            other => {
                                                let r = Rc::new(RefCell::new(other));
                                                arr_borrow.set(key.clone(), Value::Reference(r.clone()));
                                                r
                                            }
                                        };
                                        Value::Reference(reference)
                                    };

                                    self.write_operand(
                                        &op.result,
                                        ref_val,
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
                            true
                        }
                    } else {
                        true
                    };

                    if done {
                        if let OperandType::JmpTarget(target) = op.op2 {
                            ip = target as usize;
                        }
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
                            // Check for enum interfaces (UnitEnum, BackedEnum)
                            if !found {
                                if let Some(class_def) = self.classes.get(&obj_class_lower) {
                                    if class_def.is_enum {
                                        if class_lower == b"unitenum" {
                                            found = true;
                                        } else if class_lower == b"backedenum" && class_def.enum_backing_type.is_some() {
                                            found = true;
                                        }
                                    }
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
                            let raw_val_opt = class
                                .static_properties
                                .get(prop_name.as_bytes())
                                .cloned()
                                .or_else(|| class.constants.get(prop_name.as_bytes()).cloned());
                            if raw_val_opt.is_none() {
                                // Neither static property nor constant found - check parent classes
                                let mut found = false;
                                let mut parent = class.parent.clone();
                                while let Some(p) = parent {
                                    let p_lower: Vec<u8> = p.iter().map(|b| b.to_ascii_lowercase()).collect();
                                    if let Some(pc) = self.classes.get(&p_lower) {
                                        if pc.static_properties.contains_key(prop_name.as_bytes())
                                            || pc.constants.contains_key(prop_name.as_bytes()) {
                                            found = true;
                                            break;
                                        }
                                        parent = pc.parent.clone();
                                    } else {
                                        break;
                                    }
                                }
                                if !found {
                                    // Also check built-in class constants before erroring
                                    if self.get_builtin_class_constant(&class_lower, prop_name.as_bytes()).is_none() {
                                        let class_display = String::from_utf8_lossy(&resolved_class).to_string();
                                        let prop_display = prop_name.to_string_lossy();
                                        let err_msg = format!("Undefined constant {}::{}", class_display, prop_display);
                                        let exc = self.create_exception(b"Error", &err_msg, op.line);
                                        self.current_exception = Some(exc);
                                        if let Some((catch_target, _, _)) = exception_handlers.last() {
                                            ip = *catch_target as usize;
                                            continue;
                                        }
                                        return Err(VmError { message: format!("Uncaught Error: {}", err_msg), line: op.line });
                                    }
                                }
                            }
                            let raw_val = raw_val_opt.unwrap_or(Value::Null);
                            // Check if this is an enum case marker
                            if let Value::String(s) = &raw_val {
                                if s.as_bytes().starts_with(b"__enum_case__::") {
                                    // Extract the case name from the marker
                                    let case_name_bytes = s.as_bytes()[15..].to_vec(); // skip "__enum_case__::"
                                    // Check for type mismatch before creating the enum case
                                    let type_error = if let Some(class_def) = self.classes.get(&class_lower) {
                                        if let Some(ref bt) = class_def.enum_backing_type {
                                            if let Some((_, case_val)) = class_def.enum_cases.iter().find(|(n, _)| *n == case_name_bytes) {
                                                let ok = if bt.eq_ignore_ascii_case(b"int") {
                                                    matches!(case_val, Value::Long(_))
                                                } else if bt.eq_ignore_ascii_case(b"string") {
                                                    matches!(case_val, Value::String(_))
                                                } else {
                                                    true
                                                };
                                                if !ok {
                                                    Some(format!("Enum case type {} does not match enum backing type {}",
                                                        case_val.type_name(), String::from_utf8_lossy(bt)))
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
                                    };
                                    if let Some(err_msg) = type_error {
                                        let exc = self.create_exception(b"TypeError", &err_msg, op.line);
                                        self.current_exception = Some(exc);
                                        if let Some((catch_target, _, _)) = exception_handlers.pop() {
                                            ip = catch_target as usize;
                                            continue;
                                        }
                                        return Err(VmError { message: err_msg, line: op.line });
                                    }
                                    self.get_enum_case(&class_lower, &case_name_bytes)
                                        .unwrap_or(raw_val)
                                } else {
                                    raw_val
                                }
                            } else {
                                raw_val
                            }
                        } else {
                            // Check built-in class constants
                            self.get_builtin_class_constant(&class_lower, prop_name.as_bytes())
                                .unwrap_or(Value::Null)
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
                        // For namespaced constants, namespace prefix is case-insensitive
                        // Normalize: lowercase namespace prefix, keep constant name as-is
                        let normalized = if let Some(pos) = name_bytes.iter().rposition(|&b| b == b'\\') {
                            let mut norm = Vec::with_capacity(name_bytes.len());
                            for &b in &name_bytes[..pos] {
                                norm.push(b.to_ascii_lowercase());
                            }
                            norm.extend_from_slice(&name_bytes[pos..]);
                            norm
                        } else {
                            name_bytes.to_vec()
                        };
                        if let Some(v) = self.constants.get(&normalized) {
                            Some(v.clone())
                        } else {
                            // Namespace fallback: try the unqualified (global) name
                            if let Some(last_sep) = name_bytes.iter().rposition(|&b| b == b'\\') {
                                let global_name = &name_bytes[last_sep + 1..];
                                self.constants.get(global_name).cloned()
                            } else {
                                None
                            }
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
                    // Resolve to absolute path for __DIR__/__FILE__
                    let abs_path = if std::path::Path::new(path).is_absolute() {
                        path.to_string()
                    } else {
                        // Resolve relative to the directory of the current file
                        let base_dir = if let Some(pos) = self.current_file.rfind('/') {
                            &self.current_file[..pos]
                        } else {
                            "."
                        };
                        let joined = format!("{}/{}", base_dir, path);
                        // Try to canonicalize, fall back to joined path
                        std::fs::canonicalize(&joined)
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or(joined)
                    };
                    let old_file = self.current_file.clone();
                    let result = match std::fs::read(&abs_path) {
                        Ok(source) => {
                            // Compile and execute
                            let mut lexer = goro_parser::Lexer::new(&source);
                            let tokens = lexer.tokenize();
                            let mut parser = goro_parser::Parser::new(tokens);
                            match parser.parse() {
                                Ok(program) => {
                                    let mut compiler = crate::compiler::Compiler::new();
                                    compiler.source_file = abs_path.as_bytes().to_vec();
                                    self.current_file = abs_path.clone();
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
                    self.current_file = old_file;
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

                OpCode::Extract => {
                    let arr_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let flags = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals).to_long();
                    // EXTR_OVERWRITE = 0, EXTR_SKIP = 1, EXTR_PREFIX_SAME = 2, etc.
                    let mut count = 0i64;
                    if let Value::Array(arr) = &arr_val {
                        let arr_borrow = arr.borrow();
                        for (key, val) in arr_borrow.iter() {
                            if let crate::array::ArrayKey::String(key_str) = key {
                                let key_bytes = key_str.as_bytes();
                                // Find the CV index for this variable name
                                let cv_idx = op_array.cv_names.iter().position(|n| n == key_bytes);
                                if let Some(cv_idx) = cv_idx {
                                    let should_set = match flags {
                                        1 => { // EXTR_SKIP
                                            // Only set if not already defined
                                            matches!(&cvs[cv_idx], Value::Undef)
                                        }
                                        _ => true, // EXTR_OVERWRITE (default)
                                    };
                                    if should_set {
                                        cvs[cv_idx] = val.clone();
                                        count += 1;
                                    }
                                } else {
                                    // Variable doesn't exist as a CV - we can't create new CVs at runtime
                                    // Skip silently (this is a limitation)
                                }
                            }
                        }
                    }
                    self.write_operand(&op.result, Value::Long(count), &mut cvs, &mut tmps, &static_cv_keys);
                }

                OpCode::CloneObj => {
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let cloned = match &val {
                        Value::Object(obj) => {
                            let obj_borrow = obj.borrow();
                            // Check if this is an uncloneable object
                            let class_lower: Vec<u8> = obj_borrow.class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                            if class_lower == b"generator" || class_lower == b"closure" || obj_borrow.has_property(b"__enum_case") {
                                let class_name = String::from_utf8_lossy(&obj_borrow.class_name).to_string();
                                drop(obj_borrow);
                                let msg = format!("Trying to clone an uncloneable object of class {}", class_name);
                                let exc_val = self.create_exception(b"Error", &msg, op.line);
                                self.current_exception = Some(exc_val);
                                if let Some((catch_target, _, _)) = exception_handlers.pop() {
                                    ip = catch_target as usize;
                                    continue;
                                }
                                return Err(VmError { message: msg, line: op.line });
                            }
                            let clone_id = self.next_object_id;
                            self.next_object_id += 1;
                            let mut new_obj =
                                PhpObject::new(obj_borrow.class_name.clone(), clone_id);
                            // Copy all properties, deep-cloning arrays for SPL classes
                            for (name, value) in &obj_borrow.properties {
                                let cloned_value = if name.starts_with(b"__spl_") {
                                    // Deep clone SPL internal array properties
                                    match value {
                                        Value::Array(a) => {
                                            Value::Array(Rc::new(RefCell::new(a.borrow().clone())))
                                        }
                                        other => other.clone(),
                                    }
                                } else {
                                    value.clone()
                                };
                                new_obj.set_property(name.clone(), cloned_value);
                            }
                            let new_obj_val = Value::Object(Rc::new(RefCell::new(new_obj)));
                            // Call __clone() if defined
                            let class_lower: Vec<u8> = obj_borrow.class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                            drop(obj_borrow);
                            if let Some(class_def) = self.classes.get(&class_lower) {
                                if class_def.get_method(b"__clone").is_some() {
                                    self.call_object_method(&new_obj_val, b"__clone", &[]);
                                }
                            }
                            new_obj_val
                        }
                        Value::Array(arr) => {
                            // Clone array
                            let cloned_arr = arr.borrow().clone();
                            Value::Array(Rc::new(RefCell::new(cloned_arr)))
                        }
                        Value::Generator(_) => {
                            let msg = "Trying to clone an uncloneable object of class Generator".to_string();
                            let exc_val = self.create_exception(b"Error", &msg, op.line);
                            self.current_exception = Some(exc_val);
                            if let Some((catch_target, _, _)) = exception_handlers.last() {
                                ip = *catch_target as usize;
                                continue;
                            }
                            return Err(VmError {
                                message: format!("Uncaught Error: {}", msg),
                                line: op.line,
                            });
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
                        let class_lower: Vec<u8> = ob.class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                        let class_name_orig = ob.class_name.clone();
                        // Check if property exists AND is accessible from current scope
                        let prop_accessible = if ob.has_property(prop_name.as_bytes()) {
                            // Check visibility
                            let current_scope = self.class_scope_stack.last().cloned().unwrap_or_default();
                            let prop_visibility = self.classes.get(&class_lower)
                                .and_then(|c| c.properties.iter().find(|p| p.name == prop_name.as_bytes()))
                                .map(|p| p.visibility);
                            match prop_visibility {
                                Some(crate::object::Visibility::Private) => {
                                    // Private: only accessible from the declaring class
                                    let declaring = self.classes.get(&class_lower)
                                        .and_then(|c| c.properties.iter().find(|p| p.name == prop_name.as_bytes()))
                                        .map(|p| p.declaring_class.clone())
                                        .unwrap_or_else(|| class_lower.clone());
                                    current_scope == declaring
                                }
                                Some(crate::object::Visibility::Protected) => {
                                    // Protected: accessible from same class or subclass
                                    if current_scope.is_empty() {
                                        false
                                    } else {
                                        current_scope == class_lower || self.class_extends(&current_scope, &class_lower) || self.class_extends(&class_lower, &current_scope)
                                    }
                                }
                                _ => true, // Public or no visibility info (dynamic property)
                            }
                        } else {
                            false
                        };
                        if prop_accessible {
                            let val = ob.get_property(prop_name.as_bytes());
                            drop(ob);
                            if matches!(val, Value::Null) { Value::False } else { Value::True }
                        } else {
                            drop(ob);
                            // Try __isset magic
                            let has_isset = self.classes.get(&class_lower)
                                .map(|c| c.methods.contains_key(&b"__isset".to_vec()))
                                .unwrap_or(false);
                            if has_isset && self.magic_depth < 5 {
                                self.magic_depth += 1;
                                let magic_method_def = self.classes.get(&class_lower).unwrap().get_method(b"__isset").unwrap();
                                let method = magic_method_def.op_array.clone();
                                let magic_declaring = magic_method_def.declaring_class.clone();
                                let mut fn_cvs = vec![Value::Undef; method.cv_names.len()];
                                if !fn_cvs.is_empty() { fn_cvs[0] = obj_val.clone(); }
                                if fn_cvs.len() > 1 { fn_cvs[1] = Value::String(prop_name.clone()); }
                                self.class_scope_stack.push(magic_declaring);
                                self.called_class_stack.push(class_name_orig);
                                let isset_result = self.execute_op_array(&method, fn_cvs).unwrap_or(Value::False);
                                self.called_class_stack.pop();
                                self.class_scope_stack.pop();
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

                OpCode::UnsetCv => {
                    // Directly replace the CV slot with Undef (breaks reference links)
                    if let OperandType::Cv(idx) = op.op1 {
                        if let Some(slot) = cvs.get_mut(idx as usize) {
                            *slot = Value::Undef;
                        }
                    }
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
                            let class_name_orig = obj.borrow().class_name.clone();
                            let has_unset = self.classes.get(&class_lower)
                                .map(|c| c.methods.contains_key(&b"__unset".to_vec()))
                                .unwrap_or(false);
                            if has_unset && self.magic_depth < 5 {
                                self.magic_depth += 1;
                                let magic_method_def = self.classes.get(&class_lower).unwrap().get_method(b"__unset").unwrap();
                                let method = magic_method_def.op_array.clone();
                                let magic_declaring = magic_method_def.declaring_class.clone();
                                let mut fn_cvs = vec![Value::Undef; method.cv_names.len()];
                                if !fn_cvs.is_empty() { fn_cvs[0] = obj_val.clone(); }
                                if fn_cvs.len() > 1 { fn_cvs[1] = Value::String(prop_name.clone()); }
                                self.class_scope_stack.push(magic_declaring);
                                self.called_class_stack.push(class_name_orig);
                                let _ = self.execute_op_array(&method, fn_cvs);
                                self.called_class_stack.pop();
                                self.class_scope_stack.pop();
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

                OpCode::ArrayIsset => {
                    // isset($arr[$key]) - for arrays, check if key exists and value is not null
                    // For objects implementing ArrayAccess, call offsetExists()
                    let arr_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let key_val = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    let result = if let Value::Array(arr) = &arr_val {
                        let key = Self::value_to_array_key(key_val);
                        match arr.borrow().get(&key) {
                            Some(v) if !matches!(v, Value::Null | Value::Undef) => Value::True,
                            _ => Value::False,
                        }
                    } else if let Value::Object(obj) = &arr_val {
                        // ArrayAccess: isset($obj[$key]) -> offsetExists($key)
                        let class_lower: Vec<u8> = obj.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                        let is_spl_array = matches!(class_lower.as_slice(),
                            b"arrayobject" | b"arrayiterator" | b"recursivearrayiterator" |
                            b"splfixedarray" | b"splobjectstorage");
                        let has_user_offset = self.classes.get(&class_lower)
                            .map(|c| c.get_method(b"offsetexists").is_some())
                            .unwrap_or(false);
                        if is_spl_array || has_user_offset {
                            let args = vec![arr_val.clone(), key_val.clone()];
                            let exists_result = self.handle_spl_docall(&class_lower, b"offsetexists", &args)
                                .unwrap_or_else(|| {
                                    self.call_object_method(&arr_val, b"offsetexists", &[key_val])
                                        .unwrap_or(Value::False)
                                });
                            if exists_result.is_truthy() { Value::True } else { Value::False }
                        } else {
                            Value::False
                        }
                    } else if let Value::String(s) = &arr_val {
                        // String offset: isset($str[$idx])
                        let idx = key_val.to_long();
                        let len = s.as_bytes().len() as i64;
                        if idx >= 0 && idx < len {
                            Value::True
                        } else if idx < 0 && (-idx) <= len {
                            Value::True
                        } else {
                            Value::False
                        }
                    } else {
                        Value::False
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
                            if obj_borrow.has_property(b"__enum_case") {
                                // For enums, show ClassName::CaseName
                                if let Value::String(case_name) = obj_borrow.get_property(b"name") {
                                    format!("{}::{}", name, case_name.to_string_lossy())
                                } else {
                                    format!("of type {}", name)
                                }
                            } else {
                                format!("of type {}", name)
                            }
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
                            // Check for extending reserved internal classes
                            if parent_lower == b"generator" || parent_lower == b"closure" {
                                let child_display = String::from_utf8_lossy(&class.name).to_string();
                                let parent_display = String::from_utf8_lossy(parent_name).to_string();
                                return Err(VmError {
                                    message: format!("Class {} cannot extend final class {}", child_display, parent_display),
                                    line: op.line,
                                });
                            }
                            if let Some(parent) = self.classes.get(&parent_lower).cloned() {
                                // Check: interface can only extend another interface
                                if class.is_interface && !parent.is_interface {
                                    let child_display = String::from_utf8_lossy(&class.name).to_string();
                                    let parent_display = String::from_utf8_lossy(parent_name).to_string();
                                    return Err(VmError {
                                        message: format!("{} cannot implement {} - it is not an interface", child_display, parent_display),
                                        line: op.line,
                                    });
                                }
                                // Check: class cannot extend an interface
                                if !class.is_interface && parent.is_interface {
                                    let child_display = String::from_utf8_lossy(&class.name).to_string();
                                    let parent_display = String::from_utf8_lossy(parent_name).to_string();
                                    return Err(VmError {
                                        message: format!("Class {} cannot extend interface {}", child_display, parent_display),
                                        line: op.line,
                                    });
                                }
                                // Check if parent is final
                                if parent.is_final {
                                    let parent_display = String::from_utf8_lossy(parent_name).to_string();
                                    let child_display = String::from_utf8_lossy(&name_val.to_php_string().as_bytes()).to_string();
                                    return Err(VmError {
                                        message: format!("Class {} cannot extend final class {}", child_display, parent_display),
                                        line: op.line,
                                    });
                                }
                                // Check if parent is an enum (enums cannot be extended)
                                if parent.is_enum {
                                    let parent_display = String::from_utf8_lossy(parent_name).to_string();
                                    let child_display = String::from_utf8_lossy(&name_val.to_php_string().as_bytes()).to_string();
                                    return Err(VmError {
                                        message: format!("Class {} cannot extend enum {}", child_display, parent_display),
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
                                        if mn_lower != b"__construct" && !method.is_abstract
                                            && method.visibility != Visibility::Private
                                            && child_method.visibility != Visibility::Private {
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
                                // Check that we're implementing an interface, not a class
                                if !iface.is_interface && !class.is_interface {
                                    let iface_display = String::from_utf8_lossy(iface_name).to_string();
                                    let class_display = String::from_utf8_lossy(&class.name).to_string();
                                    return Err(VmError {
                                        message: format!("{} cannot implement {} - it is not an interface", class_display, iface_display),
                                        line: op.line,
                                    });
                                }
                                // Check for duplicate interface implementation (interface extends)
                                if class.is_interface {
                                    // Check if parent interfaces already include this one
                                    let already_inherited = class.interfaces.iter()
                                        .filter(|n| n.iter().map(|b| b.to_ascii_lowercase()).collect::<Vec<_>>() != iface_lower)
                                        .any(|other_iface| {
                                            let other_lower: Vec<u8> = other_iface.iter().map(|b| b.to_ascii_lowercase()).collect();
                                            if let Some(other) = self.classes.get(&other_lower) {
                                                other.interfaces.iter().any(|inherited| {
                                                    inherited.iter().map(|b| b.to_ascii_lowercase()).collect::<Vec<_>>() == iface_lower
                                                })
                                            } else {
                                                false
                                            }
                                        });
                                    if already_inherited {
                                        let iface_display = String::from_utf8_lossy(iface_name).to_string();
                                        let class_display = String::from_utf8_lossy(&class.name).to_string();
                                        return Err(VmError {
                                            message: format!("Interface {} cannot implement previously implemented interface {}", class_display, iface_display),
                                            line: op.line,
                                        });
                                    }
                                }
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
                                // Inherit interface's parent interfaces
                                for parent_iface in &iface.interfaces {
                                    let pi_lower: Vec<u8> = parent_iface.iter().map(|b| b.to_ascii_lowercase()).collect();
                                    if let Some(pi) = self.classes.get(&pi_lower).cloned() {
                                        for (method_name, method) in &pi.methods {
                                            if !class.methods.contains_key(method_name) {
                                                class.methods.insert(method_name.clone(), method.clone());
                                            }
                                        }
                                        for (const_name, const_val) in &pi.constants {
                                            if !class.constants.contains_key(const_name) {
                                                class.constants.insert(const_name.clone(), const_val.clone());
                                            }
                                        }
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
                                        // Patch __CLASS__ references in literals (only marked ones)
                                        let trait_name_original = trait_def.name.clone();
                                        for &lit_idx in &m.op_array.class_const_literals.clone() {
                                            if let Some(Value::String(s)) = m.op_array.literals.get(lit_idx as usize) {
                                                if s.as_bytes() == trait_name_original.as_slice() {
                                                    m.op_array.literals[lit_idx as usize] = Value::String(PhpString::from_vec(class.name.clone()));
                                                }
                                            }
                                        }
                                        class.methods.insert(method_name.clone(), m);
                                    }
                                }
                                // Copy trait properties (class's own properties take precedence)
                                let child_prop_names: Vec<Vec<u8>> =
                                    class.properties.iter().map(|p| p.name.clone()).collect();
                                let trait_name_original = trait_def.name.clone();
                                for prop in &trait_def.properties {
                                    if !child_prop_names.contains(&prop.name) {
                                        let mut p = prop.clone();
                                        p.declaring_class = class_name_lower.clone();
                                        // Patch __CLASS__ in property defaults
                                        if let Value::String(s) = &p.default {
                                            if s.as_bytes() == trait_name_original.as_slice() {
                                                p.default = Value::String(PhpString::from_vec(class.name.clone()));
                                            }
                                        }
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
                                        // Patch __CLASS__ in static property defaults
                                        let patched_val = if let Value::String(s) = prop_val {
                                            if s.as_bytes() == trait_name_original.as_slice() {
                                                Value::String(PhpString::from_vec(class.name.clone()))
                                            } else {
                                                prop_val.clone()
                                            }
                                        } else {
                                            prop_val.clone()
                                        };
                                        class
                                            .static_properties
                                            .insert(prop_name.clone(), patched_val);
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
                            let _class_name_lower_for_check: Vec<u8> = class.name.iter().map(|b| b.to_ascii_lowercase()).collect();
                            for (_, method) in &class.methods {
                                if method.is_abstract {
                                    // Skip abstract methods inherited from traits
                                    // (non-abstract classes are allowed to use traits with abstract methods)
                                    let declaring_is_trait = self.classes.get(&method.declaring_class)
                                        .map(|c| c.is_trait)
                                        .unwrap_or(false);
                                    if declaring_is_trait {
                                        continue;
                                    }
                                    // Also check if the method came from a trait by looking at used trait names
                                    let method_from_trait = trait_names.iter().any(|tn| {
                                        let tn_lower: Vec<u8> = tn.iter().map(|b| b.to_ascii_lowercase()).collect();
                                        if let Some(tc) = self.classes.get(&tn_lower) {
                                            let method_lower: Vec<u8> = method.name.iter().map(|b| b.to_ascii_lowercase()).collect();
                                            tc.methods.contains_key(&method_lower)
                                        } else {
                                            false
                                        }
                                    });
                                    if method_from_trait {
                                        continue;
                                    }
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
                                        // Use the declaring class of the abstract method
                                        let declaring = &method.declaring_class;
                                        if let Some(dc) = self.classes.get(declaring) {
                                            iface_origin = String::from_utf8_lossy(&dc.name).to_string();
                                        } else {
                                            iface_origin = String::from_utf8_lossy(
                                                &name_val.to_php_string().as_bytes(),
                                            )
                                            .to_string();
                                        }
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
                                let kind = if class.is_enum { "Enum" } else { "Class" };
                                let msg = if class.is_enum {
                                    // Enums use a different message format
                                    format!(
                                        "Enum {} must implement {} abstract {} ({})",
                                        class_name_str, count, method_word, methods_list
                                    )
                                } else if !self_declared_abstract.is_empty() && self_declared_abstract.len() == count {
                                    // All abstract methods are self-declared
                                    format!(
                                        "{} {} declares abstract method {}() and must therefore be declared abstract",
                                        kind, class_name_str, self_declared_abstract[0]
                                    )
                                } else {
                                    format!(
                                        "{} {} contains {} abstract {} and must therefore be declared abstract or implement the remaining {} ({})",
                                        kind, class_name_str, count, method_word, method_word, methods_list
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
                                            // If the resolved value is itself an enum case marker, resolve it
                                            if let Value::String(s2) = &val {
                                                if s2.as_bytes().starts_with(b"__enum_case__::") {
                                                    let case_name = &s2.as_bytes()[15..];
                                                    if let Some(enum_obj) = self.get_enum_case(&ref_class_lower, case_name) {
                                                        class.constants.insert(const_name, enum_obj);
                                                    } else {
                                                        class.constants.insert(const_name, val);
                                                    }
                                                } else {
                                                    class.constants.insert(const_name, val);
                                                }
                                            } else {
                                                class.constants.insert(const_name, val);
                                            }
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
                                            // If the resolved value is an enum case marker, resolve it to an actual enum object
                                            if let Value::String(s2) = &val {
                                                if s2.as_bytes().starts_with(b"__enum_case__::") {
                                                    let case_name = &s2.as_bytes()[15..];
                                                    if let Some(enum_obj) = self.get_enum_case(&ref_class_lower, case_name) {
                                                        prop.default = enum_obj;
                                                    } else {
                                                        prop.default = val;
                                                    }
                                                } else {
                                                    prop.default = val;
                                                }
                                            } else {
                                                prop.default = val;
                                            }
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
                                            // If the resolved value is an enum case marker, resolve it
                                            if let Value::String(s2) = &val {
                                                if s2.as_bytes().starts_with(b"__enum_case__::") {
                                                    let case_name = &s2.as_bytes()[15..];
                                                    if let Some(enum_obj) = self.get_enum_case(&ref_class_lower, case_name) {
                                                        class.static_properties.insert(prop_name, enum_obj);
                                                    } else {
                                                        class.static_properties.insert(prop_name, val);
                                                    }
                                                } else {
                                                    class.static_properties.insert(prop_name, val);
                                                }
                                            } else {
                                                class.static_properties.insert(prop_name, val);
                                            }
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
                            // static:: uses late static binding (called class)
                            self.resolve_static_class(class_name_raw.as_bytes())
                                .to_vec()
                        } else if class_name_raw.as_bytes().eq_ignore_ascii_case(b"self") {
                            // self:: uses the lexically defining class (class scope)
                            if let Some(scope) = self.class_scope_stack.last() {
                                // Resolve to original case from class table
                                if let Some(class_entry) = self.classes.get(scope) {
                                    class_entry.name.clone()
                                } else {
                                    scope.clone()
                                }
                            } else if let Some(called) = self.called_class_stack.last() {
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

                    // Check for reserved internal classes
                    if name_lower == b"generator" || name_lower == b"closure" {
                        let err_msg = format!(
                            "The \"{}\" class is reserved for internal use and cannot be manually instantiated",
                            class_name.to_string_lossy()
                        );
                        let exc = self.create_exception(b"Error", &err_msg, op.line);
                        self.current_exception = Some(exc);
                        if let Some((catch_target, _, _)) = exception_handlers.last() {
                            ip = *catch_target as usize;
                            continue;
                        }
                        return Err(VmError { message: format!("Uncaught Error: {}", err_msg), line: op.line });
                    }
                    // Check for abstract class, interface, or enum
                    if let Some(class) = self.classes.get(&name_lower) {
                        if class.is_abstract || class.is_interface || class.is_enum {
                            // Create an Error object and throw it
                            let err_msg = if class.is_interface {
                                format!(
                                    "Cannot instantiate interface {}",
                                    class_name.to_string_lossy()
                                )
                            } else if class.is_enum {
                                format!(
                                    "Cannot instantiate enum {}",
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
                            // DateTime classes
                            b"datetime" => b"DateTime".to_vec(),
                            b"datetimeimmutable" => b"DateTimeImmutable".to_vec(),
                            b"dateinterval" => b"DateInterval".to_vec(),
                            b"datetimezone" => b"DateTimeZone".to_vec(),
                            b"lengthexception" => b"LengthException".to_vec(),
                            b"outofrangeexception" => b"OutOfRangeException".to_vec(),
                            b"outofboundsexception" => b"OutOfBoundsException".to_vec(),
                            b"invalidargumentexception" => b"InvalidArgumentException".to_vec(),
                            // Reflection classes
                            b"reflectionclass" => b"ReflectionClass".to_vec(),
                            b"reflectionobject" => b"ReflectionObject".to_vec(),
                            b"reflectionmethod" => b"ReflectionMethod".to_vec(),
                            b"reflectionfunction" => b"ReflectionFunction".to_vec(),
                            b"reflectionproperty" => b"ReflectionProperty".to_vec(),
                            b"reflectionparameter" => b"ReflectionParameter".to_vec(),
                            b"reflectionextension" => b"ReflectionExtension".to_vec(),
                            b"reflectionexception" => b"ReflectionException".to_vec(),
                            b"reflectionnamedtype" => b"ReflectionNamedType".to_vec(),
                            b"reflectionuniontype" => b"ReflectionUnionType".to_vec(),
                            b"reflectionintersectiontype" => b"ReflectionIntersectionType".to_vec(),
                            b"reflectionenum" => b"ReflectionEnum".to_vec(),
                            b"reflectionenumunitcase" => b"ReflectionEnumUnitCase".to_vec(),
                            b"reflectionenumbackedcase" => b"ReflectionEnumBackedCase".to_vec(),
                            b"reflectionclassconstant" => b"ReflectionClassConstant".to_vec(),
                            b"reflectiongenerator" => b"ReflectionGenerator".to_vec(),
                            b"reflectionfiber" => b"ReflectionFiber".to_vec(),
                            b"reflectionattribute" => b"ReflectionAttribute".to_vec(),
                            b"reflectionfunctionabstract" => b"ReflectionFunctionAbstract".to_vec(),
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

                    // Initialize SPL class internal arrays at creation time
                    match name_lower.as_slice() {
                        b"arrayobject" | b"arrayiterator" | b"recursivearrayiterator" => {
                            if !matches!(obj.get_property(b"__spl_array"), Value::Array(_)) {
                                obj.set_property(b"__spl_array".to_vec(), Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
                            }
                        }
                        b"spldoublylinkedlist" | b"splstack" | b"splqueue" => {
                            if !matches!(obj.get_property(b"__spl_array"), Value::Array(_)) {
                                obj.set_property(b"__spl_array".to_vec(), Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
                            }
                            // SplStack defaults to LIFO mode (IT_MODE_LIFO = 2)
                            if name_lower == b"splstack" {
                                obj.set_property(b"__spl_iter_mode".to_vec(), Value::Long(6)); // IT_MODE_LIFO | IT_MODE_DELETE
                            }
                        }
                        b"splobjectstorage" => {
                            if !matches!(obj.get_property(b"__spl_array"), Value::Array(_)) {
                                obj.set_property(b"__spl_array".to_vec(), Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
                                obj.set_property(b"__spl_objects".to_vec(), Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
                            }
                        }
                        b"splheap" | b"splminheap" | b"splmaxheap" => {
                            if !matches!(obj.get_property(b"__spl_array"), Value::Array(_)) {
                                obj.set_property(b"__spl_array".to_vec(), Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
                            }
                        }
                        b"splpriorityqueue" => {
                            if !matches!(obj.get_property(b"__spl_array"), Value::Array(_)) {
                                obj.set_property(b"__spl_array".to_vec(), Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
                            }
                        }
                        b"appenditerator" => {
                            obj.set_property(b"__spl_array".to_vec(), Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
                            obj.set_property(b"__spl_idx".to_vec(), Value::Long(0));
                        }
                        b"multipleiterator" => {
                            obj.set_property(b"__spl_array".to_vec(), Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
                            obj.set_property(b"__spl_flags".to_vec(), Value::Long(1));
                        }
                        _ => {}
                    }
                    // Also check parent classes for SPL initialization
                    if let Some(parent) = get_builtin_parent(&name_lower) {
                        let parent_lower: Vec<u8> = parent.iter().map(|b| b.to_ascii_lowercase()).collect();
                        match parent_lower.as_slice() {
                            b"spldoublylinkedlist" => {
                                if !matches!(obj.get_property(b"__spl_array"), Value::Array(_)) {
                                    obj.set_property(b"__spl_array".to_vec(), Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
                                }
                            }
                            b"splheap" => {
                                if !matches!(obj.get_property(b"__spl_array"), Value::Array(_)) {
                                    obj.set_property(b"__spl_array".to_vec(), Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
                                }
                            }
                            _ => {}
                        }
                    }

                    let obj_value = Value::Object(Rc::new(RefCell::new(obj)));

                    // Track objects with __destruct for shutdown-time destruction
                    // Note: we track the object here, but mark it with __ctor_pending.
                    // After the constructor completes (InitMethodCall + DoFCall),
                    // the DoFCall handler for __construct removes this flag.
                    // At shutdown, only objects without __ctor_pending get destructed.
                    let has_destruct = self
                        .classes
                        .get(&name_lower)
                        .map(|c| c.methods.contains_key(&b"__destruct".to_vec()))
                        .unwrap_or(false);
                    if has_destruct {
                        // Check if there's a constructor - if so, mark as pending
                        let has_ctor = self
                            .classes
                            .get(&name_lower)
                            .map(|c| c.methods.contains_key(&b"__construct".to_vec()))
                            .unwrap_or(false);
                        if has_ctor {
                            if let Value::Object(ref rc) = obj_value {
                                rc.borrow_mut().set_property(b"__ctor_pending".to_vec(), Value::True);
                            }
                        }
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
                        let caller_scope_for_get = self.current_class_scope()
                            .map(|s| s.iter().map(|b| b.to_ascii_lowercase()).collect::<Vec<u8>>());
                        if let Some((vis, declaring_class, _is_readonly, _prop_type)) = self.find_property_def_for_scope(&class_lower, prop_name.as_bytes(), caller_scope_for_get.as_deref()) {
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
                                    // Emit "Undefined property" warning, but only for user-defined classes
                                    // that have declared properties in the class table.
                                    // Internal classes (Reflection*, Exception, etc.) use dynamic properties.
                                    let has_declared_props = self.classes.get(&class_lower)
                                        .map(|c| !c.properties.is_empty() || !c.methods.is_empty())
                                        .unwrap_or(false);
                                    if has_declared_props {
                                        let class_display = String::from_utf8_lossy(&class_name_orig);
                                        let prop_display = prop_name.to_string_lossy();
                                        self.emit_warning_at(&format!(
                                            "Undefined property: {}::${}",
                                            class_display, prop_display
                                        ), op.line);
                                    }
                                    Value::Null
                                }
                            } else {
                                prop
                            }
                        }
                    } else {
                        // Accessing property on non-object
                        match &obj_val {
                            Value::Null | Value::False => {
                                let type_name = Self::value_type_name(&obj_val);
                                let prop_str = prop_name.to_string_lossy();
                                self.emit_warning_at(
                                    &format!("Attempt to read property \"{}\" on {}", prop_str, type_name),
                                    op.line,
                                );
                            }
                            Value::True | Value::Long(_) | Value::Double(_) | Value::String(_) => {
                                let type_name = Self::value_type_name(&obj_val);
                                let prop_str = prop_name.to_string_lossy();
                                self.emit_warning_at(
                                    &format!("Attempt to read property \"{}\" on {}", prop_str, type_name),
                                    op.line,
                                );
                            }
                            _ => {} // Undef, Array, etc. - no warning
                        }
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

                        // Enums cannot have properties set
                        if obj.borrow().has_property(b"__enum_case") {
                            let class_name = String::from_utf8_lossy(&class_name_orig).to_string();
                            let prop_str = String::from_utf8_lossy(prop_name.as_bytes()).to_string();
                            let msg = format!("Cannot create dynamic property {}::${}", class_name, prop_str);
                            let exc = self.create_exception(b"Error", &msg, op.line);
                            self.current_exception = Some(exc);
                            if let Some((catch_target, _, _)) = exception_handlers.pop() {
                                ip = catch_target as usize;
                                continue;
                            }
                            return Err(VmError { message: msg, line: op.line });
                        }

                        // Check visibility, readonly, and type before setting the property
                        let mut visibility_error: Option<String> = None;
                        let mut readonly_error: Option<String> = None;
                        let mut type_error: Option<String> = None;
                        let caller_scope_for_prop = self.current_class_scope()
                            .map(|s| s.iter().map(|b| b.to_ascii_lowercase()).collect::<Vec<u8>>());
                        if let Some((vis, declaring_class, prop_is_readonly, prop_type)) = self.find_property_def_for_scope(&class_lower, prop_name.as_bytes(), caller_scope_for_prop.as_deref()) {
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
                            // Enforce typed properties (only if no readonly error, since readonly takes precedence)
                            if readonly_error.is_none() && visibility_error.is_none() {
                                if let Some(ref pt) = prop_type {
                                    if !self.value_matches_type(&value, pt) {
                                        let class_display = String::from_utf8_lossy(&class_name_orig).to_string();
                                        let prop_display = String::from_utf8_lossy(prop_name.as_bytes()).to_string();
                                        let expected = self.param_type_name(pt);
                                        let given = Self::value_type_name(&value);
                                        type_error = Some(format!(
                                            "Cannot assign {} to property {}::${} of type {}",
                                            given, class_display, prop_display, expected
                                        ));
                                    }
                                }
                            }
                        }

                        if let Some(err_msg) = type_error {
                            // Property type violation - throw TypeError
                            let exc = self.create_exception(b"TypeError", &err_msg, op.line);
                            self.current_exception = Some(exc);
                            if let Some((catch_target, _, _)) = exception_handlers.pop() {
                                ip = catch_target as usize;
                                continue;
                            } else {
                                return Err(VmError {
                                    message: format!("Uncaught TypeError: {}", err_msg),
                                    line: op.line,
                                });
                            }
                        } else if let Some(err_msg) = readonly_error {
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
                    } else if matches!(&obj_val, Value::Null | Value::Undef | Value::False) {
                        // Attempt to assign property on null/false
                        let type_name = Self::value_type_name(&obj_val);
                        let prop_str = prop_name.to_string_lossy();
                        let msg = format!("Attempt to assign property \"{}\" on {}", prop_str, type_name);
                        let exc = self.create_exception(b"Error", &msg, op.line);
                        self.current_exception = Some(exc);
                        if let Some((catch_target, _, _)) = exception_handlers.pop() {
                            ip = catch_target as usize;
                            continue;
                        }
                        return Err(VmError { message: format!("Uncaught Error: {}", msg), line: op.line });
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
                            // SPL class method dispatch - try the class itself and parent chain
                            let mut spl_result = self.dispatch_spl_method(
                                &class_name_lower,
                                &method_name_lower,
                                obj,
                            );
                            // If not found, try parent classes
                            if spl_result.is_none() {
                                let mut check_class = class_name_lower.clone();
                                for _ in 0..10 {
                                    if let Some(parent) = get_builtin_parent(&check_class) {
                                        let parent_lower: Vec<u8> = parent.iter().map(|b| b.to_ascii_lowercase()).collect();
                                        spl_result = self.dispatch_spl_method(&parent_lower, &method_name_lower, obj);
                                        if spl_result.is_some() { break; }
                                        check_class = parent_lower;
                                    } else if let Some(ce) = self.classes.get(&check_class) {
                                        if let Some(ref p) = ce.parent {
                                            let parent_lower: Vec<u8> = p.iter().map(|b| b.to_ascii_lowercase()).collect();
                                            spl_result = self.dispatch_spl_method(&parent_lower, &method_name_lower, obj);
                                            if spl_result.is_some() { break; }
                                            check_class = parent_lower;
                                        } else {
                                            break;
                                        }
                                    } else {
                                        break;
                                    }
                                }
                            }
                            spl_result
                        } else {
                            None
                        };

                        if let Some(result) = builtin_result {
                            self.pending_calls.push(PendingCall {
                                name: PhpString::from_bytes(b"__builtin_return"),
                                args: vec![result],
                                named_args: Vec::new(),
                            });
                        } else if !has_user_method && {
                            // Check SPL args method for class and parent chain
                            let mut is_spl = self.is_spl_args_method(&class_name_lower, &method_name_lower);
                            if !is_spl {
                                let mut check_class = class_name_lower.clone();
                                for _ in 0..10 {
                                    if let Some(parent) = get_builtin_parent(&check_class) {
                                        let parent_lower: Vec<u8> = parent.iter().map(|b| b.to_ascii_lowercase()).collect();
                                        if self.is_spl_args_method(&parent_lower, &method_name_lower) {
                                            is_spl = true;
                                            break;
                                        }
                                        check_class = parent_lower;
                                    } else if let Some(ce) = self.classes.get(&check_class) {
                                        if let Some(ref p) = ce.parent {
                                            let parent_lower: Vec<u8> = p.iter().map(|b| b.to_ascii_lowercase()).collect();
                                            if self.is_spl_args_method(&parent_lower, &method_name_lower) {
                                                is_spl = true;
                                                break;
                                            }
                                            check_class = parent_lower;
                                        } else { break; }
                                    } else { break; }
                                }
                            }
                            is_spl
                        } {
                            // SPL method that needs args - find the right parent class name
                            let mut spl_class = class_name_lower.clone();
                            if !self.is_spl_args_method(&spl_class, &method_name_lower) {
                                let mut check = spl_class.clone();
                                for _ in 0..10 {
                                    if let Some(parent) = get_builtin_parent(&check) {
                                        let parent_lower: Vec<u8> = parent.iter().map(|b| b.to_ascii_lowercase()).collect();
                                        if self.is_spl_args_method(&parent_lower, &method_name_lower) {
                                            spl_class = parent_lower;
                                            break;
                                        }
                                        check = parent_lower;
                                    } else if let Some(ce) = self.classes.get(&check) {
                                        if let Some(ref p) = ce.parent {
                                            let parent_lower: Vec<u8> = p.iter().map(|b| b.to_ascii_lowercase()).collect();
                                            if self.is_spl_args_method(&parent_lower, &method_name_lower) {
                                                spl_class = parent_lower;
                                                break;
                                            }
                                            check = parent_lower;
                                        } else { break; }
                                    } else { break; }
                                }
                            }
                            let mut spl_name = b"__spl::".to_vec();
                            spl_name.extend_from_slice(&spl_class);
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
                                if gen_borrow.state == crate::generator::GeneratorState::Created {
                                    drop(gen_borrow);
                                    let mut gen_borrow = gen_rc.borrow_mut();
                                    let _ = gen_borrow.resume(self);
                                    gen_borrow.return_value.clone()
                                } else {
                                    gen_borrow.return_value.clone()
                                }
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
            // Built-in __toString for Reflection classes
            let is_reflection = class_lower.starts_with(b"reflection");
            if is_reflection {
                let result = self.dispatch_spl_method(&class_lower, b"__tostring", obj);
                if let Some(v) = result {
                    return v.to_php_string();
                }
                // Default toString for reflection classes
                let obj_borrow = obj.borrow();
                let name = obj_borrow.get_property(b"name").to_php_string().to_string_lossy();
                return PhpString::from_string(name);
            }
        }
        val.to_php_string()
    }

    /// Check if a value is a closure (stored as string or array with closure name)
    fn is_closure_value(val: &Value) -> bool {
        match val {
            Value::String(s) => {
                let bytes = s.as_bytes();
                bytes.starts_with(b"__closure_") || bytes.starts_with(b"__arrow_") || bytes.starts_with(b"__bound_closure_") || bytes.starts_with(b"__closure_fcc_")
            }
            Value::Array(arr) => {
                let arr_borrow = arr.borrow();
                if let Some(first) = arr_borrow.values().next() {
                    if let Value::String(s) = first {
                        let bytes = s.as_bytes();
                        bytes.starts_with(b"__closure_") || bytes.starts_with(b"__arrow_") || bytes.starts_with(b"__bound_closure_") || bytes.starts_with(b"__closure_fcc_")
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// Convert a value to string, but throw an Error for objects without __toString
    pub fn value_to_string_checked(&mut self, val: &Value) -> Result<PhpString, VmError> {
        // Check for closures stored as strings/arrays
        if Self::is_closure_value(val) {
            let msg = "Object of class Closure could not be converted to string";
            let exc = self.create_exception(b"Error", msg, self.current_line);
            self.current_exception = Some(exc);
            return Err(VmError {
                message: format!("Uncaught Error: {}", msg),
                line: self.current_line,
            });
        }
        if let Value::Object(obj) = val {
            let class_name = {
                let obj_ref = obj.borrow();
                obj_ref.class_name.clone()
            };
            let class_lower: Vec<u8> = class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
            // Check if the class has __toString
            let has_tostring = self
                .classes
                .get(&class_lower)
                .map(|c| c.methods.contains_key(&b"__tostring".to_vec()))
                .unwrap_or(false);
            // Also check built-in toString support (throwable, reflection)
            let is_throwable = class_lower == b"exception"
                || class_lower == b"error"
                || is_builtin_subclass(&class_lower, b"exception")
                || is_builtin_subclass(&class_lower, b"error")
                || self.class_extends(&class_lower, b"exception")
                || self.class_extends(&class_lower, b"error");
            let is_reflection = class_lower.starts_with(b"reflection");
            if !has_tostring && !is_throwable && !is_reflection {
                let class_display = String::from_utf8_lossy(&class_name).to_string();
                let msg = format!("Object of class {} could not be converted to string", class_display);
                let exc = self.create_exception(b"Error", &msg, self.current_line);
                self.current_exception = Some(exc);
                return Err(VmError {
                    message: format!("Uncaught Error: {}", msg),
                    line: self.current_line,
                });
            }
        }
        Ok(self.value_to_string(val))
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
            OperandType::Tmp(idx) => {
                let val = tmps.get(*idx as usize).cloned().unwrap_or(Value::Null);
                if matches!(val, Value::Undef) { Value::Null } else { val }
            }
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
        b"datetime" => &[b"DateTimeInterface"],
        b"datetimeimmutable" => &[b"DateTimeInterface"],
        b"exception" | b"error" | b"typeerror" | b"valueerror" | b"argumentcounterror"
        | b"rangeerror" | b"arithmeticerror" | b"divisionbyzeroerror" | b"assertionerror"
        | b"unhandledmatcherror" | b"runtimeexception" | b"logicexception"
        | b"invalidargumentexception" | b"badmethodcallexception" | b"badfunctioncallexception"
        | b"overflowexception" | b"underflowexception" | b"outofboundsexception"
        | b"domainexception" | b"unexpectedvalueexception" | b"lengthexception"
        | b"outofrangeexception" | b"closedgeneratorexception" | b"errorexception"
        | b"jsonexception" => &[b"Throwable"],
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
        b"runtimeexception" | b"logicexception" | b"closedgeneratorexception" | b"errorexception" | b"jsonexception" => Some(b"Exception"),
        b"overflowexception" | b"underflowexception" | b"outofboundsexception" => Some(b"RuntimeException"),
        b"invalidargumentexception" | b"badmethodcallexception" | b"domainexception"
        | b"unexpectedvalueexception" | b"lengthexception" | b"outofrangeexception" => Some(b"LogicException"),
        b"badfunctioncallexception" => Some(b"BadMethodCallException"),
        b"splstack" | b"splqueue" => Some(b"SplDoublyLinkedList"),
        b"splminheap" | b"splmaxheap" => Some(b"SplHeap"),
        b"recursivearrayiterator" => Some(b"ArrayIterator"),
        b"recursiveiteratoriterator" => Some(b"IteratorIterator"),
        b"recursivecachingiterator" => Some(b"CachingIterator"),
        b"recursivefilteriterator" => Some(b"FilterIterator"),
        b"recursivecallbackfilteriterator" => Some(b"CallbackFilterIterator"),
        b"recursiveregexiterator" => Some(b"RegexIterator"),
        b"callbackfilteriterator" => Some(b"FilterIterator"),
        b"regexiterator" => Some(b"FilterIterator"),
        b"parentiterator" => Some(b"RecursiveFilterIterator"),
        b"norewinditerator" | b"infiniteiterator" | b"limititerator"
        | b"cachingiterator" | b"appenditerator" | b"filteriterator" => Some(b"IteratorIterator"),
        b"filesystemiterator" => Some(b"DirectoryIterator"),
        b"recursivedirectoryiterator" => Some(b"FilesystemIterator"),
        b"globiterator" => Some(b"FilesystemIterator"),
        b"spltempfileobject" => Some(b"SplFileObject"),
        b"splfileobject" => Some(b"SplFileInfo"),
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
        b"recursivearrayiterator" => b"RecursiveArrayIterator".to_vec(),
        b"emptyiterator" => b"EmptyIterator".to_vec(),
        b"iteratoriterator" => b"IteratorIterator".to_vec(),
        b"recursiveiteratoriterator" => b"RecursiveIteratorIterator".to_vec(),
        b"norewinditerator" => b"NoRewindIterator".to_vec(),
        b"infiniteiterator" => b"InfiniteIterator".to_vec(),
        b"limititerator" => b"LimitIterator".to_vec(),
        b"cachingiterator" => b"CachingIterator".to_vec(),
        b"recursivecachingiterator" => b"RecursiveCachingIterator".to_vec(),
        b"appenditerator" => b"AppendIterator".to_vec(),
        b"filteriterator" => b"FilterIterator".to_vec(),
        b"callbackfilteriterator" => b"CallbackFilterIterator".to_vec(),
        b"recursivefilteriterator" => b"RecursiveFilterIterator".to_vec(),
        b"recursivecallbackfilteriterator" => b"RecursiveCallbackFilterIterator".to_vec(),
        b"regexiterator" => b"RegexIterator".to_vec(),
        b"recursiveregexiterator" => b"RecursiveRegexIterator".to_vec(),
        b"multipleiterator" => b"MultipleIterator".to_vec(),
        b"parentiterator" => b"ParentIterator".to_vec(),
        b"splfileinfo" => b"SplFileInfo".to_vec(),
        b"splfileobject" => b"SplFileObject".to_vec(),
        b"spltempfileobject" => b"SplTempFileObject".to_vec(),
        b"directoryiterator" => b"DirectoryIterator".to_vec(),
        b"filesystemiterator" => b"FilesystemIterator".to_vec(),
        b"recursivedirectoryiterator" => b"RecursiveDirectoryIterator".to_vec(),
        b"globiterator" => b"GlobIterator".to_vec(),
        b"datetime" => b"DateTime".to_vec(),
        b"datetimeimmutable" => b"DateTimeImmutable".to_vec(),
        b"dateinterval" => b"DateInterval".to_vec(),
        b"datetimezone" => b"DateTimeZone".to_vec(),
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
        b"emptyiterator" => matches!(
            interface,
            b"iterator" | b"traversable"
        ),
        b"iteratoriterator" | b"recursiveiteratoriterator" => matches!(
            interface,
            b"iterator" | b"traversable" | b"outeriterator"
        ),
        b"norewinditerator" | b"infiniteiterator" | b"limititerator" => matches!(
            interface,
            b"iterator" | b"traversable" | b"outeriterator"
        ),
        b"cachingiterator" | b"recursivecachingiterator" => matches!(
            interface,
            b"iterator" | b"traversable" | b"outeriterator" | b"countable" | b"arrayaccess"
        ),
        b"appenditerator" => matches!(
            interface,
            b"iterator" | b"traversable" | b"outeriterator"
        ),
        b"filteriterator" | b"callbackfilteriterator"
        | b"recursivefilteriterator" | b"recursivecallbackfilteriterator"
        | b"regexiterator" | b"recursiveregexiterator" | b"parentiterator" => matches!(
            interface,
            b"iterator" | b"traversable" | b"outeriterator"
        ),
        b"multipleiterator" => matches!(
            interface,
            b"iterator" | b"traversable"
        ),
        b"datetime" | b"datetimeimmutable" => matches!(
            interface,
            b"datetimeinterface"
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
            b"runtimeexception" | b"logicexception" | b"closedgeneratorexception" | b"jsonexception" => {
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
            // Reflection class hierarchy
            b"reflectionexception" => Some(b"exception".to_vec()),
            b"reflectionmethod" => Some(b"reflectionfunctionabstract".to_vec()),
            b"reflectionfunction" => Some(b"reflectionfunctionabstract".to_vec()),
            b"reflectionobject" => Some(b"reflectionclass".to_vec()),
            b"reflectionenum" => Some(b"reflectionclass".to_vec()),
            b"reflectionenumbackedcase" => Some(b"reflectionenumunitcase".to_vec()),
            b"reflectionenumunitcase" => Some(b"reflectionclassconstant".to_vec()),
            b"reflectionnamedtype" => Some(b"reflectiontype".to_vec()),
            b"reflectionuniontype" => Some(b"reflectiontype".to_vec()),
            b"reflectionintersectiontype" => Some(b"reflectiontype".to_vec()),
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

/// Check if inc/dec on this value should throw a TypeError (PHP 8.3+)
/// Note: We skip object TypeError checks because they can false-positive
/// when incrementing a property value via $obj->prop++ (where the op reads the property value).
fn check_inc_dec_type(val: &Value, is_increment: bool) -> Option<String> {
    let val = match val {
        Value::Reference(r) => r.borrow().clone(),
        v => v.clone(),
    };
    match &val {
        Value::Object(obj) => {
            let class_name = {
                let o = obj.borrow();
                String::from_utf8_lossy(&o.class_name).to_string()
            };
            let op = if is_increment { "increment" } else { "decrement" };
            Some(format!("Cannot {} {}", op, class_name))
        }
        Value::Array(_) => {
            let op = if is_increment { "increment" } else { "decrement" };
            Some(format!("Cannot {} array", op))
        }
        _ => None,
    }
}

/// Emit deprecation/warning for inc/dec on non-numeric strings and booleans
fn emit_inc_dec_warnings(vm: &mut Vm, val: &Value, is_increment: bool, line: u32) {
    match val {
        Value::True | Value::False => {
            let op = if is_increment { "Increment" } else { "Decrement" };
            vm.emit_warning_at(&format!("{} on type bool has no effect, this will change in the next major version of PHP", op), line);
        }
        Value::Null | Value::Undef if !is_increment => {
            vm.emit_warning_at("Decrement on type null has no effect, this will change in the next major version of PHP", line);
        }
        Value::String(s) if is_increment => {
            let bytes = s.as_bytes();
            // Only warn for non-numeric, non-purely-alphanumeric strings
            // Purely alphanumeric strings use the magic increment (no deprecation)
            if crate::value::parse_numeric_string(bytes).is_none() {
                if bytes.is_empty() {
                    vm.emit_deprecated_at("Increment on non-numeric string is deprecated, use str_increment() instead", line);
                } else if !bytes.iter().all(|b| b.is_ascii_alphanumeric()) {
                    vm.emit_deprecated_at("Increment on non-numeric string is deprecated, use str_increment() instead", line);
                }
            }
        }
        Value::String(s) if !is_increment => {
            let bytes = s.as_bytes();
            if crate::value::parse_numeric_string(bytes).is_none() {
                if bytes.is_empty() {
                    vm.emit_deprecated_at("Decrement on empty string is deprecated as non-numeric", line);
                } else {
                    vm.emit_deprecated_at("Decrement on non-numeric string has no effect and is deprecated", line);
                }
            }
        }
        _ => {}
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
                // If the string contains '.', 'e', or 'E', it's a float string
                let is_float_string = bytes.iter().any(|&b| b == b'.' || b == b'e' || b == b'E');
                if !is_float_string && n.fract() == 0.0 && n >= i64::MIN as f64 && n <= i64::MAX as f64 {
                    return match (n as i64).checked_add(1) {
                        Some(r) => Value::Long(r),
                        None => Value::Double(n + 1.0),
                    };
                }
                return Value::Double(n + 1.0);
            }
            // Empty string: becomes "1" (PHP 8.3+ emits Deprecated)
            if bytes.is_empty() {
                return Value::String(PhpString::from_bytes(b"1"));
            }
            // Check if ALL characters are alphanumeric - PHP only does
            // alphabetic increment on purely alphanumeric strings
            let all_alnum = bytes.iter().all(|b| b.is_ascii_alphanumeric());
            if !all_alnum {
                // Contains non-alphanumeric characters: perform increment on
                // rightmost alphanumeric characters (PHP 8.3+ emits deprecation but still increments)
                let has_alnum = bytes.iter().any(|b| b.is_ascii_alphanumeric());
                if !has_alnum {
                    return val.clone();
                }
                let mut result: Vec<u8> = bytes.to_vec();
                let mut carry = true;
                for i in (0..result.len()).rev() {
                    if !carry {
                        break;
                    }
                    if !result[i].is_ascii_alphanumeric() {
                        continue;
                    }
                    carry = false;
                    match result[i] {
                        b'z' => { result[i] = b'a'; carry = true; }
                        b'Z' => { result[i] = b'A'; carry = true; }
                        b'9' => { result[i] = b'0'; carry = true; }
                        _ => { result[i] += 1; }
                    }
                }
                return Value::String(PhpString::from_vec(result));
            }
            // Alphabetic increment: "a" -> "b", "z" -> "aa", "Az" -> "Ba"
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
        Value::Object(_) | Value::Array(_) => val.clone(), // TypeError already handled in opcode handler
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
                let is_float_string = bytes.iter().any(|&b| b == b'.' || b == b'e' || b == b'E');
                if !is_float_string && n.fract() == 0.0 && n >= i64::MIN as f64 && n <= i64::MAX as f64 {
                    match (n as i64).checked_sub(1) {
                        Some(r) => return Value::Long(r),
                        None => return Value::Double(n - 1.0),
                    }
                }
                return Value::Double(n - 1.0);
            }
            // Empty string: decrement produces int(-1)
            if bytes.is_empty() {
                return Value::Long(-1);
            }
            // Non-numeric string: decrement has no effect
            val.clone()
        }
        Value::Null | Value::Undef => Value::Null, // null-- stays null
        Value::True => Value::True,   // true-- has no effect in PHP 8.3+
        Value::False => Value::False, // false-- has no effect in PHP 8.3+
        Value::Object(_) | Value::Array(_) => val.clone(), // TypeError already handled in opcode handler
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

// ==================== Date/time utility functions for VM ====================

/// Convert days since epoch (1970-01-01) to (year, month, day)
fn vm_days_to_ymd(days: i64) -> (i64, u32, u32) {
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
fn vm_ymd_to_days(year: i64, month: u32, day: u32) -> i64 {
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

/// Parse a datetime string into a unix timestamp
fn vm_parse_datetime_string(input: &str, now: i64) -> Option<i64> {
    let s = input.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("now") {
        return Some(now);
    }
    if s.starts_with('@') {
        return s[1..].trim().parse::<i64>().ok();
    }
    // Try absolute date formats
    let parts: Vec<&str> = s.splitn(2, |c: char| c == ' ' || c == 'T').collect();
    let date_str = parts.first().unwrap_or(&"");
    let date_parts: Vec<&str> = date_str.split('-').collect();
    if date_parts.len() == 3 {
        if let (Ok(year), Ok(month), Ok(day)) = (date_parts[0].parse::<i64>(), date_parts[1].parse::<u32>(), date_parts[2].parse::<u32>()) {
            if month >= 1 && month <= 12 && day >= 1 && day <= 31 {
                let mut h = 0i64;
                let mut m = 0i64;
                let mut sec = 0i64;
                if let Some(time_str) = parts.get(1) {
                    let time_clean = time_str.trim_end_matches(|c: char| c == 'Z' || c == 'z');
                    let time_no_tz = time_clean.split('+').next().unwrap_or(time_clean);
                    let time_no_micro = time_no_tz.split('.').next().unwrap_or(time_no_tz);
                    let time_parts: Vec<&str> = time_no_micro.split(':').collect();
                    h = time_parts.first().and_then(|v| v.parse().ok()).unwrap_or(0);
                    m = time_parts.get(1).and_then(|v| v.parse().ok()).unwrap_or(0);
                    sec = time_parts.get(2).and_then(|v| v.parse().ok()).unwrap_or(0);
                }
                let days = vm_ymd_to_days(year, month, day);
                return Some(days * 86400 + h * 3600 + m * 60 + sec);
            }
        }
    }
    // Relative keywords
    let lower = s.to_lowercase();
    match lower.as_str() {
        "yesterday" => return Some((now / 86400 - 1) * 86400),
        "today" | "midnight" => return Some((now / 86400) * 86400),
        "tomorrow" => return Some((now / 86400 + 1) * 86400),
        "noon" => return Some((now / 86400) * 86400 + 12 * 3600),
        _ => {}
    }
    // Try relative modification
    vm_apply_relative_modification(&lower, now)
}

/// Apply relative modification like "+1 day", "-2 hours" etc
fn vm_apply_relative_modification(s: &str, ts: i64) -> Option<i64> {
    let lower = s.trim().to_lowercase();
    let tokens: Vec<&str> = lower.split_whitespace().collect();
    let mut result = ts;
    let mut i = 0;
    let mut any_match = false;

    while i < tokens.len() {
        let token = tokens[i];
        if token == "next" || token == "last" || token == "this" {
            if i + 1 < tokens.len() {
                let amount: i64 = if token == "next" { 1 } else if token == "last" { -1 } else { 0 };
                if let Some(new_ts) = vm_apply_unit(result, amount, tokens[i + 1]) {
                    result = new_ts;
                    any_match = true;
                }
                i += 2;
                continue;
            }
        }
        if let Some(amount) = token.strip_prefix('+').and_then(|s| s.parse::<i64>().ok())
            .or_else(|| token.parse::<i64>().ok()) {
            if i + 1 < tokens.len() {
                let mut actual = amount;
                let mut skip = 2;
                if i + 2 < tokens.len() && tokens[i + 2] == "ago" { actual = -amount; skip = 3; }
                if let Some(new_ts) = vm_apply_unit(result, actual, tokens[i + 1]) {
                    result = new_ts;
                    any_match = true;
                }
                i += skip;
                continue;
            }
        }
        i += 1;
    }
    if any_match { Some(result) } else { None }
}

fn vm_apply_unit(ts: i64, amount: i64, unit: &str) -> Option<i64> {
    let unit = unit.trim_end_matches('s');
    match unit {
        "second" | "sec" => Some(ts + amount),
        "minute" | "min" => Some(ts + amount * 60),
        "hour" => Some(ts + amount * 3600),
        "day" => Some(ts + amount * 86400),
        "week" => Some(ts + amount * 7 * 86400),
        "month" => {
            let days = ts / 86400;
            let tod = ((ts % 86400) + 86400) % 86400;
            let (year, month, day) = vm_days_to_ymd(days);
            let new_m = month as i64 + amount;
            let (adj_y, adj_m) = if new_m > 0 {
                (year + (new_m - 1) / 12, ((new_m - 1) % 12 + 1) as u32)
            } else {
                (year + (new_m - 12) / 12, (12 - ((-new_m) % 12)) as u32)
            };
            let adj_m = if adj_m == 0 { 12 } else { adj_m };
            let is_leap = adj_y % 4 == 0 && (adj_y % 100 != 0 || adj_y % 400 == 0);
            let max_d = match adj_m { 2 => if is_leap {29} else {28}, 4|6|9|11 => 30, _ => 31 };
            let adj_d = day.min(max_d);
            Some(vm_ymd_to_days(adj_y, adj_m, adj_d) * 86400 + tod)
        }
        "year" => {
            let days = ts / 86400;
            let tod = ((ts % 86400) + 86400) % 86400;
            let (year, month, day) = vm_days_to_ymd(days);
            let ny = year + amount;
            let is_leap = ny % 4 == 0 && (ny % 100 != 0 || ny % 400 == 0);
            let adj_d = if month == 2 && day == 29 && !is_leap { 28 } else { day };
            Some(vm_ymd_to_days(ny, month, adj_d) * 86400 + tod)
        }
        _ => None,
    }
}

/// Parse ISO 8601 duration string like "P1Y2M3DT4H5M6S"
fn parse_iso8601_duration(spec: &str) -> (i64, i64, i64, i64, i64, i64) {
    let mut y = 0i64;
    let mut m = 0i64;
    let mut d = 0i64;
    let mut h = 0i64;
    let mut mi = 0i64;
    let mut s = 0i64;
    let bytes = spec.as_bytes();
    let mut idx = 0;
    let mut in_time = false;
    if idx < bytes.len() && bytes[idx] == b'P' { idx += 1; }
    while idx < bytes.len() {
        if bytes[idx] == b'T' { in_time = true; idx += 1; continue; }
        let start = idx;
        while idx < bytes.len() && bytes[idx].is_ascii_digit() { idx += 1; }
        if idx == start || idx >= bytes.len() { break; }
        let num: i64 = std::str::from_utf8(&bytes[start..idx]).ok().and_then(|s| s.parse().ok()).unwrap_or(0);
        match bytes[idx] {
            b'Y' => y = num,
            b'M' => if in_time { mi = num; } else { m = num; },
            b'D' => d = num,
            b'H' => h = num,
            b'S' => s = num,
            b'W' => d = num * 7,
            _ => {}
        }
        idx += 1;
    }
    (y, m, d, h, mi, s)
}

/// Create a DateInterval object from two timestamps
fn create_date_interval_from_timestamps(vm: &mut Vm, ts1: i64, ts2: i64, absolute: bool) -> Value {
    let diff = ts2 - ts1;
    let invert = if diff < 0 && !absolute { 1 } else { 0 };
    let abs_diff = diff.unsigned_abs() as i64;

    let days1 = ts1 / 86400;
    let days2 = ts2 / 86400;
    let (y1, m1, d1) = vm_days_to_ymd(days1);
    let (y2, m2, d2) = vm_days_to_ymd(days2);

    let time1 = ((ts1 % 86400) + 86400) % 86400;
    let time2 = ((ts2 % 86400) + 86400) % 86400;
    let h1 = time1 / 3600; let i1 = (time1 % 3600) / 60; let s1 = time1 % 60;
    let h2 = time2 / 3600; let i2 = (time2 % 3600) / 60; let s2 = time2 % 60;

    let (years, months, days_val, hours, minutes, seconds) = if invert == 1 {
        vm_calc_calendar_diff(y2, m2 as i64, d2 as i64, h2, i2, s2, y1, m1 as i64, d1 as i64, h1, i1, s1)
    } else {
        vm_calc_calendar_diff(y1, m1 as i64, d1 as i64, h1, i1, s1, y2, m2 as i64, d2 as i64, h2, i2, s2)
    };

    let total_days = abs_diff / 86400;
    let obj_id = vm.next_object_id;
    vm.next_object_id += 1;
    let mut obj = PhpObject::new(b"DateInterval".to_vec(), obj_id);
    obj.set_property(b"y".to_vec(), Value::Long(years));
    obj.set_property(b"m".to_vec(), Value::Long(months));
    obj.set_property(b"d".to_vec(), Value::Long(days_val));
    obj.set_property(b"h".to_vec(), Value::Long(hours));
    obj.set_property(b"i".to_vec(), Value::Long(minutes));
    obj.set_property(b"s".to_vec(), Value::Long(seconds));
    obj.set_property(b"f".to_vec(), Value::Double(0.0));
    obj.set_property(b"days".to_vec(), Value::Long(total_days));
    obj.set_property(b"invert".to_vec(), Value::Long(if absolute { 0 } else { invert }));
    Value::Object(Rc::new(RefCell::new(obj)))
}

fn vm_calc_calendar_diff(sy: i64, sm: i64, sd: i64, sh: i64, si: i64, ss: i64,
                         ey: i64, em: i64, ed: i64, eh: i64, ei: i64, es: i64) -> (i64, i64, i64, i64, i64, i64) {
    let mut seconds = es - ss;
    let mut minutes = ei - si;
    let mut hours = eh - sh;
    let mut days_val = ed - sd;
    let mut months = em - sm;
    let mut years = ey - sy;
    if seconds < 0 { seconds += 60; minutes -= 1; }
    if minutes < 0 { minutes += 60; hours -= 1; }
    if hours < 0 { hours += 24; days_val -= 1; }
    if days_val < 0 {
        let dim = match sm as u32 { 2 => if sy % 4 == 0 && (sy % 100 != 0 || sy % 400 == 0) {29} else {28}, 4|6|9|11 => 30, _ => 31 };
        days_val += dim;
        months -= 1;
    }
    if months < 0 { months += 12; years -= 1; }
    (years, months, days_val, hours, minutes, seconds)
}

/// Parse a datetime string using a PHP-style format string
fn vm_parse_with_format(format: &str, datetime: &str, now: i64) -> Option<i64> {
    let now_days = now / 86400;
    let (now_year, now_month, now_day) = vm_days_to_ymd(now_days);

    let mut year = now_year;
    let mut month = now_month;
    let mut day = now_day;
    let mut hour = 0i64;
    let mut minute = 0i64;
    let mut second = 0i64;

    let fmt_bytes = format.as_bytes();
    let dt_bytes = datetime.as_bytes();
    let mut fi = 0;
    let mut di = 0;

    while fi < fmt_bytes.len() && di <= dt_bytes.len() {
        let fc = fmt_bytes[fi];
        match fc {
            b'Y' => {
                let end = (di + 4).min(dt_bytes.len());
                year = std::str::from_utf8(&dt_bytes[di..end]).ok()?.parse().ok()?;
                di = end;
            }
            b'y' => {
                let end = (di + 2).min(dt_bytes.len());
                let y: i64 = std::str::from_utf8(&dt_bytes[di..end]).ok()?.parse().ok()?;
                year = if y >= 70 { 1900 + y } else { 2000 + y };
                di = end;
            }
            b'm' | b'n' => {
                let (val, consumed) = vm_parse_num(&dt_bytes[di..], if fc == b'm' { 2 } else { 2 })?;
                month = val as u32;
                di += consumed;
            }
            b'd' | b'j' => {
                let (val, consumed) = vm_parse_num(&dt_bytes[di..], 2)?;
                day = val as u32;
                di += consumed;
            }
            b'H' | b'G' | b'h' | b'g' => {
                let (val, consumed) = vm_parse_num(&dt_bytes[di..], 2)?;
                hour = val;
                di += consumed;
            }
            b'i' => {
                let (val, consumed) = vm_parse_num(&dt_bytes[di..], 2)?;
                minute = val;
                di += consumed;
            }
            b's' => {
                let (val, consumed) = vm_parse_num(&dt_bytes[di..], 2)?;
                second = val;
                di += consumed;
            }
            b'U' => {
                let start = di;
                if di < dt_bytes.len() && dt_bytes[di] == b'-' { di += 1; }
                while di < dt_bytes.len() && dt_bytes[di].is_ascii_digit() { di += 1; }
                return std::str::from_utf8(&dt_bytes[start..di]).ok()?.parse().ok();
            }
            b'A' | b'a' => {
                if di + 2 <= dt_bytes.len() {
                    let ampm = std::str::from_utf8(&dt_bytes[di..di+2]).ok()?.to_lowercase();
                    if ampm == "pm" && hour < 12 { hour += 12; }
                    else if ampm == "am" && hour == 12 { hour = 0; }
                    di += 2;
                }
            }
            b'u' | b'v' => {
                while di < dt_bytes.len() && dt_bytes[di].is_ascii_digit() { di += 1; }
            }
            b'e' | b'T' | b'O' | b'P' | b'p' => {
                while di < dt_bytes.len() && !dt_bytes[di].is_ascii_whitespace() { di += 1; }
            }
            b'\\' => {
                fi += 1;
                if fi < fmt_bytes.len() && di < dt_bytes.len() { di += 1; }
            }
            b'!' => { year = 1970; month = 1; day = 1; hour = 0; minute = 0; second = 0; }
            b'|' => { /* reset unset fields - simplified */ }
            _ => { if di < dt_bytes.len() { di += 1; } }
        }
        fi += 1;
    }

    let days = vm_ymd_to_days(year, month, day);
    Some(days * 86400 + hour * 3600 + minute * 60 + second)
}

fn vm_parse_num(bytes: &[u8], max_digits: usize) -> Option<(i64, usize)> {
    let mut i = 0;
    while i < bytes.len() && bytes[i] == b' ' { i += 1; }
    let start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() && (i - start) < max_digits { i += 1; }
    if i == start { return None; }
    let s = std::str::from_utf8(&bytes[start..i]).ok()?;
    Some((s.parse().ok()?, i))
}
