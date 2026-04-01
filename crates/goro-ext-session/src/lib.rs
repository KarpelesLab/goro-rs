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
    id: String,
    name: String,
    save_path: String,
    data: HashMap<String, Value>,
    cookie_lifetime: i64,
    gc_maxlifetime: i64,
    cache_limiter: String,
    cache_expire: i64,
    cookie_params: CookieParams,
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
            id: String::new(),
            name: "PHPSESSID".to_string(),
            save_path: "/tmp".to_string(),
            data: HashMap::new(),
            cookie_lifetime: 0,
            gc_maxlifetime: 1440,
            cache_limiter: "nocache".to_string(),
            cache_expire: 180,
            cookie_params: CookieParams::default(),
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

/// Set $_SESSION as a global variable in the VM from session state data
fn set_session_global(vm: &mut Vm) {
    SESSION_STATE.with(|state| {
        let state = state.borrow();
        let mut arr = PhpArray::new();
        for (key, value) in &state.data {
            arr.set(
                ArrayKey::String(PhpString::from_string(key.clone())),
                value.clone(),
            );
        }
        vm.set_global(b"_SESSION".to_vec(), Value::Array(Rc::new(RefCell::new(arr))));
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

    // Register constants
    vm.constants
        .insert(b"PHP_SESSION_DISABLED".to_vec(), Value::Long(PHP_SESSION_DISABLED));
    vm.constants
        .insert(b"PHP_SESSION_NONE".to_vec(), Value::Long(PHP_SESSION_NONE));
    vm.constants
        .insert(b"PHP_SESSION_ACTIVE".to_vec(), Value::Long(PHP_SESSION_ACTIVE));
}

/// session_start(array $options = []): bool
fn session_start(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let already_started = SESSION_STATE.with(|state| state.borrow().started);
    if already_started {
        vm.emit_warning("Ignoring session_start() because a session is already active");
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

    SESSION_STATE.with(|state| {
        let mut state = state.borrow_mut();
        // Generate session ID if not already set
        if state.id.is_empty() {
            state.id = generate_session_id("");
        }

        // Try to load existing session data from file
        let file_path = session_file_path(&state.save_path, &state.id);
        if let Ok(contents) = std::fs::read_to_string(&file_path) {
            state.data = decode_session_data(&contents);
        }

        state.started = true;
    });

    // Set $_SESSION global
    set_session_global(vm);

    Ok(Value::True)
}

/// session_destroy(): bool
fn session_destroy(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let started = SESSION_STATE.with(|state| state.borrow().started);
    if !started {
        vm.emit_warning("Trying to destroy uninitialized session");
        return Ok(Value::False);
    }

    SESSION_STATE.with(|state| {
        let mut state = state.borrow_mut();
        // Delete session file
        let file_path = session_file_path(&state.save_path, &state.id);
        let _ = std::fs::remove_file(&file_path);
        state.started = false;
        state.data.clear();
    });

    Ok(Value::True)
}

/// session_id(string $id = ?): string|false
fn session_id(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let current_id = SESSION_STATE.with(|state| state.borrow().id.clone());

    if let Some(new_id_val) = args.first() {
        if !matches!(new_id_val, Value::Null | Value::Undef) {
            let new_id = new_id_val.to_php_string().to_string_lossy();
            let started = SESSION_STATE.with(|state| state.borrow().started);
            if started {
                vm.emit_warning("session_id(): Cannot change session id when session is active");
                return Ok(Value::String(PhpString::from_string(current_id)));
            }
            if !new_id.is_empty() && !is_valid_session_id(&new_id) {
                vm.emit_warning("session_id(): Invalid session id");
                return Ok(Value::String(PhpString::from_string(current_id)));
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
    let current_name = SESSION_STATE.with(|state| state.borrow().name.clone());

    if let Some(new_name_val) = args.first() {
        if !matches!(new_name_val, Value::Null | Value::Undef) {
            let new_name = new_name_val.to_php_string().to_string_lossy();
            if new_name.is_empty() {
                vm.emit_warning("session_name(): session.name cannot be an empty string");
                return Ok(Value::String(PhpString::from_string(current_name)));
            }
            SESSION_STATE.with(|state| {
                state.borrow_mut().name = new_name;
            });
        }
    }

    Ok(Value::String(PhpString::from_string(current_name)))
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

    SESSION_STATE.with(|state| {
        let mut state = state.borrow_mut();
        // Write session data to file
        let file_path = session_file_path(&state.save_path, &state.id);
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
        vm.emit_warning("session_reset(): Cannot reset session when no session is active");
        return Ok(Value::False);
    }

    // Re-read session data from file
    SESSION_STATE.with(|state| {
        let mut state = state.borrow_mut();
        let file_path = session_file_path(&state.save_path, &state.id);
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
        vm.emit_warning("session_regenerate_id(): Session is not active");
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

        if delete_old {
            let old_file = session_file_path(&state.save_path, &old_id);
            let _ = std::fs::remove_file(&old_file);
        }
    });

    Ok(Value::True)
}

/// session_save_path(string $path = ?): string|false
fn session_save_path(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let current_path = SESSION_STATE.with(|state| state.borrow().save_path.clone());

    if let Some(new_path_val) = args.first() {
        if !matches!(new_path_val, Value::Null | Value::Undef) {
            let new_path = new_path_val.to_php_string().to_string_lossy();
            let started = SESSION_STATE.with(|state| state.borrow().started);
            if started {
                vm.emit_warning(
                    "session_save_path(): Cannot change save path when session is active",
                );
                return Ok(Value::String(PhpString::from_string(current_path)));
            }
            SESSION_STATE.with(|state| {
                state.borrow_mut().save_path = new_path;
            });
        }
    }

    Ok(Value::String(PhpString::from_string(current_path)))
}

/// session_encode(): string|false
fn session_encode(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let started = SESSION_STATE.with(|state| state.borrow().started);
    if !started {
        vm.emit_warning("session_encode(): Cannot encode session data when no session is active");
        return Ok(Value::False);
    }

    // Sync from VM first
    sync_session_from_vm(vm);

    let encoded = SESSION_STATE.with(|state| {
        let state = state.borrow();
        encode_session_data(&state.data)
    });

    Ok(Value::String(PhpString::from_string(encoded)))
}

/// session_decode(string $data): bool
fn session_decode(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let started = SESSION_STATE.with(|state| state.borrow().started);
    if !started {
        vm.emit_warning("session_decode(): Cannot decode session data when no session is active");
        return Ok(Value::False);
    }

    let data = args
        .first()
        .unwrap_or(&Value::Null)
        .to_php_string()
        .to_string_lossy();

    let decoded = decode_session_data(&data);
    SESSION_STATE.with(|state| {
        state.borrow_mut().data = decoded;
    });

    // Update $_SESSION global
    set_session_global(vm);

    Ok(Value::True)
}

/// session_gc(): int|false
fn session_gc(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let (save_path, maxlifetime) = SESSION_STATE.with(|state| {
        let state = state.borrow();
        (state.save_path.clone(), state.gc_maxlifetime)
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
fn session_cache_limiter(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let current = SESSION_STATE.with(|state| state.borrow().cache_limiter.clone());

    if let Some(val) = args.first() {
        if !matches!(val, Value::Null | Value::Undef) {
            let new_val = val.to_php_string().to_string_lossy();
            SESSION_STATE.with(|state| {
                state.borrow_mut().cache_limiter = new_val;
            });
        }
    }

    Ok(Value::String(PhpString::from_string(current)))
}

/// session_cache_expire(?int $value = null): int|false
fn session_cache_expire(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let current = SESSION_STATE.with(|state| state.borrow().cache_expire);

    if let Some(val) = args.first() {
        if !matches!(val, Value::Null | Value::Undef) {
            let new_val = val.to_long();
            SESSION_STATE.with(|state| {
                state.borrow_mut().cache_expire = new_val;
            });
        }
    }

    Ok(Value::Long(current))
}

/// session_set_cookie_params(int|array $lifetime_or_options, ...): bool
fn session_set_cookie_params(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        return Ok(Value::False);
    }

    // Check if first arg is an array (options form)
    if let Value::Array(opts) = args.first().unwrap_or(&Value::Null) {
        let opts = opts.borrow();
        SESSION_STATE.with(|state| {
            let mut state = state.borrow_mut();
            for (key, val) in opts.iter() {
                if let ArrayKey::String(k) = &key {
                    let key_str = k.to_string_lossy();
                    match key_str.as_str() {
                        "lifetime" => state.cookie_params.lifetime = val.to_long(),
                        "path" => {
                            state.cookie_params.path = val.to_php_string().to_string_lossy()
                        }
                        "domain" => {
                            state.cookie_params.domain = val.to_php_string().to_string_lossy()
                        }
                        "secure" => state.cookie_params.secure = val.is_truthy(),
                        "httponly" => state.cookie_params.httponly = val.is_truthy(),
                        "samesite" => {
                            state.cookie_params.samesite = val.to_php_string().to_string_lossy()
                        }
                        _ => {}
                    }
                }
            }
        });
    } else {
        // Positional form: session_set_cookie_params(lifetime, path, domain, secure, httponly)
        SESSION_STATE.with(|state| {
            let mut state = state.borrow_mut();
            if let Some(val) = args.first() {
                state.cookie_params.lifetime = val.to_long();
            }
            if let Some(val) = args.get(1) {
                state.cookie_params.path = val.to_php_string().to_string_lossy();
            }
            if let Some(val) = args.get(2) {
                state.cookie_params.domain = val.to_php_string().to_string_lossy();
            }
            if let Some(val) = args.get(3) {
                state.cookie_params.secure = val.is_truthy();
            }
            if let Some(val) = args.get(4) {
                state.cookie_params.httponly = val.is_truthy();
            }
        });
    }

    Ok(Value::True)
}

/// session_get_cookie_params(): array
fn session_get_cookie_params(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
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
