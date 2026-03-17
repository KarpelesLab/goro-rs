use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::array::{ArrayKey, PhpArray};
use crate::object::{ClassEntry, PhpObject};
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
    fn resolve_named_args(&mut self, cv_names: &[Vec<u8>], implicit_args: usize) {
        if self.named_args.is_empty() {
            return;
        }

        // Build the final args list:
        // Start with positional args, then overlay named args at their correct positions.
        let total_params = cv_names.len();
        let mut resolved = vec![None; total_params];

        // Place positional args first (these come after any implicit args like $this)
        for (i, arg) in self.args.iter().enumerate() {
            let target = implicit_args + i;
            if target < total_params {
                resolved[target] = Some(arg.clone());
            }
        }

        // Place named args by matching against cv_names
        for (name, val) in self.named_args.drain(..) {
            let mut found = false;
            for (idx, cv_name) in cv_names.iter().enumerate() {
                if *cv_name == name {
                    resolved[idx] = Some(val.clone());
                    found = true;
                    break;
                }
            }
            if !found {
                // Unknown named arg - append as positional (PHP would error,
                // but for now just add it to the end)
                self.args.push(val);
            }
        }

        // Rebuild self.args from resolved, stripping implicit args prefix
        // We need to produce a flat args vec where index 0 maps to CV[0]
        // (since the caller code maps args[i] -> func_cvs[i])
        self.args.clear();
        for slot in resolved {
            self.args.push(slot.unwrap_or(Value::Undef));
        }
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
    /// Current call depth (to prevent stack overflow from infinite recursion)
    call_depth: u32,
    /// Stack of saved error_reporting levels (for @ operator)
    error_reporting_stack: Vec<i64>,
    /// Pending return value (for deferred return in finally blocks)
    pending_return: Option<Value>,
    /// User error handler callback (from set_error_handler)
    pub error_handler: Option<Value>,
}

impl Vm {
    pub fn new() -> Self {
        Self {
            output: Vec::new(),
                ob_stack: Vec::new(),
            functions: HashMap::new(),
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
            call_depth: 0,
            error_reporting_stack: Vec::new(),
            pending_return: None,
            error_handler: None,
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
                c.insert(b"STDIN".to_vec(), Value::Null);
                c.insert(b"STDOUT".to_vec(), Value::Null);
                c.insert(b"STDERR".to_vec(), Value::Null);
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
                c.insert(b"M_SQRT2".to_vec(), Value::Double(std::f64::consts::SQRT_2));
                c.insert(b"M_SQRT3".to_vec(), Value::Double(1.7320508075688772));
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

    /// Emit a PHP warning message to output
    pub fn emit_warning(&mut self, msg: &str) {
        if self.error_reporting & 2 != 0 {
            // E_WARNING = 2
            let warning = format!("\nWarning: {} in Unknown on line 0\n", msg);
            self.output.extend_from_slice(warning.as_bytes());
        }
    }

    /// Emit a PHP warning with line number
    pub fn emit_warning_at(&mut self, msg: &str, line: u32) {
        if self.error_reporting & 2 != 0 {
            let warning = format!("\nWarning: {} in Unknown on line {}\n", msg, line);
            self.output.extend_from_slice(warning.as_bytes());
        }
    }

    /// Emit a PHP notice
    pub fn emit_notice_at(&mut self, msg: &str, line: u32) {
        if self.error_reporting & 8 != 0 {
            // E_NOTICE = 8
            let notice = format!("\nNotice: {} in Unknown on line {}\n", msg, line);
            self.output.extend_from_slice(notice.as_bytes());
        }
    }

    /// Emit a PHP deprecated warning
    pub fn emit_deprecated_at(&mut self, msg: &str, line: u32) {
        if self.error_reporting & 8192 != 0 {
            // E_DEPRECATED = 8192
            let deprec = format!("\nDeprecated: {} in Unknown on line {}\n", msg, line);
            self.output.extend_from_slice(deprec.as_bytes());
        }
    }

    /// Return a type name string for error messages.
    /// For objects, this returns the class name (e.g. "stdClass") instead of just "object".
    /// For generators, this returns "Generator".
    fn value_type_name(val: &Value) -> String {
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
        None
    }

    /// Create a TypeError exception object and set it as current_exception.
    /// Returns the error message for use in VmError if no exception handler is available.
    fn throw_type_error(&mut self, message: String) -> Value {
        let err_id = self.next_object_id;
        self.next_object_id += 1;
        let mut err_obj = PhpObject::new(b"TypeError".to_vec(), err_id);
        err_obj.set_property(
            b"message".to_vec(),
            Value::String(PhpString::from_string(message)),
        );
        err_obj.set_property(b"code".to_vec(), Value::Long(0));
        err_obj.set_property(b"file".to_vec(), Value::String(PhpString::from_bytes(b"")));
        err_obj.set_property(b"line".to_vec(), Value::Long(0));
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
    fn param_type_name(pt: &ParamType) -> String {
        match pt {
            ParamType::Simple(name) => String::from_utf8_lossy(name).to_string(),
            ParamType::Nullable(inner) => format!("?{}", Self::param_type_name(inner)),
            ParamType::Union(types) => types
                .iter()
                .map(|t| Self::param_type_name(t))
                .collect::<Vec<_>>()
                .join("|"),
            ParamType::Intersection(types) => types
                .iter()
                .map(|t| Self::param_type_name(t))
                .collect::<Vec<_>>()
                .join("&"),
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
                    b"self" | b"parent" | b"static" => {
                        // These need class context, skip for now
                        true
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
    fn param_type_display(param_type: &ParamType) -> String {
        match param_type {
            ParamType::Simple(name) => String::from_utf8_lossy(name).to_string(),
            ParamType::Nullable(inner) => format!("?{}", Self::param_type_display(inner)),
            ParamType::Union(types) => types
                .iter()
                .map(Self::param_type_display)
                .collect::<Vec<_>>()
                .join("|"),
            ParamType::Intersection(types) => types
                .iter()
                .map(Self::param_type_display)
                .collect::<Vec<_>>()
                .join("&"),
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
                    let expected = Self::param_type_display(&type_info.param_type);
                    let given = val.type_name();
                    let param_name = String::from_utf8_lossy(&type_info.param_name);
                    // Argument number is 1-based, excluding implicit args like $this
                    let arg_num = i + 1 - implicit_args;
                    return Some(format!(
                        "{}(): Argument #{} (${}) \
                         must be of type {}, {} given, called in Unknown on line {}",
                        func_display_name, arg_num, param_name, expected, given, line
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

    /// Register a class (from the compiler's compiled_classes list)
    pub fn register_class(&mut self, class: ClassEntry) {
        self.pending_classes.push(class);
    }

    /// Register a user-defined function
    pub fn register_user_function(&mut self, name: &[u8], op_array: OpArray) {
        self.user_functions
            .insert(name.to_ascii_lowercase(), op_array);
    }

    /// Register a built-in function
    pub fn register_function(&mut self, name: &[u8], func: BuiltinFn) {
        self.functions.insert(name.to_ascii_lowercase(), func);
    }

    /// Get the output buffer contents
    pub fn output(&self) -> &[u8] {
        &self.output
    }

    /// Take the output buffer
    pub fn take_output(&mut self) -> Vec<u8> {
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

    pub fn write_output(&mut self, data: &[u8]) {
        if let Some(buf) = self.ob_stack.last_mut() {
            buf.extend_from_slice(data);
        } else {
            self.output.extend_from_slice(data);
        }
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
                    let _ = self.execute_op_array(&destruct_op, fn_cvs);
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
                line: 0,
            });
        }
        let result = self.execute_op_array_inner(op_array, cvs);
        self.call_depth -= 1;

        // Check return type if declared
        if let Ok(ref val) = result {
            if let Some(ref ret_type) = op_array.return_type {
                if !self.value_matches_return_type(val, ret_type) {
                    let func_name = String::from_utf8_lossy(&op_array.name);
                    let actual_type = Self::value_type_name(val);
                    let expected_type = Self::param_type_name(ret_type);
                    let msg = format!(
                        "{}(): Return value must be of type {}, {} returned",
                        func_name, expected_type, actual_type
                    );
                    let exc_val = self.throw_type_error(msg.clone());
                    self.current_exception = Some(exc_val);
                    return Err(VmError {
                        message: msg,
                        line: 0,
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

        loop {
            if ip >= op_array.ops.len() {
                return Ok(Value::Null);
            }

            let op = &op_array.ops[ip];
            ip += 1;

            match op.opcode {
                OpCode::Nop => {}

                OpCode::Echo => {
                    let val = self.read_operand_warn(&op.op1, &cvs, &tmps, &op_array.literals, op_array, op.line);
                    let s = self.value_to_string(&val);
                    self.write_output(s.as_bytes());
                }

                OpCode::Print => {
                    let val = self.read_operand_warn(&op.op1, &cvs, &tmps, &op_array.literals, op_array, op.line);
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
                    self.write_operand(
                        &op.result,
                        Value::Long(a.to_long() & b.to_long()),
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::BitwiseOr => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        Value::Long(a.to_long() | b.to_long()),
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::BitwiseXor => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        Value::Long(a.to_long() ^ b.to_long()),
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::BitwiseNot => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        Value::Long(!a.to_long()),
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
                    self.write_operand(
                        &op.op1,
                        cv_val.concat(&rhs),
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::AssignBitwiseAnd => {
                    let cv_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(
                        &op.op1,
                        Value::Long(cv_val.to_long() & rhs.to_long()),
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::AssignBitwiseOr => {
                    let cv_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(
                        &op.op1,
                        Value::Long(cv_val.to_long() | rhs.to_long()),
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::AssignBitwiseXor => {
                    let cv_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(
                        &op.op1,
                        Value::Long(cv_val.to_long() ^ rhs.to_long()),
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
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let name_val = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    let name = name_val.to_php_string().as_bytes().to_vec();
                    if let Some(call) = self.pending_calls.last_mut() {
                        call.named_args.push((name, val));
                    }
                }
                OpCode::SendUnpack => {
                    // Unpack an array into individual arguments
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    if let Some(call) = self.pending_calls.last_mut() {
                        match val {
                            Value::Array(arr) => {
                                let arr = arr.borrow();
                                for (_, v) in arr.iter() {
                                    call.args.push(v.clone());
                                }
                            }
                            _ => {
                                // Non-array, just push as single arg
                                call.args.push(val);
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

                    let func_name_lower: Vec<u8> = call
                        .name
                        .as_bytes()
                        .iter()
                        .map(|b| b.to_ascii_lowercase())
                        .collect();

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
                        // Closure::bind($closure, $newThis, $newScope) - return closure as-is for now
                        let closure = call.args.first().cloned().unwrap_or(Value::Null);
                        self.write_operand(
                            &op.result,
                            closure,
                            &mut cvs,
                            &mut tmps,
                            &static_cv_keys,
                        );
                    } else if func_name_lower == b"__builtin_return" {
                        let result = call.args.first().cloned().unwrap_or(Value::Null);
                        self.write_operand(
                            &op.result,
                            result,
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
                        // Built-in function
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
                        // Resolve named arguments by reordering to match parameter positions
                        if !call.named_args.is_empty() {
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
                            call.resolve_named_args(&user_fn.cv_names, implicit_args_count);
                        }

                        // Check parameter types before executing
                        if !user_fn.param_types.is_empty() {
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
                            if let Some(err_msg) = self.check_param_types(
                                &user_fn,
                                &call.args,
                                &display_name,
                                implicit_args,
                                op.line,
                            ) {
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
                        }

                        // Special handling for __call/__callStatic:
                        // Pack extra args into an array for the $args parameter
                        if func_name_lower.ends_with(b"::__call")
                            || func_name_lower.ends_with(b"::__callstatic")
                        {
                            // Args: [this, method_name, arg1, arg2, ...]
                            // Need: [this, method_name, [arg1, arg2, ...]]
                            if call.args.len() > 2 {
                                let extra_args: Vec<Value> = call.args.drain(2..).collect();
                                let mut args_arr = crate::array::PhpArray::new();
                                for arg in extra_args {
                                    args_arr.push(arg);
                                }
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
                        let call_result = self.execute_op_array(&user_fn, func_cvs);

                        // Pop the called class stack
                        if pushed_called_class {
                            self.called_class_stack.pop();
                        }

                        self.is_global_scope = was_global;

                        let result = match call_result {
                            Ok(v) => v,
                            Err(e) => {
                                // Check if we have an exception handler for uncaught exceptions
                                if let Some(exc) = self.current_exception.take() {
                                    if let Some((catch_target, _, _)) = exception_handlers.pop() {
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
                            // For Exception-like classes, set message/code from args
                            if !call.args.is_empty() {
                                // First arg (after $this) is message, second is code
                                // args[0] = $this (for method calls)
                                let this_idx = if call.args.len() > 1 { 0 } else { usize::MAX };
                                if this_idx == 0
                                    && let Value::Object(obj) = &call.args[0]
                                {
                                    let mut obj_mut = obj.borrow_mut();
                                    if call.args.len() > 1 {
                                        obj_mut.set_property(
                                            b"message".to_vec(),
                                            call.args[1].clone(),
                                        );
                                    }
                                    if call.args.len() > 2 {
                                        obj_mut
                                            .set_property(b"code".to_vec(), call.args[2].clone());
                                    }
                                    if call.args.len() > 3 {
                                        obj_mut.set_property(
                                            b"previous".to_vec(),
                                            call.args[3].clone(),
                                        );
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
                            // Check for __callStatic on ClassName::method calls
                            let name_bytes = call.name.as_bytes();
                            let mut handled = false;
                            if let Some(pos) = name_bytes.iter().position(|&b| b == b':') {
                                if pos + 1 < name_bytes.len() && name_bytes[pos + 1] == b':' {
                                    let class_part = &name_bytes[..pos];
                                    let method_part = &name_bytes[pos + 2..];
                                    let class_lower: Vec<u8> =
                                        class_part.iter().map(|b| b.to_ascii_lowercase()).collect();
                                    if let Some(class_def) = self.classes.get(&class_lower) {
                                        if let Some(call_static) =
                                            class_def.get_method(b"__callstatic")
                                        {
                                            let call_static_op = call_static.op_array.clone();
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
                                            let result =
                                                self.execute_op_array(&call_static_op, fn_cvs)?;
                                            self.called_class_stack.pop();
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
                            if !handled {
                                return Err(VmError {
                                    message: format!(
                                        "Call to undefined function {}()",
                                        call.name.to_string_lossy()
                                    ),
                                    line: op.line,
                                });
                            }
                        }
                    }
                }

                OpCode::Return => {
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
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
                    if let Value::Array(arr) = arr_val {
                        arr.borrow_mut().push(val);
                    }
                }
                OpCode::ArraySet => {
                    let arr_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let val = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    let key_val = self.read_operand(&op.result, &cvs, &tmps, &op_array.literals);
                    if let Value::Array(arr) = arr_val {
                        let key = Self::value_to_array_key(key_val);
                        arr.borrow_mut().set(key, val);
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
                        arr.borrow().get(&key).cloned().unwrap_or(Value::Null)
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
                    } else {
                        Value::Null
                    };
                    self.write_operand(&op.result, result, &mut cvs, &mut tmps, &static_cv_keys);
                }

                OpCode::ForeachInit => {
                    let arr_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);

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
                        // Convert object properties to an array for iteration
                        let obj_borrow = obj.borrow();
                        let mut arr = PhpArray::new();
                        for (name, value) in &obj_borrow.properties {
                            arr.set(
                                ArrayKey::String(PhpString::from_vec(name.clone())),
                                value.clone(),
                            );
                        }
                        drop(obj_borrow);
                        let arr_val = Value::Array(Rc::new(RefCell::new(arr)));
                        self.write_operand(
                            &op.result,
                            arr_val,
                            &mut cvs,
                            &mut tmps,
                            &static_cv_keys,
                        );
                    } else {
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

                    let result = if let Value::Generator(_) = &val {
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
                    let val = self.constants.get(name_bytes).cloned().unwrap_or_else(|| {
                        // If not found, return the name as a string (PHP warning: undefined constant)
                        Value::String(name.clone())
                    });
                    self.write_operand(&op.result, val, &mut cvs, &mut tmps, &static_cv_keys);
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
                                    let compiler = crate::compiler::Compiler::new();
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
                    }
                }

                OpCode::PropertyUnset => {
                    // Remove property from object: op1 = object, op2 = property name
                    let obj_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let prop_name = self
                        .read_operand(&op.op2, &cvs, &tmps, &op_array.literals)
                        .to_php_string();
                    if let Value::Object(obj) = obj_val {
                        let mut obj = obj.borrow_mut();
                        obj.properties
                            .retain(|(name, _)| name != prop_name.as_bytes());
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
                    if let Some(exc) = &self.current_exception {
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
                            if lossy.len() > 15 {
                                // Truncate with ...
                                let truncated: String = lossy.chars().take(15).collect();
                                format!("'{}'...'", truncated)
                            } else {
                                format!("'{}'", lossy)
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
                                // Inherit methods (child overrides take precedence)
                                for (method_name, method) in &parent.methods {
                                    if !class.methods.contains_key(method_name) {
                                        class.methods.insert(method_name.clone(), method.clone());
                                    }
                                }
                                // Inherit properties (child overrides take precedence)
                                let child_prop_names: Vec<Vec<u8>> =
                                    class.properties.iter().map(|p| p.name.clone()).collect();
                                for prop in &parent.properties {
                                    if !child_prop_names.contains(&prop.name) {
                                        class.properties.push(prop.clone());
                                    }
                                }
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
                        for trait_name in &trait_names {
                            let trait_lower: Vec<u8> =
                                trait_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                            if let Some(trait_def) = self.classes.get(&trait_lower).cloned() {
                                // Copy trait methods (class's own methods take precedence)
                                for (method_name, method) in &trait_def.methods {
                                    if !class.methods.contains_key(method_name) {
                                        class.methods.insert(method_name.clone(), method.clone());
                                    }
                                }
                                // Copy trait properties (class's own properties take precedence)
                                let child_prop_names: Vec<Vec<u8>> =
                                    class.properties.iter().map(|p| p.name.clone()).collect();
                                for prop in &trait_def.properties {
                                    if !child_prop_names.contains(&prop.name) {
                                        class.properties.push(prop.clone());
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
                                let count = abstract_methods.len();
                                abstract_methods.sort();
                                let methods_list = abstract_methods.join(", ");
                                return Err(VmError {
                                    message: format!(
                                        "Class {} contains {} abstract method(s) and must therefore be declared abstract or implement the remaining methods ({})",
                                        class_name_str, count, methods_list
                                    ),
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
                            b"closedgeneratorexception" => b"ClosedGeneratorException".to_vec(),
                            b"unexpectedvalueexception" => b"UnexpectedValueException".to_vec(),
                            b"domainexception" => b"DomainException".to_vec(),
                            b"assertionerror" => b"AssertionError".to_vec(),
                            b"unhandledmatcherror" => b"UnhandledMatchError".to_vec(),
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
                            Value::String(PhpString::from_bytes(b"Unknown")),
                        );
                        obj.set_property(b"line".to_vec(), Value::Long(op.line as i64));
                        obj.set_property(b"previous".to_vec(), Value::Null);
                        obj.set_property(
                            b"trace".to_vec(),
                            Value::Array(Rc::new(RefCell::new(PhpArray::new()))),
                        );
                    }

                    // Initialize properties from class definition
                    if let Some(class) = self.classes.get(&name_lower) {
                        for prop in &class.properties {
                            if !prop.is_static {
                                obj.set_property(prop.name.clone(), prop.default.clone());
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
                        let prop = obj.borrow().get_property(prop_name.as_bytes());
                        if matches!(prop, Value::Null)
                            && !obj.borrow().has_property(prop_name.as_bytes())
                            && self.magic_depth < 5
                        {
                            // Try __get magic method (with recursion guard)
                            let class_lower: Vec<u8> = obj
                                .borrow()
                                .class_name
                                .iter()
                                .map(|b| b.to_ascii_lowercase())
                                .collect();
                            let has_get = self
                                .classes
                                .get(&class_lower)
                                .map(|c| c.methods.contains_key(&b"__get".to_vec()))
                                .unwrap_or(false);
                            if has_get {
                                self.magic_depth += 1;
                                let method = self
                                    .classes
                                    .get(&class_lower)
                                    .unwrap()
                                    .get_method(b"__get")
                                    .unwrap()
                                    .op_array
                                    .clone();
                                let mut fn_cvs = vec![Value::Undef; method.cv_names.len()];
                                if !fn_cvs.is_empty() {
                                    fn_cvs[0] = obj_val.clone();
                                } // $this
                                if fn_cvs.len() > 1 {
                                    fn_cvs[1] = Value::String(prop_name.clone());
                                } // $name
                                let result = self
                                    .execute_op_array(&method, fn_cvs)
                                    .unwrap_or(Value::Null);
                                self.magic_depth -= 1;
                                result
                            } else {
                                Value::Null
                            }
                        } else {
                            prop
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
                        // Check for __set magic method
                        let class_lower: Vec<u8> = obj
                            .borrow()
                            .class_name
                            .iter()
                            .map(|b| b.to_ascii_lowercase())
                            .collect();
                        let has_set = self
                            .classes
                            .get(&class_lower)
                            .map(|c| c.methods.contains_key(&b"__set".to_vec()))
                            .unwrap_or(false);
                        if has_set
                            && !obj.borrow().has_property(prop_name.as_bytes())
                            && self.magic_depth < 5
                        {
                            let method = self
                                .classes
                                .get(&class_lower)
                                .unwrap()
                                .get_method(b"__set")
                                .unwrap()
                                .op_array
                                .clone();
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
                            let _ = self.execute_op_array(&method, fn_cvs);
                        } else {
                            obj.borrow_mut()
                                .set_property(prop_name.as_bytes().to_vec(), value);
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
                                    Some(Value::String(PhpString::from_bytes(b"")))
                                }
                                b"getprevious" => Some(obj_borrow.get_property(b"previous")),
                                b"__tostring" => Some(obj_borrow.get_property(b"message")),
                                _ => None,
                            }
                        } else {
                            None
                        };

                        if let Some(result) = builtin_result {
                            self.pending_calls.push(PendingCall {
                                name: PhpString::from_bytes(b"__builtin_return"),
                                args: vec![result],
                                named_args: Vec::new(),
                            });
                        } else
                        // Find the method in the class
                        if let Some(class) = self.classes.get(&class_name_lower) {
                            if let Some(method) = class.get_method(&method_name_lower) {
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
                                self.pending_calls.push(PendingCall {
                                    name: call_name,
                                    args: vec![obj_val.clone()], // $this is first arg, mapped to CV 0
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
                                // Method not found - push call with $this
                                self.pending_calls.push(PendingCall {
                                    name: method_name,
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
                                // Closure::bindTo() - return closure as-is (simplified)
                                self.pending_calls.push(PendingCall {
                                    name: PhpString::from_bytes(b"__builtin_return"),
                                    args: vec![obj_val.clone()],
                                    named_args: Vec::new(),
                                });
                            }
                            b"call" => {
                                // Closure::call($newThis) - call closure with $this
                                // For now, just set up a call to the closure
                                let closure_name = obj_val.to_php_string();
                                self.pending_calls.push(PendingCall {
                                    name: closure_name,
                                    args: vec![],
                                    named_args: Vec::new(),
                                });
                            }
                            _ => {
                                // Not an object - push call without $this
                                self.pending_calls.push(PendingCall {
                                    name: method_name,
                                    args: vec![],
                                    named_args: Vec::new(),
                                });
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
                call.resolve_named_args(&user_fn.cv_names, implicit_args_count);
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
    // Get the parent chain for the child class
    let parents = builtin_parent_chain(child);
    parents.iter().any(|p| p == parent)
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
            b"overflowexception" | b"underflowexception" => Some(b"runtimeexception".to_vec()),
            b"invalidargumentexception"
            | b"badmethodcallexception"
            | b"domainexception"
            | b"unexpectedvalueexception" => Some(b"logicexception".to_vec()),
            b"badfunctioncallexception" => Some(b"badmethodcallexception".to_vec()),
            b"exception" => Some(b"throwable".to_vec()),
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
            // Only increment alphanumeric strings
            if bytes.is_empty() || !bytes.iter().all(|b| b.is_ascii_alphanumeric()) {
                // Non-alphanumeric: convert to number and increment
                return Value::Long(val.to_long() + 1);
            }
            // Check if it's a numeric string
            if let Some(n) = crate::value::parse_numeric_string(bytes) {
                if n.fract() == 0.0 && n >= i64::MIN as f64 && n <= i64::MAX as f64 {
                    return Value::Long(n as i64 + 1);
                }
                return Value::Double(n + 1.0);
            }
            // Alphabetic increment: "a" -> "b", "z" -> "aa", "Az" -> "Ba"
            let mut result: Vec<u8> = bytes.to_vec();
            let mut carry = true;
            for i in (0..result.len()).rev() {
                if !carry {
                    break;
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
        Value::False => Value::Long(1), // false++ = 1
        Value::True => Value::True,     // true++ stays true
        _ => val.add(&Value::Long(1)),
    }
}

/// PHP decrement: strings are NOT decremented (only numeric types)
fn php_decrement(val: &Value) -> Value {
    match val {
        Value::Long(n) => match n.checked_sub(1) {
            Some(r) => Value::Long(r),
            None => Value::Double(*n as f64 - 1.0),
        },
        Value::Double(f) => Value::Double(f - 1.0),
        // PHP: string decrement is not supported, stays the same
        Value::String(_) => val.clone(),
        Value::Null | Value::Undef => Value::Null, // null-- stays null
        _ => val.sub(&Value::Long(1)),
    }
}
