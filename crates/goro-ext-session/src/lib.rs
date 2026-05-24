use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

// PHP session status constants
const PHP_SESSION_DISABLED: i64 = 0;
const PHP_SESSION_NONE: i64 = 1;
const PHP_SESSION_ACTIVE: i64 = 2;

/// Per-thread session state
struct SessionState {
    started: bool,
    ini_applied: bool,
    id: String,
    name: String,
    save_path: String,
    module_name: String,
    serialize_handler: String,
    data: HashMap<String, Value>,
    cookie_lifetime: i64,
    gc_maxlifetime: i64,
    cache_limiter: String,
    cache_expire: i64,
    cookie_params: CookieParams,
    /// File:line where session_start() was last called, used in warning messages
    /// matching PHP's "(started from %s on line %d)" suffix.
    started_at: Option<(String, u32)>,
}

struct CookieParams {
    lifetime: i64,
    path: String,
    domain: String,
    secure: bool,
    httponly: bool,
    samesite: String,
}

impl Default for CookieParams {
    fn default() -> Self {
        CookieParams {
            lifetime: 0,
            path: "/".to_string(),
            domain: String::new(),
            secure: false,
            httponly: false,
            samesite: String::new(),
        }
    }
}

impl Default for SessionState {
    fn default() -> Self {
        SessionState {
            started: false,
            ini_applied: false,
            id: String::new(),
            name: "PHPSESSID".to_string(),
            save_path: String::new(),
            module_name: "files".to_string(),
            serialize_handler: "php".to_string(),
            data: HashMap::new(),
            cookie_lifetime: 0,
            gc_maxlifetime: 1440,
            cache_limiter: "nocache".to_string(),
            cache_expire: 180,
            cookie_params: CookieParams::default(),
            started_at: None,
        }
    }
}

thread_local! {
    static SESSION_STATE: RefCell<SessionState> = RefCell::new(SessionState::default());
}

/// Generate a random session ID (32 hex chars)
fn generate_session_id(prefix: &str) -> String {
    use std::time::SystemTime;
    let mut seed = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    // Mix in thread-local address for uniqueness
    seed ^= (&seed as *const u64) as u64;
    let mut id = String::with_capacity(prefix.len() + 32);
    id.push_str(prefix);
    for _ in 0..32 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        id.push(char::from(b"0123456789abcdef"[(seed & 0xF) as usize]));
    }
    id
}

/// Get the session file path for a given ID
fn session_file_path(save_path: &str, id: &str) -> String {
    format!("{}/sess_{}", save_path, id)
}

/// Emit a Warning that respects an active output buffer (ob_start).
/// PHP captures warnings into ob_stack when output_buffering / ob_start is active.
/// The core VM's emit_warning writes directly to vm.output, bypassing ob - so
/// when ob_end_flush eventually runs, the order is wrong. We replicate the
/// formatted line into ob_stack so that the order matches PHP semantics.
fn emit_warning_obaware(vm: &mut Vm, msg: &str) {
    // Respect error_reporting & E_WARNING (2)
    if vm.error_reporting & 2 == 0 {
        return;
    }
    let line = vm.current_line;
    let formatted = format!("\nWarning: {} in {} on line {}\n", msg, vm.current_file, line);
    if vm.ob_stack.is_empty() {
        // No buffer active: behave as normal (call user error handler etc).
        vm.emit_warning_at(msg, line);
    } else {
        // Active ob buffer: append directly into top buffer.
        if let Some(buf) = vm.ob_stack.last_mut() {
            buf.extend_from_slice(formatted.as_bytes());
        }
    }
}

/// Same as emit_warning_obaware but for Notice level (E_NOTICE = 8).
fn emit_notice_obaware(vm: &mut Vm, msg: &str) {
    if vm.error_reporting & 8 == 0 {
        return;
    }
    let line = vm.current_line;
    let formatted = format!("\nNotice: {} in {} on line {}\n", msg, vm.current_file, line);
    if vm.ob_stack.is_empty() {
        vm.emit_notice_at(msg, line);
    } else if let Some(buf) = vm.ob_stack.last_mut() {
        buf.extend_from_slice(formatted.as_bytes());
    }
}

/// Same as emit_warning_obaware but for Deprecated level (E_DEPRECATED = 8192).
fn emit_deprecated_obaware(vm: &mut Vm, msg: &str) {
    if vm.error_reporting & 8192 == 0 {
        return;
    }
    let line = vm.current_line;
    let formatted = format!("\nDeprecated: {} in {} on line {}\n", msg, vm.current_file, line);
    if vm.ob_stack.is_empty() {
        vm.emit_deprecated(msg);
    } else if let Some(buf) = vm.ob_stack.last_mut() {
        buf.extend_from_slice(formatted.as_bytes());
    }
}

/// Format the PHP-style "(started from %s on line %d)" suffix used in many
/// "session is active" warnings. Falls back to "(session started automatically)".
fn started_from_suffix() -> String {
    SESSION_STATE.with(|state| {
        let s = state.borrow();
        match &s.started_at {
            Some((file, line)) => format!("(started from {} on line {})", file, line),
            None => "(session started automatically)".to_string(),
        }
    })
}

/// Effective save path - falls back to /tmp when empty.
fn effective_save_path(save_path: &str) -> String {
    if save_path.is_empty() {
        std::env::temp_dir().to_string_lossy().into_owned()
    } else {
        // Parse N;M;path or N;path syntax
        let mut last = save_path;
        for part in save_path.split(';') {
            last = part;
        }
        last.to_string()
    }
}

/// Validate a session ID (alphanumeric + hyphen + comma only)
fn is_valid_session_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b',')
}

/// Encode session data in PHP session serialize format: key|serialized_value
fn encode_session_data(data: &HashMap<String, Value>) -> String {
    let mut result = String::new();
    for (key, value) in data {
        result.push_str(key);
        result.push('|');
        serialize_value(&value, &mut result);
    }
    result
}

/// Serialize a single value in PHP serialize format
fn serialize_value(value: &Value, out: &mut String) {
    match value {
        Value::Null | Value::Undef => {
            out.push_str("N;");
        }
        Value::True => {
            out.push_str("b:1;");
        }
        Value::False => {
            out.push_str("b:0;");
        }
        Value::Long(n) => {
            out.push_str(&format!("i:{};", n));
        }
        Value::Double(f) => {
            if f.is_infinite() {
                if f.is_sign_positive() {
                    out.push_str("d:INF;");
                } else {
                    out.push_str("d:-INF;");
                }
            } else if f.is_nan() {
                out.push_str("d:NAN;");
            } else {
                out.push_str(&format!("d:{};", f));
            }
        }
        Value::String(s) => {
            let bytes = s.as_bytes();
            out.push_str(&format!("s:{}:\"", bytes.len()));
            // Write raw bytes as chars (PHP serialize is binary-safe but we handle UTF-8)
            for &b in bytes {
                out.push(b as char);
            }
            out.push_str("\";");
        }
        Value::Array(arr) => {
            let arr = arr.borrow();
            out.push_str(&format!("a:{}:{{", arr.len()));
            for (key, val) in arr.iter() {
                match key {
                    ArrayKey::Int(i) => {
                        out.push_str(&format!("i:{};", i));
                    }
                    ArrayKey::String(s) => {
                        let bytes = s.as_bytes();
                        out.push_str(&format!("s:{}:\"", bytes.len()));
                        for &b in bytes {
                            out.push(b as char);
                        }
                        out.push_str("\";");
                    }
                }
                serialize_value(val, out);
            }
            out.push('}');
        }
        Value::Reference(r) => {
            serialize_value(&r.borrow(), out);
        }
        _ => {
            // Objects, generators etc. - serialize as NULL
            out.push_str("N;");
        }
    }
}

/// Decode PHP session serialize format: key|serialized_value
fn decode_session_data(data: &str) -> HashMap<String, Value> {
    let mut result = HashMap::new();
    let bytes = data.as_bytes();
    let mut pos = 0;
    while pos < bytes.len() {
        // Find the pipe separator for key|value
        let key_end = match bytes[pos..].iter().position(|&b| b == b'|') {
            Some(p) => pos + p,
            None => break,
        };
        let key = String::from_utf8_lossy(&bytes[pos..key_end]).to_string();
        pos = key_end + 1;
        // Parse the serialized value
        let (value, new_pos) = parse_serialized_value(bytes, pos);
        result.insert(key, value);
        pos = new_pos;
    }
    result
}

/// Parse a single PHP serialized value, returning (value, new_position)
fn parse_serialized_value(data: &[u8], pos: usize) -> (Value, usize) {
    if pos >= data.len() {
        return (Value::Null, pos);
    }
    match data[pos] {
        b'N' => {
            // N;
            let end = pos + 2; // skip N;
            (Value::Null, end.min(data.len()))
        }
        b'b' => {
            // b:0; or b:1;
            if pos + 3 < data.len() && data[pos + 1] == b':' {
                let val = data[pos + 2] == b'1';
                let end = pos + 4; // skip b:X;
                (
                    if val { Value::True } else { Value::False },
                    end.min(data.len()),
                )
            } else {
                (Value::Null, data.len())
            }
        }
        b'i' => {
            // i:123;
            if pos + 2 < data.len() && data[pos + 1] == b':' {
                let start = pos + 2;
                let end = match data[start..].iter().position(|&b| b == b';') {
                    Some(p) => start + p,
                    None => return (Value::Null, data.len()),
                };
                let num_str = String::from_utf8_lossy(&data[start..end]);
                let val = num_str.parse::<i64>().unwrap_or(0);
                (Value::Long(val), end + 1)
            } else {
                (Value::Null, data.len())
            }
        }
        b'd' => {
            // d:1.5;
            if pos + 2 < data.len() && data[pos + 1] == b':' {
                let start = pos + 2;
                let end = match data[start..].iter().position(|&b| b == b';') {
                    Some(p) => start + p,
                    None => return (Value::Null, data.len()),
                };
                let num_str = String::from_utf8_lossy(&data[start..end]);
                let val = match num_str.as_ref() {
                    "INF" => f64::INFINITY,
                    "-INF" => f64::NEG_INFINITY,
                    "NAN" => f64::NAN,
                    _ => num_str.parse::<f64>().unwrap_or(0.0),
                };
                (Value::Double(val), end + 1)
            } else {
                (Value::Null, data.len())
            }
        }
        b's' => {
            // s:5:"hello";
            if pos + 2 < data.len() && data[pos + 1] == b':' {
                let start = pos + 2;
                let colon = match data[start..].iter().position(|&b| b == b':') {
                    Some(p) => start + p,
                    None => return (Value::Null, data.len()),
                };
                let len_str = String::from_utf8_lossy(&data[start..colon]);
                let str_len = len_str.parse::<usize>().unwrap_or(0);
                // Skip :"
                let str_start = colon + 2; // skip :"
                if str_start + str_len > data.len() {
                    return (Value::Null, data.len());
                }
                let str_data = &data[str_start..str_start + str_len];
                let end = str_start + str_len + 2; // skip ";
                (
                    Value::String(PhpString::from_vec(str_data.to_vec())),
                    end.min(data.len()),
                )
            } else {
                (Value::Null, data.len())
            }
        }
        b'a' => {
            // a:2:{...}
            if pos + 2 < data.len() && data[pos + 1] == b':' {
                let start = pos + 2;
                let brace = match data[start..].iter().position(|&b| b == b'{') {
                    Some(p) => start + p,
                    None => return (Value::Null, data.len()),
                };
                let count_str = String::from_utf8_lossy(&data[start..brace]);
                let count = count_str.parse::<usize>().unwrap_or(0);
                let mut arr = PhpArray::new();
                let mut cur = brace + 1;
                for _ in 0..count {
                    // Parse key
                    let (key_val, next) = parse_serialized_value(data, cur);
                    cur = next;
                    // Parse value
                    let (val, next) = parse_serialized_value(data, cur);
                    cur = next;
                    // Convert key to ArrayKey
                    let key = match &key_val {
                        Value::Long(i) => ArrayKey::Int(*i),
                        Value::String(s) => ArrayKey::String(s.clone()),
                        _ => ArrayKey::Int(0),
                    };
                    arr.set(key, val);
                }
                // Skip closing }
                if cur < data.len() && data[cur] == b'}' {
                    cur += 1;
                }
                (Value::Array(Rc::new(RefCell::new(arr))), cur)
            } else {
                (Value::Null, data.len())
            }
        }
        _ => {
            // Unknown type, skip to next semicolon
            let end = match data[pos..].iter().position(|&b| b == b';') {
                Some(p) => pos + p + 1,
                None => data.len(),
            };
            (Value::Null, end)
        }
    }
}

/// Sync $_SESSION data from VM global back to session state
fn sync_session_from_vm(vm: &Vm) {
    if let Some(session_val) = vm.get_global(b"_SESSION") {
        let session_val = session_val.deref();
        if let Value::Array(arr) = session_val {
            let arr = arr.borrow();
            SESSION_STATE.with(|state| {
                let mut state = state.borrow_mut();
                state.data.clear();
                for (key, value) in arr.iter() {
                    let key_str = match key {
                        ArrayKey::Int(i) => i.to_string(),
                        ArrayKey::String(s) => s.to_string_lossy(),
                    };
                    state.data.insert(key_str, value.clone());
                }
            });
        }
    }
}

/// Set $_SESSION as a global variable in the VM from session state data.
/// Attempts in-place mutation of the existing $_SESSION array if it exists so
/// any CV (compiled variable) cached reference also sees the new contents.
fn set_session_global(vm: &mut Vm) {
    SESSION_STATE.with(|state| {
        let state = state.borrow();
        // Build the fresh array contents.
        let mut new_arr = PhpArray::new();
        for (key, value) in &state.data {
            new_arr.set(
                ArrayKey::String(PhpString::from_string(key.clone())),
                value.clone(),
            );
        }

        // If $_SESSION already exists as an array, mutate it in-place so CVs that
        // have already grabbed an Rc<RefCell<PhpArray>> see the updated state.
        if let Some(Value::Array(existing)) = vm.get_global(b"_SESSION") {
            let mut borrowed = existing.borrow_mut();
            *borrowed = new_arr;
            drop(borrowed);
            // Still reinsert the (same) Rc into globals to ensure the global slot
            // points at it (this is cheap because it's the same Rc).
            vm.set_global(b"_SESSION".to_vec(), Value::Array(existing));
            return;
        }

        vm.set_global(b"_SESSION".to_vec(), Value::Array(Rc::new(RefCell::new(new_arr))));
    });
}

/// Register all session extension functions and constants
pub fn register(vm: &mut Vm) {
    vm.register_extension(b"session");
    // Reset session state for this VM instance
    SESSION_STATE.with(|state| {
        *state.borrow_mut() = SessionState::default();
    });

    // Register functions
    vm.register_function(b"session_start", session_start);
    vm.register_function(b"session_destroy", session_destroy);
    vm.register_function(b"session_id", session_id);
    vm.register_function(b"session_name", session_name);
    vm.register_function(b"session_status", session_status);
    vm.register_function(b"session_write_close", session_write_close);
    vm.register_function(b"session_commit", session_write_close); // alias
    vm.register_function(b"session_abort", session_abort);
    vm.register_function(b"session_reset", session_reset);
    vm.register_function(b"session_unset", session_unset);
    vm.register_function(b"session_regenerate_id", session_regenerate_id);
    vm.register_function(b"session_save_path", session_save_path);
    vm.register_function(b"session_encode", session_encode);
    vm.register_function(b"session_decode", session_decode);
    vm.register_function(b"session_gc", session_gc);
    vm.register_function(b"session_create_id", session_create_id);
    vm.register_function(b"session_cache_limiter", session_cache_limiter);
    vm.register_function(b"session_cache_expire", session_cache_expire);
    vm.register_function(b"session_set_cookie_params", session_set_cookie_params);
    vm.register_function(b"session_get_cookie_params", session_get_cookie_params);
    vm.register_function(b"session_set_save_handler", session_set_save_handler_fn);
    vm.register_function(b"session_module_name", session_module_name_fn);

    // Register constants
    vm.constants
        .insert(b"PHP_SESSION_DISABLED".to_vec(), Value::Long(PHP_SESSION_DISABLED));
    vm.constants
        .insert(b"PHP_SESSION_NONE".to_vec(), Value::Long(PHP_SESSION_NONE));
    vm.constants
        .insert(b"PHP_SESSION_ACTIVE".to_vec(), Value::Long(PHP_SESSION_ACTIVE));
    // SID constant is *not* defined until session_start() runs. PHP does not pre-define it,
    // so user `define('SID', ...)` succeeds; session_start then overrides it.

    // Register default session INI values (can be overridden by --INI-- section)
    vm.constants.entry(b"session.save_path".to_vec()).or_insert_with(|| Value::String(PhpString::from_bytes(b"")));
    vm.constants.entry(b"session.name".to_vec()).or_insert_with(|| Value::String(PhpString::from_bytes(b"PHPSESSID")));
    vm.constants.entry(b"session.save_handler".to_vec()).or_insert_with(|| Value::String(PhpString::from_bytes(b"files")));
    vm.constants.entry(b"session.use_cookies".to_vec()).or_insert_with(|| Value::String(PhpString::from_bytes(b"1")));
    vm.constants.entry(b"session.use_only_cookies".to_vec()).or_insert_with(|| Value::String(PhpString::from_bytes(b"1")));
    vm.constants.entry(b"session.use_strict_mode".to_vec()).or_insert_with(|| Value::String(PhpString::from_bytes(b"0")));
    vm.constants.entry(b"session.auto_start".to_vec()).or_insert_with(|| Value::String(PhpString::from_bytes(b"0")));
    vm.constants.entry(b"session.gc_probability".to_vec()).or_insert_with(|| Value::String(PhpString::from_bytes(b"1")));
    vm.constants.entry(b"session.gc_divisor".to_vec()).or_insert_with(|| Value::String(PhpString::from_bytes(b"100")));
    vm.constants.entry(b"session.gc_maxlifetime".to_vec()).or_insert_with(|| Value::String(PhpString::from_bytes(b"1440")));
    vm.constants.entry(b"session.serialize_handler".to_vec()).or_insert_with(|| Value::String(PhpString::from_bytes(b"php")));
    vm.constants.entry(b"session.cookie_lifetime".to_vec()).or_insert_with(|| Value::String(PhpString::from_bytes(b"0")));
    vm.constants.entry(b"session.cookie_path".to_vec()).or_insert_with(|| Value::String(PhpString::from_bytes(b"/")));
    vm.constants.entry(b"session.cookie_domain".to_vec()).or_insert_with(|| Value::String(PhpString::from_bytes(b"")));
    vm.constants.entry(b"session.cookie_secure".to_vec()).or_insert_with(|| Value::String(PhpString::from_bytes(b"0")));
    vm.constants.entry(b"session.cookie_httponly".to_vec()).or_insert_with(|| Value::String(PhpString::from_bytes(b"")));
    vm.constants.entry(b"session.cookie_samesite".to_vec()).or_insert_with(|| Value::String(PhpString::from_bytes(b"")));
    vm.constants.entry(b"session.cache_limiter".to_vec()).or_insert_with(|| Value::String(PhpString::from_bytes(b"nocache")));
    vm.constants.entry(b"session.cache_expire".to_vec()).or_insert_with(|| Value::String(PhpString::from_bytes(b"180")));
    vm.constants.entry(b"session.use_trans_sid".to_vec()).or_insert_with(|| Value::String(PhpString::from_bytes(b"0")));
    vm.constants.entry(b"session.sid_length".to_vec()).or_insert_with(|| Value::String(PhpString::from_bytes(b"32")));
    vm.constants.entry(b"session.sid_bits_per_character".to_vec()).or_insert_with(|| Value::String(PhpString::from_bytes(b"4")));
    vm.constants.entry(b"session.lazy_write".to_vec()).or_insert_with(|| Value::String(PhpString::from_bytes(b"1")));
}

/// Apply INI settings from vm.constants to session state.
/// Called before session_start() to pick up settings like session.save_path.
/// Most settings only get applied once (first call). However a few like
/// session.save_path are re-read every call so that ini_set("session.save_path")
/// is observable when the session has not yet been started.
fn apply_ini_from_constants(vm: &Vm) {
    SESSION_STATE.with(|state| {
        let mut state = state.borrow_mut();
        // Always re-read save_path while no session is active so ini_set propagates.
        if !state.started {
            if let Some(val) = vm.constants.get(b"session.save_path".as_slice()) {
                state.save_path = val.to_php_string().to_string_lossy();
            }
        }
        if state.ini_applied {
            return;
        }
        state.ini_applied = true;
        if let Some(val) = vm.constants.get(b"session.save_path".as_slice()) {
            state.save_path = val.to_php_string().to_string_lossy();
        }
        if let Some(val) = vm.constants.get(b"session.name".as_slice()) {
            let s = val.to_php_string().to_string_lossy();
            if !s.is_empty() {
                state.name = s;
            }
        }
        if let Some(val) = vm.constants.get(b"session.gc_maxlifetime".as_slice()) {
            state.gc_maxlifetime = val.to_long();
        }
        if let Some(val) = vm.constants.get(b"session.cache_expire".as_slice()) {
            state.cache_expire = val.to_long();
        }
        if let Some(val) = vm.constants.get(b"session.cache_limiter".as_slice()) {
            let s = val.to_php_string().to_string_lossy();
            if !s.is_empty() {
                state.cache_limiter = s;
            }
        }
        if let Some(val) = vm.constants.get(b"session.cookie_lifetime".as_slice()) {
            state.cookie_params.lifetime = val.to_long();
        }
        if let Some(val) = vm.constants.get(b"session.cookie_path".as_slice()) {
            state.cookie_params.path = val.to_php_string().to_string_lossy();
        }
        if let Some(val) = vm.constants.get(b"session.cookie_domain".as_slice()) {
            state.cookie_params.domain = val.to_php_string().to_string_lossy();
        }
        if let Some(val) = vm.constants.get(b"session.cookie_secure".as_slice()) {
            state.cookie_params.secure = val.is_truthy();
        }
        if let Some(val) = vm.constants.get(b"session.cookie_httponly".as_slice()) {
            state.cookie_params.httponly = val.is_truthy();
        }
        if let Some(val) = vm.constants.get(b"session.cookie_samesite".as_slice()) {
            state.cookie_params.samesite = val.to_php_string().to_string_lossy();
        }
    });
}

/// Initialize session state - call after first session function used to capture INI values.
fn ensure_ini_initialized(vm: &Vm) {
    apply_ini_from_constants(vm);
}

/// session_start(array $options = []): bool
fn session_start(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Apply INI settings before starting
    ensure_ini_initialized(vm);

    let already_started = SESSION_STATE.with(|state| state.borrow().started);
    if already_started {
        emit_notice_obaware(
            vm,
            "session_start(): Ignoring session_start() because a session is already active",
        );
        return Ok(Value::True);
    }

    // Process options array if provided
    if let Some(Value::Array(opts)) = args.first() {
        let opts = opts.borrow();
        for (key, val) in opts.iter() {
            if let ArrayKey::String(k) = &key {
                let key_str = k.to_string_lossy();
                match key_str.as_str() {
                    "save_path" => {
                        let path = val.to_php_string().to_string_lossy();
                        SESSION_STATE.with(|state| {
                            state.borrow_mut().save_path = path;
                        });
                    }
                    "name" => {
                        let name = val.to_php_string().to_string_lossy();
                        SESSION_STATE.with(|state| {
                            state.borrow_mut().name = name;
                        });
                    }
                    "cookie_lifetime" => {
                        let lifetime = val.to_long();
                        SESSION_STATE.with(|state| {
                            state.borrow_mut().cookie_lifetime = lifetime;
                        });
                    }
                    "gc_maxlifetime" => {
                        let maxlifetime = val.to_long();
                        SESSION_STATE.with(|state| {
                            state.borrow_mut().gc_maxlifetime = maxlifetime;
                        });
                    }
                    _ => {}
                }
            }
        }
    }

    // Read session id from $_COOKIE / $_GET / $_POST when their respective
    // settings allow it. PHP precedence: cookie > GET/POST when use_cookies enabled.
    let use_cookies = vm
        .constants
        .get(b"session.use_cookies".as_slice())
        .map(|v| v.is_truthy())
        .unwrap_or(true);
    let use_only_cookies = vm
        .constants
        .get(b"session.use_only_cookies".as_slice())
        .map(|v| v.is_truthy())
        .unwrap_or(true);
    let session_name = SESSION_STATE.with(|s| s.borrow().name.clone());

    let mut detected_id: Option<String> = None;
    // Try cookie first when use_cookies is enabled
    if use_cookies {
        if let Some(Value::Array(cookie)) = vm.get_global(b"_COOKIE") {
            let cookie = cookie.borrow();
            if let Some(v) = cookie.get(&ArrayKey::String(PhpString::from_string(session_name.clone()))) {
                let s = v.to_php_string().to_string_lossy();
                if !s.is_empty() {
                    detected_id = Some(s);
                }
            }
        }
    }
    // Fall back to GET/POST when use_only_cookies is disabled
    if detected_id.is_none() && !use_only_cookies {
        for sg in [b"_GET".as_slice(), b"_POST".as_slice()] {
            if let Some(Value::Array(arr)) = vm.get_global(sg) {
                let arr = arr.borrow();
                if let Some(v) = arr.get(&ArrayKey::String(PhpString::from_string(session_name.clone()))) {
                    let s = v.to_php_string().to_string_lossy();
                    if !s.is_empty() {
                        detected_id = Some(s);
                        break;
                    }
                }
            }
        }
    }

    // Validate save path directory existence (only when user explicitly set it).
    let (raw_save_path, resolved_path) = SESSION_STATE.with(|state| {
        let s = state.borrow();
        (s.save_path.clone(), effective_save_path(&s.save_path))
    });
    let user_provided_path = !raw_save_path.is_empty();
    if user_provided_path {
        let exists = std::path::Path::new(&resolved_path).is_dir();
        if !exists {
            let line = vm.current_line;
            let file = vm.current_file.clone();
            let msg1 = format!(
                "session_start(): open({}{}{}, O_RDWR) failed: No such file or directory (2)",
                resolved_path,
                std::path::MAIN_SEPARATOR,
                "sess_X"
            );
            let msg2 = format!(
                "session_start(): Failed to read session data: files (path: {})",
                resolved_path
            );
            // Use ob-aware emission.
            emit_warning_obaware(vm, &msg1);
            emit_warning_obaware(vm, &msg2);
            let _ = (line, file); // unused but kept for parity
            return Ok(Value::False);
        }
    }

    let start_loc = (vm.current_file.clone(), vm.current_line);
    SESSION_STATE.with(|state| {
        let mut state = state.borrow_mut();
        // Apply detected id from superglobals (overrides any prior id).
        if let Some(id) = detected_id {
            state.id = id;
        }
        // Generate session ID if not already set
        if state.id.is_empty() {
            state.id = generate_session_id("");
        }

        // Resolve save path - fall back to /tmp when empty (PHP defaults to sys_temp_dir)
        let path = effective_save_path(&state.save_path);
        // Try to load existing session data from file
        let file_path = session_file_path(&path, &state.id);
        if let Ok(contents) = std::fs::read_to_string(&file_path) {
            state.data = decode_session_data(&contents);
        } else {
            // No existing data - start with empty session
            state.data.clear();
        }

        state.started = true;
        state.started_at = Some(start_loc);
    });

    // Set $_SESSION global
    set_session_global(vm);

    // Update SID constant: empty if cookies are used (PHP behaviour), otherwise "NAME=ID".
    let cookies_in_use = use_cookies || use_only_cookies;
    let (name, id) = SESSION_STATE.with(|state| {
        let s = state.borrow();
        (s.name.clone(), s.id.clone())
    });
    let sid_val = if cookies_in_use {
        PhpString::from_bytes(b"")
    } else {
        PhpString::from_string(format!("{}={}", name, id))
    };
    vm.constants.insert(b"SID".to_vec(), Value::String(sid_val));

    Ok(Value::True)
}

/// session_destroy(): bool
fn session_destroy(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let started = SESSION_STATE.with(|state| state.borrow().started);
    if !started {
        emit_warning_obaware(vm, "session_destroy(): Trying to destroy uninitialized session");
        return Ok(Value::False);
    }

    SESSION_STATE.with(|state| {
        let mut state = state.borrow_mut();
        // Delete session file
        let path = effective_save_path(&state.save_path);
        let file_path = session_file_path(&path, &state.id);
        let _ = std::fs::remove_file(&file_path);
        state.started = false;
        state.data.clear();
        state.id.clear();
    });

    // Reset $_SESSION to empty array (PHP behaviour: session_unset is implicit on destroy when called)
    set_session_global(vm);

    Ok(Value::True)
}

/// session_id(string $id = ?): string|false
fn session_id(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    ensure_ini_initialized(vm);
    let current_id = SESSION_STATE.with(|state| state.borrow().id.clone());

    if let Some(new_id_val) = args.first() {
        if !matches!(new_id_val, Value::Null | Value::Undef) {
            let new_id = new_id_val.to_php_string().to_string_lossy();
            let started = SESSION_STATE.with(|state| state.borrow().started);
            if started {
                let suffix = started_from_suffix();
                emit_warning_obaware(vm, &format!(
                    "session_id(): Session ID cannot be changed when a session is active {}",
                    suffix
                ));
                return Ok(Value::False);
            }
            SESSION_STATE.with(|state| {
                state.borrow_mut().id = new_id;
            });
        }
    }

    Ok(Value::String(PhpString::from_string(current_id)))
}

/// session_name(string $name = ?): string|false
fn session_name(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    ensure_ini_initialized(vm);
    let current_name = SESSION_STATE.with(|state| state.borrow().name.clone());

    if let Some(new_name_val) = args.first() {
        if !matches!(new_name_val, Value::Null | Value::Undef) {
            let new_name_str = new_name_val.to_php_string();
            let new_name_bytes = new_name_str.as_bytes();
            // Null-byte check raises a TypeError-like fatal in PHP via Argument check
            if new_name_bytes.contains(&0u8) {
                let msg =
                    "session_name(): Argument #1 ($name) must not contain any null bytes"
                        .to_string();
                let line = vm.current_line;
                let exc = vm.create_exception(b"ValueError", &msg, line);
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg, line });
            }
            let started = SESSION_STATE.with(|state| state.borrow().started);
            if started {
                let suffix = started_from_suffix();
                emit_warning_obaware(vm, &format!(
                    "session_name(): Session name cannot be changed when a session is active {}",
                    suffix
                ));
                return Ok(Value::False);
            }
            let new_name = new_name_str.to_string_lossy();
            if !is_valid_session_setting_name(&new_name) {
                emit_warning_obaware(vm, &format!(
                    "session_name(): session.name \"{}\" must not be numeric, empty, contain null bytes or any of the following characters \"=,;.[ \\t\\r\\n\\013\\014\"",
                    new_name
                ));
                return Ok(Value::String(PhpString::from_string(current_name)));
            }
            SESSION_STATE.with(|state| {
                state.borrow_mut().name = new_name.clone();
            });
            vm.constants.insert(
                b"session.name".to_vec(),
                Value::String(PhpString::from_string(new_name)),
            );
        }
    }

    Ok(Value::String(PhpString::from_string(current_name)))
}

/// Validate a session.name value - must not be empty, numeric, or contain forbidden chars.
fn is_valid_session_setting_name(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    // Cannot be entirely numeric
    if s.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    // Forbidden chars: =,;.[ \t\r\n\013\014 (and null bytes)
    for c in s.chars() {
        match c {
            '=' | ',' | ';' | '.' | '[' | ' ' | '\t' | '\r' | '\n' | '\x0B' | '\x0C'
            | '\0' => return false,
            _ => {}
        }
    }
    true
}

/// session_status(): int
fn session_status(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let status = SESSION_STATE.with(|state| {
        if state.borrow().started {
            PHP_SESSION_ACTIVE
        } else {
            PHP_SESSION_NONE
        }
    });
    Ok(Value::Long(status))
}

/// session_write_close() / session_commit(): bool
fn session_write_close(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let started = SESSION_STATE.with(|state| state.borrow().started);
    if !started {
        return Ok(Value::True);
    }

    // Sync data from $_SESSION global variable back to state
    sync_session_from_vm(vm);

    // Validate keys - PHP errors out when a session key contains '|' or '!'.
    let invalid_key = SESSION_STATE.with(|state| {
        state
            .borrow()
            .data
            .keys()
            .find(|k| k.contains('|') || k.contains('!'))
            .cloned()
    });
    if let Some(bad) = invalid_key {
        emit_warning_obaware(
            vm,
            &format!(
                "session_write_close(): Failed to write session data. Data contains invalid key \"{}\"",
                bad
            ),
        );
        SESSION_STATE.with(|state| {
            state.borrow_mut().started = false;
        });
        return Ok(Value::False);
    }

    SESSION_STATE.with(|state| {
        let mut state = state.borrow_mut();
        // Write session data to file
        let path = effective_save_path(&state.save_path);
        let file_path = session_file_path(&path, &state.id);
        let encoded = encode_session_data(&state.data);
        let _ = std::fs::write(&file_path, encoded);
        state.started = false;
    });

    Ok(Value::True)
}

/// session_abort(): bool
fn session_abort(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let started = SESSION_STATE.with(|state| state.borrow().started);
    if !started {
        return Ok(Value::True);
    }

    SESSION_STATE.with(|state| {
        let mut state = state.borrow_mut();
        // Discard changes, just close the session
        state.started = false;
    });

    Ok(Value::True)
}

/// session_reset(): bool
fn session_reset(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let started = SESSION_STATE.with(|state| state.borrow().started);
    if !started {
        emit_warning_obaware(vm, "session_reset(): Session is not active");
        return Ok(Value::False);
    }

    // Re-read session data from file
    SESSION_STATE.with(|state| {
        let mut state = state.borrow_mut();
        let path = effective_save_path(&state.save_path);
        let file_path = session_file_path(&path, &state.id);
        if let Ok(contents) = std::fs::read_to_string(&file_path) {
            state.data = decode_session_data(&contents);
        } else {
            state.data.clear();
        }
    });

    // Update $_SESSION global
    set_session_global(vm);

    Ok(Value::True)
}

/// session_unset(): bool
fn session_unset(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    SESSION_STATE.with(|state| {
        state.borrow_mut().data.clear();
    });

    // Update $_SESSION global to empty array
    set_session_global(vm);

    Ok(Value::True)
}

/// session_regenerate_id(bool $delete_old_session = false): bool
fn session_regenerate_id(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let started = SESSION_STATE.with(|state| state.borrow().started);
    if !started {
        emit_warning_obaware(vm, 
            "session_regenerate_id(): Session ID cannot be regenerated when there is no active session",
        );
        return Ok(Value::False);
    }

    let delete_old = args
        .first()
        .map(|v| v.is_truthy())
        .unwrap_or(false);

    // Sync current data from VM
    sync_session_from_vm(vm);

    SESSION_STATE.with(|state| {
        let mut state = state.borrow_mut();
        let old_id = state.id.clone();
        state.id = generate_session_id("");

        // Write current data to the new session file so it can be read after regenerate
        let path = effective_save_path(&state.save_path);
        let new_file = session_file_path(&path, &state.id);
        let encoded = encode_session_data(&state.data);
        let _ = std::fs::write(&new_file, encoded);

        if delete_old {
            let old_file = session_file_path(&path, &old_id);
            let _ = std::fs::remove_file(&old_file);
        }
    });

    Ok(Value::True)
}

/// session_save_path(string $path = ?): string|false
fn session_save_path(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    ensure_ini_initialized(vm);
    let current_path = SESSION_STATE.with(|state| state.borrow().save_path.clone());

    if let Some(new_path_val) = args.first() {
        if !matches!(new_path_val, Value::Null | Value::Undef) {
            let new_path = new_path_val.to_php_string().to_string_lossy();
            let started = SESSION_STATE.with(|state| state.borrow().started);
            if started {
                let suffix = started_from_suffix();
                emit_warning_obaware(vm, &format!(
                    "session_save_path(): Session save path cannot be changed when a session is active {}",
                    suffix
                ));
                return Ok(Value::False);
            }
            SESSION_STATE.with(|state| {
                state.borrow_mut().save_path = new_path.clone();
            });
            // Mirror into INI so subsequent ini_get returns the same value.
            vm.constants.insert(
                b"session.save_path".to_vec(),
                Value::String(PhpString::from_string(new_path)),
            );
        }
    }

    Ok(Value::String(PhpString::from_string(current_path)))
}

/// session_encode(): string|false
fn session_encode(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let started = SESSION_STATE.with(|state| state.borrow().started);
    if !started {
        emit_warning_obaware(vm, "session_encode(): Cannot encode non-existent session");
        return Ok(Value::False);
    }

    // Sync from VM first
    sync_session_from_vm(vm);

    let handler = vm
        .constants
        .get(b"session.serialize_handler".as_slice())
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_else(|| "php".to_string());

    let encoded = SESSION_STATE.with(|state| {
        let state = state.borrow();
        if handler == "php_serialize" {
            // Encode entire data as a serialized array.
            let mut arr = PhpArray::new();
            for (k, v) in &state.data {
                arr.set(
                    ArrayKey::String(PhpString::from_string(k.clone())),
                    v.clone(),
                );
            }
            let val = Value::Array(Rc::new(RefCell::new(arr)));
            let mut out = String::new();
            serialize_value(&val, &mut out);
            out
        } else {
            encode_session_data(&state.data)
        }
    });

    Ok(Value::String(PhpString::from_string(encoded)))
}

/// session_decode(string $data): bool
fn session_decode(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let started = SESSION_STATE.with(|state| state.borrow().started);
    if !started {
        emit_warning_obaware(vm, 
            "session_decode(): Session data cannot be decoded when there is no active session",
        );
        return Ok(Value::False);
    }

    let data = args
        .first()
        .unwrap_or(&Value::Null)
        .to_php_string()
        .to_string_lossy();

    // Pick the right validator/decoder based on the current serialize handler.
    let handler = vm
        .constants
        .get(b"session.serialize_handler".as_slice())
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_else(|| "php".to_string());

    let (valid, decoded) = match handler.as_str() {
        "php_serialize" => {
            // Top-level value is a PHP-serialized array.
            let bytes = data.as_bytes();
            let (val, new_pos) = parse_serialized_value(bytes, 0);
            if new_pos != bytes.len() {
                (false, HashMap::new())
            } else if let Value::Array(arr) = val {
                let mut map = HashMap::new();
                for (k, v) in arr.borrow().iter() {
                    let key = match k {
                        ArrayKey::Int(i) => i.to_string(),
                        ArrayKey::String(s) => s.to_string_lossy(),
                    };
                    map.insert(key, v.clone());
                }
                (true, map)
            } else if matches!(val, Value::Null) && data.is_empty() {
                (true, HashMap::new())
            } else {
                (false, HashMap::new())
            }
        }
        _ => {
            // Default "php" handler: key|serialized_value pairs.
            if data.is_empty() {
                (true, HashMap::new())
            } else if is_valid_session_encoded(&data) {
                (true, decode_session_data(&data))
            } else {
                (false, HashMap::new())
            }
        }
    };

    if !valid {
        emit_warning_obaware(vm, 
            "session_decode(): Failed to decode session object. Session has been destroyed",
        );
        // Destroy the session (PHP behaviour): remove file, clear state, mark not started.
        SESSION_STATE.with(|state| {
            let mut state = state.borrow_mut();
            let path = effective_save_path(&state.save_path);
            let file_path = session_file_path(&path, &state.id);
            let _ = std::fs::remove_file(&file_path);
            state.started = false;
            state.data.clear();
            state.id.clear();
        });
        // Reset $_SESSION to empty
        set_session_global(vm);
        return Ok(Value::False);
    }

    SESSION_STATE.with(|state| {
        state.borrow_mut().data = decoded;
    });

    // Update $_SESSION global
    set_session_global(vm);

    Ok(Value::True)
}

/// Check whether the input is parseable as a session-encoded string in `php` format.
fn is_valid_session_encoded(data: &str) -> bool {
    let bytes = data.as_bytes();
    let mut pos = 0;
    while pos < bytes.len() {
        let key_end = match bytes[pos..].iter().position(|&b| b == b'|') {
            Some(p) => pos + p,
            None => return false,
        };
        // Key must be non-empty and not contain bad chars (basic check: not null bytes)
        if key_end == pos {
            return false;
        }
        for &c in &bytes[pos..key_end] {
            if c == 0 || c == b'!' {
                return false;
            }
        }
        pos = key_end + 1;
        // Try to parse the serialized value
        let (ok, next_pos) = validate_serialized_value(bytes, pos);
        if !ok {
            return false;
        }
        pos = next_pos;
    }
    true
}

/// Validate a PHP serialized value, returning (is_valid, new_position).
fn validate_serialized_value(data: &[u8], pos: usize) -> (bool, usize) {
    if pos >= data.len() {
        return (false, pos);
    }
    match data[pos] {
        b'N' => {
            if pos + 1 < data.len() && data[pos + 1] == b';' {
                (true, pos + 2)
            } else {
                (false, pos)
            }
        }
        b'b' => {
            if pos + 3 < data.len() && data[pos + 1] == b':' && data[pos + 3] == b';'
                && (data[pos + 2] == b'0' || data[pos + 2] == b'1')
            {
                (true, pos + 4)
            } else {
                (false, pos)
            }
        }
        b'i' => {
            if pos + 2 >= data.len() || data[pos + 1] != b':' {
                return (false, pos);
            }
            let start = pos + 2;
            let end = match data[start..].iter().position(|&b| b == b';') {
                Some(p) => start + p,
                None => return (false, pos),
            };
            // Verify number
            let s = match std::str::from_utf8(&data[start..end]) {
                Ok(v) => v,
                Err(_) => return (false, pos),
            };
            if s.parse::<i64>().is_err() {
                return (false, pos);
            }
            (true, end + 1)
        }
        b'd' => {
            if pos + 2 >= data.len() || data[pos + 1] != b':' {
                return (false, pos);
            }
            let start = pos + 2;
            let end = match data[start..].iter().position(|&b| b == b';') {
                Some(p) => start + p,
                None => return (false, pos),
            };
            (true, end + 1)
        }
        b's' => {
            if pos + 2 >= data.len() || data[pos + 1] != b':' {
                return (false, pos);
            }
            let start = pos + 2;
            let colon = match data[start..].iter().position(|&b| b == b':') {
                Some(p) => start + p,
                None => return (false, pos),
            };
            let len_str = match std::str::from_utf8(&data[start..colon]) {
                Ok(v) => v,
                Err(_) => return (false, pos),
            };
            let str_len = match len_str.parse::<usize>() {
                Ok(v) => v,
                Err(_) => return (false, pos),
            };
            let str_start = colon + 2;
            if str_start > data.len() || str_start + str_len + 2 > data.len() {
                return (false, pos);
            }
            if data[colon + 1] != b'"' || data[str_start + str_len] != b'"'
                || data[str_start + str_len + 1] != b';'
            {
                return (false, pos);
            }
            (true, str_start + str_len + 2)
        }
        b'a' => {
            if pos + 2 >= data.len() || data[pos + 1] != b':' {
                return (false, pos);
            }
            let start = pos + 2;
            let brace = match data[start..].iter().position(|&b| b == b'{') {
                Some(p) => start + p,
                None => return (false, pos),
            };
            let count_str = match std::str::from_utf8(&data[start..brace]) {
                Ok(v) => v,
                Err(_) => return (false, pos),
            };
            // Strip trailing `:`
            let count_str = count_str.strip_suffix(':').unwrap_or(count_str);
            let count = match count_str.parse::<usize>() {
                Ok(v) => v,
                Err(_) => return (false, pos),
            };
            let mut cur = brace + 1;
            for _ in 0..count {
                let (ok, next) = validate_serialized_value(data, cur);
                if !ok {
                    return (false, pos);
                }
                cur = next;
                let (ok, next) = validate_serialized_value(data, cur);
                if !ok {
                    return (false, pos);
                }
                cur = next;
            }
            if cur >= data.len() || data[cur] != b'}' {
                return (false, pos);
            }
            (true, cur + 1)
        }
        b'O' | b'C' | b'R' | b'r' => {
            // Object/class/reference - just scan to terminating } or ;
            // For now, treat as valid if it parses successfully
            // Simple approach: find balanced braces
            let start = pos;
            let mut cur = pos + 2;
            // Skip past ":n:" to "{"
            while cur < data.len() && data[cur] != b'{' && data[cur] != b';' {
                cur += 1;
            }
            if cur >= data.len() {
                return (false, start);
            }
            if data[cur] == b';' {
                return (true, cur + 1);
            }
            // Match braces
            let mut depth = 1i32;
            cur += 1;
            while cur < data.len() && depth > 0 {
                match data[cur] {
                    b'{' => depth += 1,
                    b'}' => depth -= 1,
                    _ => {}
                }
                cur += 1;
            }
            if depth != 0 {
                return (false, start);
            }
            (true, cur)
        }
        _ => (false, pos),
    }
}

/// session_gc(): int|false
fn session_gc(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let started = SESSION_STATE.with(|state| state.borrow().started);
    if !started {
        emit_warning_obaware(vm, 
            "session_gc(): Session cannot be garbage collected when there is no active session",
        );
        return Ok(Value::False);
    }
    let (save_path, maxlifetime) = SESSION_STATE.with(|state| {
        let state = state.borrow();
        (effective_save_path(&state.save_path), state.gc_maxlifetime)
    });

    let mut cleaned = 0i64;
    if let Ok(entries) = std::fs::read_dir(&save_path) {
        let now = std::time::SystemTime::now();
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with("sess_") {
                    if let Ok(metadata) = std::fs::metadata(&path) {
                        if let Ok(modified) = metadata.modified() {
                            if let Ok(age) = now.duration_since(modified) {
                                if age.as_secs() as i64 > maxlifetime {
                                    if std::fs::remove_file(&path).is_ok() {
                                        cleaned += 1;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(Value::Long(cleaned))
}

/// session_create_id(string $prefix = ""): string|false
fn session_create_id(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let prefix = args
        .first()
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_default();

    let id = generate_session_id(&prefix);
    Ok(Value::String(PhpString::from_string(id)))
}

/// session_cache_limiter(?string $value = null): string|false
fn session_cache_limiter(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    ensure_ini_initialized(vm);
    let current = SESSION_STATE.with(|state| state.borrow().cache_limiter.clone());

    if let Some(val) = args.first() {
        if !matches!(val, Value::Null | Value::Undef) {
            let new_val = val.to_php_string().to_string_lossy();
            let started = SESSION_STATE.with(|state| state.borrow().started);
            if started {
                let suffix = started_from_suffix();
                emit_warning_obaware(vm, &format!(
                    "session_cache_limiter(): Session cache limiter cannot be changed when a session is active {}",
                    suffix
                ));
                return Ok(Value::False);
            }
            SESSION_STATE.with(|state| {
                state.borrow_mut().cache_limiter = new_val;
            });
        }
    }

    Ok(Value::String(PhpString::from_string(current)))
}

/// session_cache_expire(?int $value = null): int|false
fn session_cache_expire(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    ensure_ini_initialized(vm);
    let current = SESSION_STATE.with(|state| state.borrow().cache_expire);

    if let Some(val) = args.first() {
        if !matches!(val, Value::Null | Value::Undef) {
            let new_val = val.to_long();
            let started = SESSION_STATE.with(|state| state.borrow().started);
            if started {
                let suffix = started_from_suffix();
                emit_warning_obaware(vm, &format!(
                    "session_cache_expire(): Session cache expiration cannot be changed when a session is active {}",
                    suffix
                ));
                return Ok(Value::False);
            }
            SESSION_STATE.with(|state| {
                state.borrow_mut().cache_expire = new_val;
            });
        }
    }

    Ok(Value::Long(current))
}

/// session_set_cookie_params(int|array $lifetime_or_options, ...): bool
fn session_set_cookie_params(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    ensure_ini_initialized(vm);
    if args.is_empty() {
        return Ok(Value::False);
    }

    // Active-session guard.
    let started = SESSION_STATE.with(|state| state.borrow().started);
    if started {
        let suffix = started_from_suffix();
        emit_warning_obaware(
            vm,
            &format!(
                "session_set_cookie_params(): Session cookie parameters cannot be changed when a session is active {}",
                suffix
            ),
        );
        return Ok(Value::False);
    }

    let mut new_lifetime: Option<i64> = None;
    let mut new_path: Option<String> = None;
    let mut new_domain: Option<String> = None;
    let mut new_secure: Option<bool> = None;
    let mut new_httponly: Option<bool> = None;
    let mut new_samesite: Option<String> = None;

    // Check if first arg is an array (options form)
    if let Value::Array(opts) = args.first().unwrap_or(&Value::Null) {
        let opts = opts.borrow();
        for (key, val) in opts.iter() {
            if let ArrayKey::String(k) = &key {
                let key_str = k.to_string_lossy();
                match key_str.as_str() {
                    "lifetime" => new_lifetime = Some(val.to_long()),
                    "path" => new_path = Some(val.to_php_string().to_string_lossy()),
                    "domain" => new_domain = Some(val.to_php_string().to_string_lossy()),
                    "secure" => new_secure = Some(val.is_truthy()),
                    "httponly" => new_httponly = Some(val.is_truthy()),
                    "samesite" => new_samesite = Some(val.to_php_string().to_string_lossy()),
                    _ => {}
                }
            }
        }
    } else {
        // Positional: session_set_cookie_params(lifetime, path, domain, secure, httponly)
        if let Some(val) = args.first() {
            new_lifetime = Some(val.to_long());
        }
        if let Some(val) = args.get(1) {
            new_path = Some(val.to_php_string().to_string_lossy());
        }
        if let Some(val) = args.get(2) {
            new_domain = Some(val.to_php_string().to_string_lossy());
        }
        if let Some(val) = args.get(3) {
            new_secure = Some(val.is_truthy());
        }
        if let Some(val) = args.get(4) {
            new_httponly = Some(val.is_truthy());
        }
    }

    SESSION_STATE.with(|state| {
        let mut state = state.borrow_mut();
        if let Some(v) = new_lifetime {
            state.cookie_params.lifetime = v;
        }
        if let Some(ref v) = new_path {
            state.cookie_params.path = v.clone();
        }
        if let Some(ref v) = new_domain {
            state.cookie_params.domain = v.clone();
        }
        if let Some(v) = new_secure {
            state.cookie_params.secure = v;
        }
        if let Some(v) = new_httponly {
            state.cookie_params.httponly = v;
        }
        if let Some(ref v) = new_samesite {
            state.cookie_params.samesite = v.clone();
        }
    });

    // Mirror to INI so ini_get() reflects (PHP stores as strings "0"/"1" for bools).
    if let Some(v) = new_lifetime {
        vm.constants.insert(
            b"session.cookie_lifetime".to_vec(),
            Value::String(PhpString::from_string(v.to_string())),
        );
    }
    if let Some(v) = new_path {
        vm.constants.insert(
            b"session.cookie_path".to_vec(),
            Value::String(PhpString::from_string(v)),
        );
    }
    if let Some(v) = new_domain {
        vm.constants.insert(
            b"session.cookie_domain".to_vec(),
            Value::String(PhpString::from_string(v)),
        );
    }
    if let Some(v) = new_secure {
        vm.constants.insert(
            b"session.cookie_secure".to_vec(),
            Value::String(PhpString::from_bytes(if v { b"1" } else { b"0" })),
        );
    }
    if let Some(v) = new_httponly {
        vm.constants.insert(
            b"session.cookie_httponly".to_vec(),
            Value::String(PhpString::from_bytes(if v { b"1" } else { b"0" })),
        );
    }
    if let Some(v) = new_samesite {
        vm.constants.insert(
            b"session.cookie_samesite".to_vec(),
            Value::String(PhpString::from_string(v)),
        );
    }

    Ok(Value::True)
}

/// session_get_cookie_params(): array
fn session_get_cookie_params(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    ensure_ini_initialized(vm);
    let arr = SESSION_STATE.with(|state| {
        let state = state.borrow();
        let mut arr = PhpArray::new();
        arr.set(
            ArrayKey::String(PhpString::from_bytes(b"lifetime")),
            Value::Long(state.cookie_params.lifetime),
        );
        arr.set(
            ArrayKey::String(PhpString::from_bytes(b"path")),
            Value::String(PhpString::from_string(state.cookie_params.path.clone())),
        );
        arr.set(
            ArrayKey::String(PhpString::from_bytes(b"domain")),
            Value::String(PhpString::from_string(state.cookie_params.domain.clone())),
        );
        arr.set(
            ArrayKey::String(PhpString::from_bytes(b"secure")),
            if state.cookie_params.secure {
                Value::True
            } else {
                Value::False
            },
        );
        arr.set(
            ArrayKey::String(PhpString::from_bytes(b"httponly")),
            if state.cookie_params.httponly {
                Value::True
            } else {
                Value::False
            },
        );
        arr.set(
            ArrayKey::String(PhpString::from_bytes(b"samesite")),
            Value::String(PhpString::from_string(state.cookie_params.samesite.clone())),
        );
        arr
    });

    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

fn session_set_save_handler_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Check if first arg is an object implementing SessionHandlerInterface
    if args.is_empty() {
        return Ok(Value::False);
    }
    // For now, emit deprecation if using individual callbacks (not object form)
    if args.len() > 2 {
        emit_deprecated_obaware(vm, "session_set_save_handler(): Providing individual callbacks instead of an object implementing SessionHandlerInterface is deprecated");
    }
    // Stub: accept the handler but don't actually use it
    Ok(Value::True)
}

fn session_module_name_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    ensure_ini_initialized(vm);
    let current = SESSION_STATE.with(|state| state.borrow().module_name.clone());

    if let Some(new_val) = args.first() {
        if !matches!(new_val, Value::Null | Value::Undef) {
            let new_name = new_val.to_php_string().to_string_lossy();
            // PHP behaviour: check "session active" first, then validate the value.
            let started = SESSION_STATE.with(|state| state.borrow().started);
            if started {
                let suffix = started_from_suffix();
                emit_warning_obaware(vm, &format!(
                    "session_module_name(): Session save handler module cannot be changed when a session is active {}",
                    suffix
                ));
                return Ok(Value::False);
            }
            // "user" is not allowed via this function (use session_set_save_handler instead)
            if new_name == "user" {
                let msg = "session_module_name(): Argument #1 ($module) cannot be \"user\"".to_string();
                let line = vm.current_line;
                let exc = vm.create_exception(b"ValueError", &msg, line);
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg, line });
            }
            // Only "files" is currently a valid handler
            if new_name != "files" {
                emit_warning_obaware(vm, &format!(
                    "session_module_name(): Session handler module \"{}\" cannot be found",
                    new_name
                ));
                return Ok(Value::False);
            }
            SESSION_STATE.with(|state| {
                state.borrow_mut().module_name = new_name;
            });
        }
    }

    Ok(Value::String(PhpString::from_string(current)))
}
