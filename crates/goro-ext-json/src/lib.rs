//! ext/json: a port of json.c, json_encoder.c, json_scanner.re and
//! json_parser.y.

mod decoder;
mod encoder;

use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

// Options
pub(crate) const HEX_TAG: i64 = 1 << 0;
pub(crate) const HEX_AMP: i64 = 1 << 1;
pub(crate) const HEX_APOS: i64 = 1 << 2;
pub(crate) const HEX_QUOT: i64 = 1 << 3;
pub(crate) const FORCE_OBJECT: i64 = 1 << 4;
pub(crate) const NUMERIC_CHECK: i64 = 1 << 5;
pub(crate) const UNESCAPED_SLASHES: i64 = 1 << 6;
pub(crate) const PRETTY_PRINT: i64 = 1 << 7;
pub(crate) const UNESCAPED_UNICODE: i64 = 1 << 8;
pub(crate) const PARTIAL_OUTPUT_ON_ERROR: i64 = 1 << 9;
pub(crate) const PRESERVE_ZERO_FRACTION: i64 = 1 << 10;
pub(crate) const UNESCAPED_LINE_TERMINATORS: i64 = 1 << 11;
pub(crate) const OBJECT_AS_ARRAY: i64 = 1 << 0;
pub(crate) const BIGINT_AS_STRING: i64 = 1 << 1;
pub(crate) const INVALID_UTF8_IGNORE: i64 = 1 << 20;
pub(crate) const INVALID_UTF8_SUBSTITUTE: i64 = 1 << 21;
pub(crate) const THROW_ON_ERROR: i64 = 1 << 22;

// php_json_error_code
pub(crate) const ERROR_NONE: i64 = 0;
pub(crate) const ERROR_DEPTH: i64 = 1;
pub(crate) const ERROR_STATE_MISMATCH: i64 = 2;
pub(crate) const ERROR_CTRL_CHAR: i64 = 3;
pub(crate) const ERROR_SYNTAX: i64 = 4;
pub(crate) const ERROR_UTF8: i64 = 5;
pub(crate) const ERROR_RECURSION: i64 = 6;
pub(crate) const ERROR_INF_OR_NAN: i64 = 7;
pub(crate) const ERROR_UNSUPPORTED_TYPE: i64 = 8;
pub(crate) const ERROR_INVALID_PROPERTY_NAME: i64 = 9;
pub(crate) const ERROR_UTF16: i64 = 10;
pub(crate) const ERROR_NON_BACKED_ENUM: i64 = 11;

/// PHP_JSON_PARSER_DEFAULT_DEPTH
const DEFAULT_DEPTH: i64 = 512;

/// Register all JSON extension functions
pub fn register(vm: &mut Vm) {
    vm.register_extension(b"json");
    vm.register_function(b"json_encode", json_encode);
    vm.register_function(b"json_decode", json_decode);
    vm.register_function(b"json_last_error", json_last_error);
    vm.register_function(b"json_last_error_msg", json_last_error_msg);
    vm.register_function(b"json_validate", json_validate);
}

/// php_json_get_error_msg()
fn error_msg(code: i64) -> &'static str {
    match code {
        ERROR_NONE => "No error",
        ERROR_DEPTH => "Maximum stack depth exceeded",
        ERROR_STATE_MISMATCH => "State mismatch (invalid or malformed JSON)",
        ERROR_CTRL_CHAR => "Control character error, possibly incorrectly encoded",
        ERROR_SYNTAX => "Syntax error",
        ERROR_UTF8 => "Malformed UTF-8 characters, possibly incorrectly encoded",
        ERROR_RECURSION => "Recursion detected",
        ERROR_INF_OR_NAN => "Inf and NaN cannot be JSON encoded",
        ERROR_UNSUPPORTED_TYPE => "Type is not supported",
        ERROR_INVALID_PROPERTY_NAME => "The decoded property name is invalid",
        ERROR_UTF16 => "Single unpaired UTF-16 surrogate in unicode escape",
        ERROR_NON_BACKED_ENUM => "Non-backed enums have no default serialization",
        _ => "Unknown error",
    }
}

fn pending(vm: &Vm) -> VmError {
    VmError {
        message: vm.uncaught_exception_message(),
        line: vm.current_line,
    }
}

/// zend_throw_exception(ce, message, code)
fn throw(vm: &mut Vm, class: &[u8], message: &str, code: i64) -> VmError {
    let exc = vm.create_exception(class, message, vm.current_line);
    if code != 0 {
        if let Value::Object(o) = &exc {
            o.borrow_mut()
                .set_property(b"code".to_vec(), Value::Long(code));
        }
    }
    vm.current_exception = Some(exc);
    pending(vm)
}

fn json_exception(vm: &mut Vm, code: i64) -> VmError {
    throw(vm, b"JsonException", error_msg(code), code)
}

fn arg_long(args: &[Value], i: usize, default: i64) -> i64 {
    args.get(i).map(|v| v.to_long()).unwrap_or(default)
}

fn arg_string(args: &[Value], i: usize) -> PhpString {
    args.get(i)
        .map(|v| v.to_php_string())
        .unwrap_or_else(PhpString::empty)
}

/// json_encode(mixed $value, int $flags = 0, int $depth = 512): string|false
fn json_encode(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let parameter = args.first().cloned().unwrap_or(Value::Null);
    let options = arg_long(args, 1, 0);
    let depth = arg_long(args, 2, DEFAULT_DEPTH);

    let mut enc = encoder::Encoder::new(depth as i32);
    let mut buf = Vec::new();
    encoder::encode_zval(vm, &mut buf, &parameter, options as i32 as i64, &mut enc);

    let partial = options & PARTIAL_OUTPUT_ON_ERROR != 0;
    if options & THROW_ON_ERROR == 0 || partial {
        vm.json_last_error = enc.error_code;
        if vm.current_exception.is_some() {
            return Err(pending(vm));
        }
        if enc.error_code != ERROR_NONE && !partial {
            return Ok(Value::False);
        }
    } else {
        if vm.current_exception.is_some() {
            return Err(pending(vm));
        }
        if enc.error_code != ERROR_NONE {
            return Err(json_exception(vm, enc.error_code));
        }
    }
    Ok(Value::String(PhpString::from_vec(buf)))
}

/// php_json_decode_ex() / php_json_validate_ex()
fn parse(vm: &mut Vm, s: &[u8], options: i64, depth: i64, validate: bool) -> Result<Value, i64> {
    let mut p = decoder::Parser::new(vm, s, options, depth as i32, validate);
    match p.parse() {
        Ok(v) => Ok(v),
        Err(()) => Err(p.error_code()),
    }
}

/// json_decode(string $json, ?bool $associative = null, int $depth = 512, int $flags = 0): mixed
fn json_decode(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = arg_string(args, 0);
    let assoc = args.get(1).cloned().unwrap_or(Value::Null);
    let depth = arg_long(args, 2, DEFAULT_DEPTH);
    let mut options = arg_long(args, 3, 0);

    if options & THROW_ON_ERROR == 0 {
        vm.json_last_error = ERROR_NONE;
    }
    if s.as_bytes().is_empty() {
        if options & THROW_ON_ERROR == 0 {
            vm.json_last_error = ERROR_SYNTAX;
            return Ok(Value::Null);
        }
        return Err(json_exception(vm, ERROR_SYNTAX));
    }
    if depth <= 0 {
        return Err(throw(
            vm,
            b"ValueError",
            "json_decode(): Argument #3 ($depth) must be greater than 0",
            0,
        ));
    }
    if depth > i32::MAX as i64 {
        return Err(throw(
            vm,
            b"ValueError",
            &format!(
                "json_decode(): Argument #3 ($depth) must be less than {}",
                i32::MAX
            ),
            0,
        ));
    }
    // For BC reasons, the bool $assoc overrides the long $options bit for
    // PHP_JSON_OBJECT_AS_ARRAY
    match assoc.deref() {
        Value::Null | Value::Undef => {}
        v if v.is_truthy() => options |= OBJECT_AS_ARRAY,
        _ => options &= !OBJECT_AS_ARRAY,
    }
    match parse(vm, s.as_bytes(), options, depth, false) {
        Ok(v) => Ok(v),
        Err(code) => {
            if options & THROW_ON_ERROR == 0 {
                vm.json_last_error = code;
                Ok(Value::Null)
            } else {
                Err(json_exception(vm, code))
            }
        }
    }
}

/// json_validate(string $json, int $depth = 512, int $flags = 0): bool
fn json_validate(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = arg_string(args, 0);
    let depth = arg_long(args, 1, DEFAULT_DEPTH);
    let options = arg_long(args, 2, 0);

    if options != 0 && options != INVALID_UTF8_IGNORE {
        return Err(throw(
            vm,
            b"ValueError",
            "json_validate(): Argument #3 ($flags) must be a valid flag (allowed flags: JSON_INVALID_UTF8_IGNORE)",
            0,
        ));
    }
    if s.as_bytes().is_empty() {
        vm.json_last_error = ERROR_SYNTAX;
        return Ok(Value::False);
    }
    vm.json_last_error = ERROR_NONE;
    if depth <= 0 {
        return Err(throw(
            vm,
            b"ValueError",
            "json_validate(): Argument #2 ($depth) must be greater than 0",
            0,
        ));
    }
    if depth > i32::MAX as i64 {
        return Err(throw(
            vm,
            b"ValueError",
            &format!(
                "json_validate(): Argument #2 ($depth) must be less than {}",
                i32::MAX
            ),
            0,
        ));
    }
    match parse(vm, s.as_bytes(), options, depth, true) {
        Ok(_) => Ok(Value::True),
        Err(code) => {
            vm.json_last_error = code;
            Ok(Value::False)
        }
    }
}

/// json_last_error(): int
fn json_last_error(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(vm.json_last_error))
}

/// json_last_error_msg(): string
fn json_last_error_msg(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(PhpString::from_bytes(
        error_msg(vm.json_last_error).as_bytes(),
    )))
}
